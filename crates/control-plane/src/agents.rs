//! Guest-only Agent contracts. There is deliberately no production GuestChannel
//! implementation or host process fallback in this module.
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use uuid::Uuid;

const MAX_CATALOG_BYTES: usize = 128 * 1024;
const MAX_PROMPT_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const SENTINEL: &str = "step-one-local-bridge";
pub type AgentResult<T> = std::result::Result<T, AdapterError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterError {
    pub code: String,
    pub message: String,
}
impl AdapterError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AdapterError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    ClaudeCode,
    Codex,
    Dsh,
}
impl Provider {
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Dsh => "dsh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub package: String,
    pub version: String,
    pub tarball: String,
    pub integrity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallManifest {
    pub provider: Provider,
    pub version: String,
    pub artifacts: Vec<Artifact>,
    pub executable: String,
    pub native_member: String,
}

/// Reviewed host catalog. Claims such as archiveHashVerified are documentation,
/// never runtime evidence: install/probe receipts must verify the actual bytes.
pub struct Catalog {
    entries: BTreeMap<Provider, InstallManifest>,
}
impl Catalog {
    pub fn from_json(bytes: &[u8]) -> AgentResult<Self> {
        if bytes.is_empty() || bytes.len() > MAX_CATALOG_BYTES {
            return Err(invalid_catalog());
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid_catalog())?;
        if value["schemaVersion"] != 1
            || value["target"]["os"] != "linux"
            || value["target"]["architecture"] != "x86_64"
            || value["target"]["libc"] != "glibc"
        {
            return Err(invalid_catalog());
        }
        let agents = value["agents"].as_array().ok_or_else(invalid_catalog)?;
        let mut entries = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for entry in agents {
            let provider = match entry["id"].as_str() {
                Some("claude-code") => Provider::ClaudeCode,
                Some("codex") => Provider::Codex,
                Some("dsh") => Provider::Dsh,
                _ => return Err(invalid_catalog()),
            };
            if !seen.insert(provider) {
                return Err(invalid_catalog());
            }
            if provider == Provider::Dsh {
                if entry["status"] != "NOT_CONFIGURED"
                    || !entry["version"].is_null()
                    || entry["enabledByDefault"] != false
                    || entry["artifacts"]
                        .as_array()
                        .is_none_or(|items| !items.is_empty())
                {
                    return Err(invalid_catalog());
                }
                continue;
            }
            if entry["sourceKind"] != "official-npm" {
                return Err(invalid_catalog());
            }
            let version = entry["version"]
                .as_str()
                .filter(|value| fixed_version(value))
                .ok_or_else(invalid_catalog)?
                .to_owned();
            let expected = match provider {
                Provider::ClaudeCode => vec![
                    ("@anthropic-ai/claude-code", version.clone()),
                    ("@anthropic-ai/claude-code-linux-x64", version.clone()),
                ],
                Provider::Codex => vec![
                    ("@openai/codex", version.clone()),
                    ("@openai/codex", format!("{version}-linux-x64")),
                ],
                Provider::Dsh => unreachable!(),
            };
            let raw_artifacts = entry["artifacts"].as_array().ok_or_else(invalid_catalog)?;
            if raw_artifacts.len() != expected.len() {
                return Err(invalid_catalog());
            }
            let mut artifacts = vec![];
            for (raw, (package, artifact_version)) in raw_artifacts.iter().zip(expected) {
                let leaf = package.rsplit('/').next().ok_or_else(invalid_catalog)?;
                let tarball =
                    format!("https://registry.npmjs.org/{package}/-/{leaf}-{artifact_version}.tgz");
                if raw["package"] != package
                    || raw["version"] != artifact_version
                    || raw["tarball"] != tarball
                {
                    return Err(invalid_catalog());
                }
                let integrity = raw["integrity"].as_str().ok_or_else(invalid_catalog)?;
                let encoded = integrity
                    .strip_prefix("sha512-")
                    .ok_or_else(invalid_catalog)?;
                let digest = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|_| invalid_catalog())?;
                if digest.len() != 64
                    || base64::engine::general_purpose::STANDARD.encode(&digest) != encoded
                {
                    return Err(invalid_catalog());
                }
                artifacts.push(Artifact {
                    package: package.into(),
                    version: artifact_version,
                    tarball,
                    integrity: integrity.into(),
                });
            }
            let (member, executable) = match provider {
                Provider::ClaudeCode => ("package/claude", format!("/opt/step-one/agents/claude-code/{version}/claude")),
                Provider::Codex => ("package/vendor/x86_64-unknown-linux-musl/bin/codex", format!("/opt/step-one/agents/codex/{version}/vendor/x86_64-unknown-linux-musl/bin/codex")),
                Provider::Dsh => unreachable!(),
            };
            if entry["installation"]["guestExecutable"] != executable
                || entry["installation"]["binaryInNativeTarball"] != member
            {
                return Err(invalid_catalog());
            }
            entries.insert(
                provider,
                InstallManifest {
                    provider,
                    version,
                    artifacts,
                    executable,
                    native_member: member.into(),
                },
            );
        }
        if seen != [Provider::ClaudeCode, Provider::Codex, Provider::Dsh].into() {
            return Err(invalid_catalog());
        }
        Ok(Self { entries })
    }
    pub fn manifest(&self, provider: Provider) -> AgentResult<&InstallManifest> {
        self.entries.get(&provider).ok_or_else(|| {
            AdapterError::new("NOT_CONFIGURED", "此 Agent 的可信来源和启动契约尚未配置。")
        })
    }
}
fn fixed_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 6
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}
fn invalid_catalog() -> AdapterError {
    AdapterError::new(
        "INVALID_CATALOG",
        "Agent 安装清单未通过固定来源、版本或完整性检查。",
    )
}

/// Only a trusted runtime provider may produce this evidence. Booleans are a
/// contract seam for tests, not a substitute for authenticated peer/VM checks.
#[derive(Debug, Clone)]
pub struct GuestEvidence {
    pub resource_id: Uuid,
    pub authenticated_channel: bool,
    pub kernel_isolation_ready: bool,
    pub guest_service_ready: bool,
    pub model_proxy_ready: bool,
    pub proxy_ignores_client_auth: bool,
    pub streaming_ready: bool,
}

#[derive(Clone)]
pub struct GuestBinding {
    resource_id: Uuid,
    channel: Arc<dyn GuestChannel>,
}
impl std::fmt::Debug for GuestBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GuestBinding([AUTHENTICATED-VM])")
    }
}
impl GuestBinding {
    pub fn connect(resource_id: Uuid, channel: Arc<dyn GuestChannel>) -> AgentResult<Self> {
        if resource_id.is_nil() {
            return Err(not_ready());
        }
        let binding = Self {
            resource_id,
            channel,
        };
        binding.ensure_channel_ready()?;
        Ok(binding)
    }
    fn ensure_channel_ready(&self) -> AgentResult<GuestEvidence> {
        let evidence = self
            .channel
            .attest(self.resource_id)
            .map_err(|_| not_ready())?;
        if evidence.resource_id != self.resource_id
            || !evidence.authenticated_channel
            || !evidence.kernel_isolation_ready
            || !evidence.guest_service_ready
        {
            return Err(not_ready());
        }
        Ok(evidence)
    }
    fn ensure_ready(&self) -> AgentResult<()> {
        let evidence = self.ensure_channel_ready()?;
        if !evidence.model_proxy_ready
            || !evidence.proxy_ignores_client_auth
            || !evidence.streaming_ready
        {
            return Err(not_ready());
        }
        Ok(())
    }
}
fn not_ready() -> AdapterError {
    AdapterError::new(
        "GUEST_NOT_READY",
        "经过认证的沙箱、模型桥接和流式通道尚未就绪。",
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReceipt {
    pub resource_id: Uuid,
    pub version: String,
    pub executable: String,
    pub artifact_integrities: Vec<String>,
    pub archive_members_verified: bool,
}

/// No inherited environment or caller-controlled shell flags. Guest service
/// writes only these fixed configuration paths before execve, never host files.
#[derive(Clone)]
pub struct LaunchPlan {
    pub argv: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub cwd: String,
    pub inherit_environment: bool,
}
impl std::fmt::Debug for LaunchPlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LaunchPlan([GUEST-ONLY, PROMPT REDACTED])")
    }
}

#[derive(Debug, Clone)]
pub struct GuestProcessReceipt {
    pub resource_id: Uuid,
    pub process_id: Uuid,
}
#[derive(Clone)]
pub struct ProcessHandle {
    resource_id: Uuid,
    process_id: Uuid,
    provider: Provider,
    adapter_id: Uuid,
}
impl std::fmt::Debug for ProcessHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessHandle")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}
#[derive(Clone)]
pub struct ProcessUpdate {
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
impl std::fmt::Debug for ProcessUpdate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessUpdate")
            .field("exited", &self.exited)
            .field("exit_code", &self.exit_code)
            .field("output", &"[REDACTED]")
            .finish()
    }
}

/// All operations occur in the authenticated Linux guest. A production provider
/// must verify SRI before transfer/extraction, reject archive links/traversal,
/// bound output and execution time, stop the full guest process group and verify
/// resource_id on every request. There is intentionally no host Command fallback.
pub trait GuestChannel: Send + Sync {
    fn attest(&self, resource_id: Uuid) -> AgentResult<GuestEvidence>;
    fn install(&self, resource_id: Uuid, manifest: &InstallManifest)
        -> AgentResult<InstallReceipt>;
    fn probe(
        &self,
        resource_id: Uuid,
        manifest: &InstallManifest,
    ) -> AgentResult<Option<InstallReceipt>>;
    fn start(&self, resource_id: Uuid, plan: &LaunchPlan) -> AgentResult<GuestProcessReceipt>;
    fn stop(&self, resource_id: Uuid, process_id: Uuid) -> AgentResult<ProcessUpdate>;
    fn poll(&self, resource_id: Uuid, process_id: Uuid) -> AgentResult<ProcessUpdate>;
}

pub struct AgentAdapter {
    provider: Provider,
    manifest: InstallManifest,
    binding: GuestBinding,
    adapter_id: Uuid,
}
impl AgentAdapter {
    pub fn new(provider: Provider, catalog: &Catalog, binding: GuestBinding) -> AgentResult<Self> {
        Ok(Self {
            provider,
            manifest: catalog.manifest(provider)?.clone(),
            binding,
            adapter_id: Uuid::new_v4(),
        })
    }
    fn verify(&self, receipt: &InstallReceipt) -> AgentResult<()> {
        if receipt.resource_id != self.binding.resource_id
            || receipt.version != self.manifest.version
            || receipt.executable != self.manifest.executable
            || receipt.artifact_integrities
                != self
                    .manifest
                    .artifacts
                    .iter()
                    .map(|item| item.integrity.clone())
                    .collect::<Vec<_>>()
            || !receipt.archive_members_verified
        {
            return Err(AdapterError::new(
                "INSTALLATION_MISMATCH",
                "沙箱内 Agent 的版本、来源完整性或文件位置与清单不符。",
            ));
        }
        Ok(())
    }
    pub fn install(&self) -> AgentResult<()> {
        self.binding.ensure_channel_ready()?;
        let receipt = self
            .binding
            .channel
            .install(self.binding.resource_id, &self.manifest)
            .map_err(|_| guest_error())?;
        self.verify(&receipt)
    }
    pub fn start(&self, prompt: &str) -> AgentResult<ProcessHandle> {
        self.binding.ensure_ready()?;
        let receipt = self
            .binding
            .channel
            .probe(self.binding.resource_id, &self.manifest)
            .map_err(|_| guest_error())?
            .ok_or_else(|| {
                AdapterError::new("AGENT_NOT_INSTALLED", "请先在沙箱中安装经过验证的 Agent。")
            })?;
        self.verify(&receipt)?;
        let plan = self.plan(prompt)?;
        // Recheck after installation/probe so revocation cannot be mistaken for
        // an authenticated current binding. In-flight guest RPC cancellation is
        // a responsibility of the production channel implementation.
        self.binding.ensure_ready()?;
        let started = self
            .binding
            .channel
            .start(self.binding.resource_id, &plan)
            .map_err(|_| guest_error())?;
        if started.resource_id != self.binding.resource_id || started.process_id.is_nil() {
            return Err(AdapterError::new(
                "PROCESS_BINDING_MISMATCH",
                "沙箱进程身份未通过验证。",
            ));
        }
        Ok(ProcessHandle {
            resource_id: started.resource_id,
            process_id: started.process_id,
            provider: self.provider,
            adapter_id: self.adapter_id,
        })
    }
    pub fn stop(&self, handle: &ProcessHandle) -> AgentResult<ProcessUpdate> {
        self.check_handle(handle)?;
        self.binding.ensure_channel_ready()?;
        // Cleanup must remain possible if the model proxy was revoked. The
        // channel itself still authenticates the VM and the opaque process ID.
        let update = self
            .binding
            .channel
            .stop(handle.resource_id, handle.process_id)
            .map_err(|_| guest_error())?;
        let update = validate_update(update)?;
        if !update.exited {
            return Err(AdapterError::new(
                "CLEANUP_INCOMPLETE",
                "沙箱进程尚未确认停止，保留会话记录。",
            ));
        }
        Ok(update)
    }
    pub fn poll(&self, handle: &ProcessHandle) -> AgentResult<ProcessUpdate> {
        self.check_handle(handle)?;
        self.binding.ensure_channel_ready()?;
        validate_update(
            self.binding
                .channel
                .poll(handle.resource_id, handle.process_id)
                .map_err(|_| guest_error())?,
        )
    }
    fn check_handle(&self, handle: &ProcessHandle) -> AgentResult<()> {
        if handle.resource_id != self.binding.resource_id
            || handle.provider != self.provider
            || handle.adapter_id != self.adapter_id
        {
            return Err(AdapterError::new(
                "PROCESS_BINDING_MISMATCH",
                "进程不属于此沙箱或 Agent。",
            ));
        }
        Ok(())
    }
    pub fn plan(&self, prompt: &str) -> AgentResult<LaunchPlan> {
        if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES || prompt.contains('\0') {
            return Err(AdapterError::new(
                "INVALID_PROMPT",
                "请求必须为不含空字符的非空文本，且大小在允许范围内。",
            ));
        }
        let mut plan = LaunchPlan {
            argv: vec![self.manifest.executable.clone()],
            environment: BTreeMap::new(),
            files: BTreeMap::new(),
            cwd: "/workspace".into(),
            inherit_environment: false,
        };
        plan.environment.insert("HOME".into(), "/home/agent".into());
        plan.environment
            .insert("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into());
        match self.provider {
            Provider::ClaudeCode => {
                plan.argv
                    .extend(["--print".into(), "--".into(), prompt.into()]);
                plan.environment.insert(
                    "ANTHROPIC_BASE_URL".into(),
                    "http://127.0.0.1:19100/anthropic".into(),
                );
                // Protocol-only sentinel. The authenticated bridge ignores and
                // replaces it; it grants no host broker authority by itself.
                plan.environment
                    .insert("ANTHROPIC_AUTH_TOKEN".into(), SENTINEL.into());
                plan.environment
                    .insert("DISABLE_UPDATES".into(), "1".into());
                plan.environment.insert(
                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
                    "1".into(),
                );
                let managed = serde_json::json!({"allowedProviders":["customEndpoint"],"env": {
                    "ANTHROPIC_BASE_URL":"http://127.0.0.1:19100/anthropic", "ANTHROPIC_AUTH_TOKEN":SENTINEL,
                    "DISABLE_UPDATES":"1", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC":"1"}});
                plan.files.insert(
                    "/etc/claude-code/managed-settings.json".into(),
                    serde_json::to_vec(&managed).map_err(|_| guest_error())?,
                );
            }
            Provider::Codex => {
                plan.argv
                    .extend(["exec".into(), "--".into(), prompt.into()]);
                plan.environment
                    .insert("CODEX_HOME".into(), "/home/agent/.codex".into());
                plan.files.insert("/home/agent/.codex/config.toml".into(), b"model_provider = \"step_one\"\n[model_providers.step_one]\nname = \"Step-One host broker\"\nbase_url = \"http://127.0.0.1:19100/openai/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n".to_vec());
            }
            Provider::Dsh => {
                return Err(AdapterError::new(
                    "NOT_CONFIGURED",
                    "dsh 可信安装源和启动契约尚未确认。",
                ))
            }
        }
        Ok(plan)
    }
}
fn guest_error() -> AdapterError {
    AdapterError::new(
        "GUEST_OPERATION_FAILED",
        "沙箱操作未确认完成，请检查已绑定的沙箱通道。",
    )
}
fn validate_update(update: ProcessUpdate) -> AgentResult<ProcessUpdate> {
    if update.stdout.len().saturating_add(update.stderr.len()) > MAX_OUTPUT_BYTES {
        return Err(AdapterError::new("OUTPUT_LIMIT", "沙箱输出超出限制。"));
    }
    if update.exited != update.exit_code.is_some() {
        return Err(AdapterError::new(
            "INVALID_PROCESS_STATE",
            "沙箱进程状态不一致。",
        ));
    }
    Ok(update)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn bundled_catalog_matches_the_verified_install_contract() {
        let catalog = Catalog::from_json(include_bytes!("../catalog/agents.json")).unwrap();
        assert_eq!(
            catalog.manifest(Provider::Codex).unwrap().version,
            "0.164.0"
        );
        assert_eq!(
            catalog.manifest(Provider::ClaudeCode).unwrap().version,
            "2.1.310"
        );
        assert_eq!(
            catalog.manifest(Provider::Dsh).unwrap_err().code,
            "NOT_CONFIGURED"
        );
    }
    fn catalog_json() -> Value {
        let integrity = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode([42u8; 64])
        );
        let artifact = |package: &str, version: &str| {
            serde_json::json!({"package":package,"version":version,
            "tarball":format!("https://registry.npmjs.org/{package}/-/{leaf}-{version}.tgz",leaf=package.rsplit('/').next().unwrap()),"integrity":integrity})
        };
        serde_json::json!({"schemaVersion":1,"target":{"os":"linux","architecture":"x86_64","libc":"glibc"},"agents":[
            {"id":"claude-code","version":"1.2.3","sourceKind":"official-npm","artifacts":[artifact("@anthropic-ai/claude-code","1.2.3"),artifact("@anthropic-ai/claude-code-linux-x64","1.2.3")],"installation":{"guestExecutable":"/opt/step-one/agents/claude-code/1.2.3/claude","binaryInNativeTarball":"package/claude"}},
            {"id":"codex","version":"4.5.6","sourceKind":"official-npm","artifacts":[artifact("@openai/codex","4.5.6"),artifact("@openai/codex","4.5.6-linux-x64")],"installation":{"guestExecutable":"/opt/step-one/agents/codex/4.5.6/vendor/x86_64-unknown-linux-musl/bin/codex","binaryInNativeTarball":"package/vendor/x86_64-unknown-linux-musl/bin/codex"}},
            {"id":"dsh","version":null,"enabledByDefault":false,"artifacts":[],"status":"NOT_CONFIGURED"}]})
    }
    fn catalog() -> Catalog {
        Catalog::from_json(&serde_json::to_vec(&catalog_json()).unwrap()).unwrap()
    }
    struct FakeChannel {
        evidence: Mutex<GuestEvidence>,
        calls: Mutex<Vec<String>>,
        installed: Mutex<bool>,
        wrong_version: Mutex<bool>,
        output_size: Mutex<usize>,
        fail_stop: Mutex<bool>,
        plans: Mutex<Vec<LaunchPlan>>,
    }
    impl FakeChannel {
        fn new(resource_id: Uuid) -> Self {
            Self {
                evidence: Mutex::new(GuestEvidence {
                    resource_id,
                    authenticated_channel: true,
                    kernel_isolation_ready: true,
                    guest_service_ready: true,
                    model_proxy_ready: true,
                    proxy_ignores_client_auth: true,
                    streaming_ready: true,
                }),
                calls: Mutex::new(vec![]),
                installed: Mutex::new(false),
                wrong_version: Mutex::new(false),
                output_size: Mutex::new(0),
                fail_stop: Mutex::new(false),
                plans: Mutex::new(vec![]),
            }
        }
        fn receipt(&self, id: Uuid, manifest: &InstallManifest) -> InstallReceipt {
            InstallReceipt {
                resource_id: id,
                version: if *self.wrong_version.lock().unwrap() {
                    "other-version".into()
                } else {
                    manifest.version.clone()
                },
                executable: manifest.executable.clone(),
                artifact_integrities: manifest
                    .artifacts
                    .iter()
                    .map(|artifact| artifact.integrity.clone())
                    .collect(),
                archive_members_verified: true,
            }
        }
    }
    impl GuestChannel for FakeChannel {
        fn attest(&self, _: Uuid) -> AgentResult<GuestEvidence> {
            Ok(self.evidence.lock().unwrap().clone())
        }
        fn install(&self, id: Uuid, manifest: &InstallManifest) -> AgentResult<InstallReceipt> {
            self.calls.lock().unwrap().push("install".into());
            *self.installed.lock().unwrap() = true;
            Ok(self.receipt(id, manifest))
        }
        fn probe(
            &self,
            id: Uuid,
            manifest: &InstallManifest,
        ) -> AgentResult<Option<InstallReceipt>> {
            self.calls.lock().unwrap().push("probe".into());
            Ok((*self.installed.lock().unwrap()).then(|| self.receipt(id, manifest)))
        }
        fn start(&self, id: Uuid, plan: &LaunchPlan) -> AgentResult<GuestProcessReceipt> {
            self.calls.lock().unwrap().push("start".into());
            self.plans.lock().unwrap().push(plan.clone());
            Ok(GuestProcessReceipt {
                resource_id: id,
                process_id: Uuid::new_v4(),
            })
        }
        fn stop(&self, _: Uuid, _: Uuid) -> AgentResult<ProcessUpdate> {
            self.calls.lock().unwrap().push("stop".into());
            if *self.fail_stop.lock().unwrap() {
                return Err(AdapterError::new("secret", "raw secret diagnostic"));
            }
            Ok(ProcessUpdate {
                exited: true,
                exit_code: Some(0),
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn poll(&self, _: Uuid, _: Uuid) -> AgentResult<ProcessUpdate> {
            self.calls.lock().unwrap().push("poll".into());
            Ok(ProcessUpdate {
                exited: false,
                exit_code: None,
                stdout: vec![b'x'; *self.output_size.lock().unwrap()],
                stderr: vec![],
            })
        }
    }
    fn fixture(provider: Provider) -> (AgentAdapter, Arc<FakeChannel>) {
        let resource = Uuid::new_v4();
        let channel = Arc::new(FakeChannel::new(resource));
        let binding = GuestBinding::connect(resource, channel.clone()).unwrap();
        (
            AgentAdapter::new(provider, &catalog(), binding).unwrap(),
            channel,
        )
    }

    #[test]
    fn pinned_catalog_rejects_source_hash_version_binary_tampering() {
        assert!(catalog().manifest(Provider::ClaudeCode).is_ok());
        assert_eq!(
            catalog().manifest(Provider::Dsh).unwrap_err().code,
            "NOT_CONFIGURED"
        );
        for pointer in [
            "/agents/0/version",
            "/agents/0/artifacts/0/tarball",
            "/agents/0/artifacts/1/integrity",
            "/agents/0/installation/guestExecutable",
            "/agents/0/installation/binaryInNativeTarball",
        ] {
            let mut value = catalog_json();
            *value.pointer_mut(pointer).unwrap() = Value::String("attacker".into());
            assert!(Catalog::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut value = catalog_json();
        value["agents"].as_array_mut().unwrap().remove(2);
        assert!(Catalog::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(Catalog::from_json(&vec![b'x'; MAX_CATALOG_BYTES + 1]).is_err());
    }
    #[test]
    fn ready_channel_and_exact_install_receipt_are_required() {
        let (adapter, channel) = fixture(Provider::Codex);
        assert_eq!(
            adapter.start("hello").unwrap_err().code,
            "AGENT_NOT_INSTALLED"
        );
        *channel.wrong_version.lock().unwrap() = true;
        assert_eq!(adapter.install().unwrap_err().code, "INSTALLATION_MISMATCH");
        assert_eq!(
            adapter.start("hello").unwrap_err().code,
            "INSTALLATION_MISMATCH"
        );
        assert!(!channel.calls.lock().unwrap().contains(&"start".into()));
        *channel.wrong_version.lock().unwrap() = false;
        adapter.install().unwrap();
        channel.evidence.lock().unwrap().streaming_ready = false;
        assert_eq!(adapter.start("hello").unwrap_err().code, "GUEST_NOT_READY");
        channel.evidence.lock().unwrap().streaming_ready = true;
        assert!(adapter.start("hello").is_ok());
    }
    #[test]
    fn launch_plans_have_fixed_proxy_no_ambient_keys_and_prompt_is_one_argument() {
        for provider in [Provider::ClaudeCode, Provider::Codex] {
            let (adapter, channel) = fixture(provider);
            adapter.install().unwrap();
            let prompt = "--config evil; $(do not execute)\nprivate prompt";
            adapter.start(prompt).unwrap();
            let plans = channel.plans.lock().unwrap();
            let plan = &plans[0];
            assert_eq!(plan.argv.last().unwrap(), prompt);
            assert_eq!(plan.argv[plan.argv.len() - 2], "--");
            assert!(!plan.inherit_environment);
            assert_eq!(plan.cwd, "/workspace");
            assert!(!plan.environment.contains_key("OPENAI_API_KEY"));
            assert!(!plan.environment.contains_key("ANTHROPIC_API_KEY"));
            assert!(!format!("{plan:?}").contains(prompt));
            if provider == Provider::Codex {
                let config =
                    std::str::from_utf8(&plan.files["/home/agent/.codex/config.toml"]).unwrap();
                assert!(config.contains("requires_openai_auth = false"));
                assert!(config.contains("http://127.0.0.1:19100/openai/v1"));
                assert!(!config.contains("env_key"));
            } else {
                assert_eq!(plan.environment["ANTHROPIC_AUTH_TOKEN"], SENTINEL);
                assert_eq!(
                    plan.environment["ANTHROPIC_BASE_URL"],
                    "http://127.0.0.1:19100/anthropic"
                );
            }
        }
    }
    #[test]
    fn process_handles_are_adapter_scoped_and_cleanup_survives_proxy_revocation() {
        let (adapter, channel) = fixture(Provider::ClaudeCode);
        adapter.install().unwrap();
        let handle = adapter.start("hello").unwrap();
        let other =
            AgentAdapter::new(Provider::ClaudeCode, &catalog(), adapter.binding.clone()).unwrap();
        assert_eq!(
            other.poll(&handle).unwrap_err().code,
            "PROCESS_BINDING_MISMATCH"
        );
        channel.evidence.lock().unwrap().model_proxy_ready = false;
        assert!(adapter.stop(&handle).unwrap().exited);
        *channel.fail_stop.lock().unwrap() = true;
        let error = adapter.stop(&handle).unwrap_err();
        assert_eq!(error.code, "GUEST_OPERATION_FAILED");
        assert!(!error.message.contains("secret"));
        channel.evidence.lock().unwrap().authenticated_channel = false;
        assert_eq!(adapter.poll(&handle).unwrap_err().code, "GUEST_NOT_READY");
    }
    #[test]
    fn invalid_binding_and_bounded_outputs_fail_closed() {
        let resource = Uuid::new_v4();
        let channel = Arc::new(FakeChannel::new(resource));
        assert!(GuestBinding::connect(Uuid::nil(), channel.clone()).is_err());
        assert!(GuestBinding::connect(Uuid::new_v4(), channel).is_err());
        let (adapter, channel) = fixture(Provider::Codex);
        adapter.install().unwrap();
        let handle = adapter.start("hello").unwrap();
        *channel.output_size.lock().unwrap() = MAX_OUTPUT_BYTES + 1;
        assert_eq!(adapter.poll(&handle).unwrap_err().code, "OUTPUT_LIMIT");
        assert!(adapter.plan("").is_err());
        assert!(adapter.plan("nul\0").is_err());
        assert_eq!(
            validate_update(ProcessUpdate {
                exited: true,
                exit_code: None,
                stdout: vec![],
                stderr: vec![]
            })
            .unwrap_err()
            .code,
            "INVALID_PROCESS_STATE"
        );
    }
}

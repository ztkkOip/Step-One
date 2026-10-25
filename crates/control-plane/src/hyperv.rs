//! Hyper-V VM lifecycle adapter. This does not install/run agent processes or
//! mount workspaces. Guest I/O and hardware isolation acceptance remain gates.
use crate::model::{Agent, ControlError, Result, RuntimeInfo, PERMISSIONS};
use crate::runtime::{Runtime, RuntimeAction, RuntimeObservation};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub const CPU_COUNT: u32 = 2;
pub const MEMORY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const DISK_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const OPERATION_TIMEOUT_SECONDS: u64 = 120;

/// Host-admin image manifest, never accepted from the agent or Tauri requests.
/// The script rechecks file identity/type/hash immediately before create/reset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HyperVConfig {
    pub image_path: PathBuf,
    pub image_sha256: String,
    pub base_root: PathBuf,
}

impl HyperVConfig {
    pub fn validate(&self) -> Result<()> {
        if !safe_absolute(&self.image_path)
            || !safe_absolute(&self.base_root)
            || self.base_root.parent().is_none()
            || self.image_path.parent().is_none()
            || self
                .image_path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("vhdx"))
            || self.image_sha256.len() != 64
            || !self
                .image_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ControlError::new(
                "INVALID_IMAGE_MANIFEST",
                "沙箱镜像清单无效，请重新配置宿主机镜像。",
            ));
        }
        // The base image must not be in the managed deletion subtree.
        if self.image_path.starts_with(&self.base_root) {
            return Err(ControlError::new(
                "INVALID_IMAGE_MANIFEST",
                "基础镜像必须保存在会话资源目录之外。",
            ));
        }
        Ok(())
    }
}

fn safe_absolute(path: &Path) -> bool {
    let Some(value) = path.to_str() else {
        return false;
    };
    path.is_absolute()
        && !value.chars().any(char::is_control)
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HyperVAction {
    Probe,
    Inspect,
    Create,
    Start,
    Stop,
    Reset,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HyperVRequest {
    pub action: HyperVAction,
    pub resource_id: Option<String>,
    pub image_path: PathBuf,
    pub image_sha256: String,
    pub base_root: PathBuf,
    pub cpu_count: u32,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HyperVResponse {
    pub ok: bool,
    pub available: bool,
    pub state: String,
    pub owned: bool,
    pub checks: Vec<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}

/// Invoke the bundled, immutable script with JSON on stdin. Implementations may
/// not pass agent text to PowerShell source or expose a generic shell operation.
/// The production runner enforces deadline/output caps and a kill-on-close job.
/// A script's nonzero exit may still contain a valid failed response.
pub trait HyperVRunner: Send + Sync {
    fn invoke(&self, request: &HyperVRequest) -> Result<HyperVResponse>;
}

pub struct HyperVRuntime {
    config: HyperVConfig,
    runner: Arc<dyn HyperVRunner>,
    available: bool,
}

impl HyperVRuntime {
    /// A failed capability probe produces an unavailable Hyper-V runtime. It
    /// never silently selects the simulator or attempts to elevate the desktop.
    pub fn initialize(config: HyperVConfig, runner: Arc<dyn HyperVRunner>) -> Result<Self> {
        config.validate()?;
        let mut runtime = Self {
            config,
            runner,
            available: false,
        };
        let response = runtime
            .runner
            .invoke(&runtime.request(HyperVAction::Probe, None));
        runtime.available = matches!(
            response,
            Ok(HyperVResponse {
                ok: true,
                available: true,
                ..
            })
        );
        Ok(runtime)
    }

    fn request(&self, action: HyperVAction, resource_id: Option<String>) -> HyperVRequest {
        HyperVRequest {
            action,
            resource_id,
            image_path: self.config.image_path.clone(),
            image_sha256: self.config.image_sha256.to_ascii_lowercase(),
            base_root: self.config.base_root.clone(),
            cpu_count: CPU_COUNT,
            memory_bytes: MEMORY_BYTES,
            disk_bytes: DISK_BYTES,
            timeout_seconds: OPERATION_TIMEOUT_SECONDS,
        }
    }

    fn check_agent(&self, agent: &Agent) -> Result<String> {
        if agent.runtime_kind != "hyperv" {
            return Err(ControlError::new(
                "RUNTIME_MISMATCH",
                "此 Agent 不属于 Hyper-V 运行时。",
            ));
        }
        let id = Uuid::parse_str(&agent.resource_id)
            .map_err(|_| ControlError::new("OWNERSHIP_DENIED", "会话资源标识无效，拒绝操作。"))?;
        if id.is_nil() || id.hyphenated().to_string() != agent.resource_id {
            return Err(ControlError::new(
                "OWNERSHIP_DENIED",
                "会话资源标识无效，拒绝操作。",
            ));
        }
        Ok(id.hyphenated().to_string())
    }

    fn invoke(&self, agent: &Agent, action: HyperVAction) -> Result<RuntimeObservation> {
        let resource_id = self.check_agent(agent)?;
        if !self.available {
            return Err(ControlError::new(
                "PREREQUISITE_MISSING",
                "Hyper-V 前置条件未满足，请检查系统版本、虚拟化和管理权限。",
            ));
        }
        let response = self
            .runner
            .invoke(&self.request(action, Some(resource_id)))
            .map_err(|error| mapped_error(Some(&error.code)))?;
        if !response.ok || !response.available {
            return Err(mapped_error(response.error_code.as_deref()));
        }
        // A resource which exists but lacks ownership must never be acted on or
        // represented as one of our guests. Missing resources may be unowned.
        if response.state != "missing" && !response.owned {
            return Err(mapped_error(Some("OWNERSHIP_DENIED")));
        }
        let allowed = match action {
            HyperVAction::Inspect => {
                ["missing", "ready", "running", "stopped"].contains(&response.state.as_str())
            }
            HyperVAction::Create | HyperVAction::Reset => {
                response.state == "ready" || response.state == "stopped"
            }
            HyperVAction::Start => response.state == "running",
            HyperVAction::Stop => response.state == "stopped" || response.state == "missing",
            HyperVAction::Delete => response.state == "missing",
            HyperVAction::Probe => false,
        };
        if !allowed {
            return Err(mapped_error(Some("INVALID_RESPONSE")));
        }
        // There is no guest process channel yet. VM boot success must not set
        // Agent.status=running or imply that an installer/agent executed.
        Ok(RuntimeObservation::new(&response.state, "stopped"))
    }
}

impl Runtime for HyperVRuntime {
    fn info(&self) -> RuntimeInfo {
        RuntimeInfo {
            kind: "hyperv".into(),
            available: self.available,
            isolation_enforced: false,
            capabilities: PERMISSIONS
                .iter()
                .map(|key| ((*key).into(), false))
                .collect(),
            message: if self.available {
                "Hyper-V 生命周期可用；工作区与 Agent 通道尚未启用，真实隔离仍待硬件验收。"
            } else {
                "Hyper-V 不可用；需要受支持的 Windows、虚拟化与管理权限。不会自动切换模拟模式。"
            }
            .into(),
        }
    }

    fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation> {
        self.invoke(agent, HyperVAction::Inspect)
    }

    fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation> {
        let action = match action {
            RuntimeAction::Create => HyperVAction::Create,
            RuntimeAction::Start => HyperVAction::Start,
            RuntimeAction::Stop => HyperVAction::Stop,
            RuntimeAction::Reset => HyperVAction::Reset,
            RuntimeAction::Delete => HyperVAction::Delete,
            RuntimeAction::GrantPermission(_) | RuntimeAction::RevokePermission(_) => {
                return Err(ControlError::new(
                    "UNSUPPORTED_CAPABILITY",
                    "此运行时尚未实现权限执行，不会授予实际访问能力。",
                ));
            }
        };
        self.invoke(agent, action)
    }
}

fn mapped_error(code: Option<&str>) -> ControlError {
    let safe_code = match code {
        Some("PREREQUISITE_MISSING") => "PREREQUISITE_MISSING",
        Some("IMAGE_INVALID") => "IMAGE_INVALID",
        Some("OWNERSHIP_DENIED") => "OWNERSHIP_DENIED",
        Some("TIMEOUT") => "TIMEOUT",
        Some("CLEANUP_FAILED") => "CLEANUP_FAILED",
        Some("INVALID_REQUEST") => "INVALID_REQUEST",
        Some("INVALID_RESPONSE") => "INVALID_RESPONSE",
        _ => "RUNTIME_FAILED",
    };
    let mut error = ControlError::new(
        safe_code,
        "虚拟机操作未确认完成；资源归属记录已保留，请检查运行环境。",
    );
    error.retryable = matches!(safe_code, "TIMEOUT" | "CLEANUP_FAILED");
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    struct FakeRunner {
        requests: Mutex<Vec<HyperVRequest>>,
        responses: Mutex<VecDeque<Result<HyperVResponse>>>,
    }
    impl FakeRunner {
        fn new(responses: Vec<Result<HyperVResponse>>) -> Self {
            Self {
                requests: Mutex::new(vec![]),
                responses: Mutex::new(responses.into()),
            }
        }
    }
    impl HyperVRunner for FakeRunner {
        fn invoke(&self, request: &HyperVRequest) -> Result<HyperVResponse> {
            self.requests.lock().unwrap().push(request.clone());
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected command")
        }
    }
    fn config() -> HyperVConfig {
        let root = std::env::current_dir().unwrap();
        HyperVConfig {
            image_path: root.join("base/image.vhdx"),
            image_sha256: "a".repeat(64),
            base_root: root.join("private-vms"),
        }
    }
    fn response(state: &str) -> Result<HyperVResponse> {
        Ok(HyperVResponse {
            ok: true,
            available: true,
            state: state.into(),
            owned: state != "missing",
            checks: vec![],
            error_code: None,
        })
    }
    fn agent() -> Agent {
        Agent {
            id: Uuid::new_v4().to_string(),
            name: "untrusted; Remove-Item".into(),
            provider: "Codex".into(),
            status: "ready".into(),
            workspace: "private workspace".into(),
            workspace_grant_id: String::new(),
            security_mode: "Protected".into(),
            version: "not-installed".into(),
            last_activity: String::new(),
            accent: "blue".into(),
            permissions: BTreeMap::new(),
            runtime_kind: "hyperv".into(),
            runtime_state: "ready".into(),
            resource_id: Uuid::new_v4().to_string(),
            operation_id: None,
            last_error: None,
        }
    }

    #[test]
    fn unavailable_probe_never_falls_back_or_runs_an_operation() {
        for probe in [
            Err(ControlError::new("host-secret", "sensitive")),
            Ok(HyperVResponse {
                ok: false,
                available: false,
                state: "missing".into(),
                owned: false,
                checks: vec!["host-secret".into()],
                error_code: Some("PREREQUISITE_MISSING".into()),
            }),
        ] {
            let runner = Arc::new(FakeRunner::new(vec![probe]));
            let runtime = HyperVRuntime::initialize(config(), runner.clone()).unwrap();
            assert!(!runtime.info().available);
            assert_eq!(runtime.info().kind, "hyperv");
            assert!(!runtime.info().message.contains("host-secret"));
            assert_eq!(
                runtime
                    .execute(&agent(), &RuntimeAction::Create)
                    .unwrap_err()
                    .code,
                "PREREQUISITE_MISSING"
            );
            assert_eq!(runner.requests.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn fixed_vm_contract_never_passes_user_commands_or_workspace_data() {
        let runner = Arc::new(FakeRunner::new(vec![
            response("missing"),
            response("ready"),
            response("running"),
            response("stopped"),
            response("ready"),
            response("missing"),
        ]));
        let runtime = HyperVRuntime::initialize(config(), runner.clone()).unwrap();
        let agent = agent();
        for action in [
            RuntimeAction::Create,
            RuntimeAction::Start,
            RuntimeAction::Stop,
            RuntimeAction::Reset,
            RuntimeAction::Delete,
        ] {
            let state = runtime.execute(&agent, &action).unwrap();
            assert_eq!(state.agent_status, "stopped");
        }
        let requests = runner.requests.lock().unwrap();
        for request in requests.iter().skip(1) {
            assert_eq!(request.resource_id.as_ref().unwrap(), &agent.resource_id);
            assert_eq!(request.cpu_count, 2);
            assert_eq!(request.memory_bytes, MEMORY_BYTES);
            assert_eq!(request.disk_bytes, DISK_BYTES);
            assert_eq!(request.timeout_seconds, 120);
            let encoded = serde_json::to_string(request).unwrap();
            assert!(!encoded.contains(&agent.name));
            assert!(!encoded.contains(&agent.workspace));
        }
        assert!(!runtime.info().isolation_enforced);
        assert!(runtime.info().capabilities.values().all(|allowed| !allowed));
    }

    #[test]
    fn invalid_resource_ids_and_permissions_never_reach_runner() {
        let runner = Arc::new(FakeRunner::new(vec![response("missing")]));
        let runtime = HyperVRuntime::initialize(config(), runner.clone()).unwrap();
        for id in [
            "",
            "../../owned",
            "';Invoke-Expression x",
            "00000000-0000-0000-0000-000000000000",
        ] {
            let mut agent = agent();
            agent.resource_id = id.into();
            assert_eq!(
                runtime
                    .execute(&agent, &RuntimeAction::Delete)
                    .unwrap_err()
                    .code,
                "OWNERSHIP_DENIED"
            );
        }
        let mut wrong = agent();
        wrong.runtime_kind = "simulator".into();
        assert_eq!(
            runtime.inspect(&wrong).unwrap_err().code,
            "RUNTIME_MISMATCH"
        );
        assert_eq!(
            runtime
                .execute(&agent(), &RuntimeAction::GrantPermission("network".into()))
                .unwrap_err()
                .code,
            "UNSUPPORTED_CAPABILITY"
        );
        assert_eq!(runner.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn ownership_and_partial_failure_responses_fail_closed() {
        let mut unowned = response("running").unwrap();
        unowned.owned = false;
        let mut failed = response("running").unwrap();
        failed.ok = false;
        failed.error_code = Some("CLEANUP_FAILED".into());
        let runner = Arc::new(FakeRunner::new(vec![
            response("missing"),
            Ok(unowned),
            Ok(failed),
            response("unexpected"),
        ]));
        let runtime = HyperVRuntime::initialize(config(), runner).unwrap();
        let agent = agent();
        assert_eq!(
            runtime.inspect(&agent).unwrap_err().code,
            "OWNERSHIP_DENIED"
        );
        assert_eq!(
            runtime
                .execute(&agent, &RuntimeAction::Delete)
                .unwrap_err()
                .code,
            "CLEANUP_FAILED"
        );
        assert_eq!(
            runtime
                .execute(&agent, &RuntimeAction::Start)
                .unwrap_err()
                .code,
            "INVALID_RESPONSE"
        );
    }

    #[test]
    fn manifest_rejects_unfixed_hash_relative_paths_and_managed_image() {
        for invalid in ["", "abc", &"x".repeat(64)] {
            let mut config = config();
            config.image_sha256 = invalid.into();
            assert!(config.validate().is_err());
        }
        let mut invalid = config();
        invalid.base_root = PathBuf::from("relative");
        assert!(invalid.validate().is_err());
        let mut invalid = config();
        invalid.image_path = invalid.base_root.join("image.vhdx");
        assert!(invalid.validate().is_err());
        let mut invalid = config();
        invalid.image_path = std::env::current_dir().unwrap().join("image.exe");
        assert!(invalid.validate().is_err());
        assert!(config().validate().is_ok());
    }
}

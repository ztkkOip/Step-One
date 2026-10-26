use crate::model::*;
use crate::runtime::{Runtime, RuntimeAction, RuntimeObservation};
use crate::storage::Store;
use crate::workspace::{WorkspaceGrant, WorkspaceRegistry};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// The state lock protects short state transitions and SQLite commits only.
/// Durable pending markers serialize each agent; runtime work runs lock-free.
pub struct ControlService {
    state: Mutex<PersistedState>,
    store: Store,
    runtime: Arc<dyn Runtime>,
}

impl ControlService {
    pub fn open(path: &Path, runtime: Arc<dyn Runtime>) -> Result<Self> {
        let store = Store::open(path)?;
        let mut state = store.load()?.unwrap_or_else(|| PersistedState {
            schema_version: 1,
            console: Console::new(runtime.info()),
            grants: vec![],
        });
        if state.schema_version != 1 {
            return Err(ControlError::new(
                "SCHEMA_UNSUPPORTED",
                "本地数据版本不受支持，请升级应用。",
            ));
        }
        state.console.runtime = runtime.info();
        state.console.approvals.clear();
        for agent in &mut state.console.agents {
            let pending = agent.operation_id.take();
            let observed = if agent.runtime_kind != state.console.runtime.kind {
                Err(ControlError::new(
                    "RUNTIME_MISMATCH",
                    "此 Agent 的运行时当前不可用。",
                ))
            } else {
                runtime.inspect(agent)
            };
            match observed {
                Ok(observation) => {
                    agent.runtime_state = observation.runtime_state;
                    agent.status = observation.agent_status;
                    if agent.runtime_state == "missing" {
                        agent.last_error = Some(
                            ControlError::new(
                                "RESOURCE_MISSING",
                                "上次会话资源已不存在；请重新启动或重置。",
                            )
                            .retryable(),
                        );
                    }
                }
                Err(_) => {
                    agent.runtime_state = "failed".into();
                    agent.status = "unknown".into();
                    agent.last_error = Some(
                        ControlError::new(
                            "RECOVERY_FAILED",
                            "无法核对上次会话资源，请检查运行环境。",
                        )
                        .retryable(),
                    );
                }
            }
            if let Some(id) = pending {
                let error = ControlError::new(
                    "OPERATION_INTERRUPTED",
                    "上次操作被中断；已重新核对资源状态，请检查后重试。",
                )
                .retryable()
                .for_operation(&id);
                agent.last_error = Some(error.clone());
                if let Some(operation) = state
                    .console
                    .operations
                    .iter_mut()
                    .find(|operation| operation.id == id)
                {
                    operation.state = "failed".into();
                    operation.error = Some(error);
                }
            }
        }
        if !state.console.agents.is_empty() {
            record(
                &mut state.console,
                "恢复会话状态",
                "已核对资源；模拟会话不会自动重新运行。",
            );
        }
        store.save(&state)?;
        Ok(Self {
            state: Mutex::new(state),
            store,
            runtime,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, PersistedState>> {
        self.state
            .lock()
            .map_err(|_| ControlError::new("STATE_UNAVAILABLE", "控制台状态不可用，请重新启动。"))
    }

    fn commit(&self, state: &mut PersistedState, next: PersistedState) -> Result<Console> {
        self.store.save(&next)?;
        *state = next;
        Ok(state.console.clone())
    }

    pub fn snapshot(&self) -> Result<Console> {
        Ok(self.lock()?.console.clone())
    }

    /// Call only after the native folder picker returns; raw IPC cannot grant paths.
    pub fn authorize_workspace(&self, path: &Path) -> Result<WorkspaceGrant> {
        let mut state = self.lock()?;
        let mut next = state.clone();
        let mut registry = WorkspaceRegistry::restore(next.grants.clone());
        let grant = registry.grant(path)?;
        next.grants = registry.grants();
        self.commit(&mut state, next)?;
        Ok(grant)
    }

    pub fn create(&self, input: CreateAgentInput) -> Result<Console> {
        let name = input.name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(ControlError::new(
                "INVALID_INPUT",
                "名称必须为 1–80 个可显示字符。",
            ));
        }
        if !["Claude Code", "Codex", "DeepSeek Harness"].contains(&input.provider.as_str())
            || !["Protected", "Developer"].contains(&input.security_mode.as_str())
        {
            return Err(ControlError::new(
                "INVALID_INPUT",
                "不支持的 Agent 类型或安全模式。",
            ));
        }
        let agent = {
            let mut state = self.lock()?;
            let mut next = state.clone();
            let info = &next.console.runtime;
            if !info.available
                || input
                    .runtime_kind
                    .as_ref()
                    .is_some_and(|kind| kind != &info.kind)
            {
                return Err(ControlError::new(
                    "RUNTIME_UNAVAILABLE",
                    "所选运行时不可用；不会自动切换为模拟模式。",
                ));
            }
            let workspace = WorkspaceRegistry::restore(next.grants.clone())
                .revalidate(&input.workspace_grant_id)?;
            let accent = match input.provider.as_str() {
                "Claude Code" => "violet",
                "Codex" => "blue",
                _ => "orange",
            };
            let mut agent = Agent {
                id: Uuid::new_v4().to_string(),
                name: name.into(),
                provider: input.provider,
                status: "ready".into(),
                workspace: workspace.path,
                workspace_grant_id: workspace.id,
                security_mode: input.security_mode,
                version: "not-installed".into(),
                last_activity: "刚刚".into(),
                accent: accent.into(),
                permissions: PERMISSIONS
                    .iter()
                    .map(|key| ((*key).into(), false))
                    .collect(),
                runtime_kind: info.kind.clone(),
                runtime_state: "creating".into(),
                resource_id: Uuid::new_v4().to_string(),
                operation_id: None,
                last_error: None,
            };
            begin(&mut next.console, &mut agent, &RuntimeAction::Create);
            next.console.agents.insert(0, agent.clone());
            self.commit(&mut state, next)?;
            agent
        };
        self.execute(agent, RuntimeAction::Create)
    }

    pub fn action(&self, id: &str, action: &str) -> Result<Console> {
        let action = match action {
            "start" => RuntimeAction::Start,
            "stop" => RuntimeAction::Stop,
            _ => {
                return Err(ControlError::new(
                    "INVALID_TRANSITION",
                    "重置和删除需要单独确认。",
                ))
            }
        };
        let agent = {
            let mut state = self.lock()?;
            let mut next = state.clone();
            let index = position(&next.console, id)?;
            let mut agent = next.console.agents[index].clone();
            ensure_idle(&agent)?;
            ensure_no_approval(&next.console, id)?;
            if agent.runtime_kind != next.console.runtime.kind || !next.console.runtime.available {
                return Err(ControlError::new(
                    "RUNTIME_UNAVAILABLE",
                    "此 Agent 的运行时不可用。",
                ));
            }
            match (&action, agent.status.as_str()) {
                (RuntimeAction::Start, "ready" | "stopped") if runtime_is_stopped(&agent) => {
                    WorkspaceRegistry::restore(next.grants.clone())
                        .revalidate(&agent.workspace_grant_id)?;
                }
                (RuntimeAction::Stop, _)
                    if agent.status == "running"
                        || agent.status == "unknown"
                        || !runtime_is_stopped(&agent) => {}
                _ => {
                    return Err(ControlError::new(
                        "INVALID_TRANSITION",
                        "当前状态不支持此操作。",
                    ))
                }
            }
            begin(&mut next.console, &mut agent, &action);
            next.console.agents[index] = agent.clone();
            self.commit(&mut state, next)?;
            agent
        };
        self.execute(agent, action)
    }

    pub fn request(&self, id: &str, action: &str) -> Result<Console> {
        let mut state = self.lock()?;
        let mut next = state.clone();
        let agent = &next.console.agents[position(&next.console, id)?];
        ensure_editable(agent)?;
        ensure_no_approval(&next.console, id)?;
        if action != "reset" && action != "delete" && !PERMISSIONS.contains(&action) {
            return Err(ControlError::new("INVALID_INPUT", "未知权限或操作。"));
        }
        if PERMISSIONS.contains(&action) {
            ensure_capability(&next.console.runtime, action)?;
            if agent.permissions.get(action) == Some(&true) {
                return Err(ControlError::new("ALREADY_GRANTED", "权限已经允许。"));
            }
        }
        let title = match action {
            "reset" => "重置环境",
            "delete" => "删除 Agent",
            "workspaceWrite" => "允许修改工作区",
            "network" => "允许网络访问",
            "mcpTools" => "允许调用 MCP 工具",
            "clipboard" => "允许读取剪贴板",
            _ => "允许导出文件",
        };
        let detail = if next.console.runtime.isolation_enforced {
            format!(
                "{}：确认后仅对此 Agent 执行操作。工作区文件不会因删除或重置而被删除。",
                agent.name
            )
        } else {
            format!(
                "{}：当前为模拟模式，仅改变模拟状态，不授予真实访问权限。",
                agent.name
            )
        };
        next.console.approvals.push(Approval {
            id: Uuid::new_v4().to_string(),
            agent_id: id.into(),
            action: action.into(),
            title: title.into(),
            detail,
        });
        record(&mut next.console, "请求用户确认", title);
        self.commit(&mut state, next)
    }

    pub fn decide(&self, approval_id: &str, allowed: bool) -> Result<Console> {
        let (agent, action) = {
            let mut state = self.lock()?;
            let mut next = state.clone();
            let approval_index = next
                .console
                .approvals
                .iter()
                .position(|approval| approval.id == approval_id)
                .ok_or_else(|| ControlError::new("APPROVAL_EXPIRED", "请求已处理或不存在。"))?;
            let approval = next.console.approvals[approval_index].clone();
            let index = position(&next.console, &approval.agent_id)?;
            let mut agent = next.console.agents[index].clone();
            ensure_editable(&agent)?;
            if !allowed {
                next.console.approvals.remove(approval_index);
                record(&mut next.console, "用户拒绝", &approval.title);
                return self.commit(&mut state, next);
            }
            if agent.runtime_kind != next.console.runtime.kind || !next.console.runtime.available {
                return Err(ControlError::new(
                    "RUNTIME_UNAVAILABLE",
                    "此 Agent 的运行时不可用。",
                ));
            }
            let action = match approval.action.as_str() {
                "reset" => RuntimeAction::Reset,
                "delete" => RuntimeAction::Delete,
                key => {
                    ensure_capability(&next.console.runtime, key)?;
                    WorkspaceRegistry::restore(next.grants.clone())
                        .revalidate(&agent.workspace_grant_id)?;
                    RuntimeAction::GrantPermission(key.into())
                }
            };
            next.console.approvals.remove(approval_index);
            record(&mut next.console, "用户确认", &approval.title);
            begin(&mut next.console, &mut agent, &action);
            next.console.agents[index] = agent.clone();
            self.commit(&mut state, next)?;
            (agent, action)
        };
        self.execute(agent, action)
    }

    pub fn revoke(&self, id: &str, key: &str) -> Result<Console> {
        if !PERMISSIONS.contains(&key) {
            return Err(ControlError::new("INVALID_INPUT", "未知权限。"));
        }
        let action = RuntimeAction::RevokePermission(key.into());
        let agent = {
            let mut state = self.lock()?;
            let mut next = state.clone();
            let index = position(&next.console, id)?;
            let mut agent = next.console.agents[index].clone();
            ensure_editable(&agent)?;
            ensure_no_approval(&next.console, id)?;
            if agent.runtime_kind != next.console.runtime.kind || !next.console.runtime.available {
                return Err(ControlError::new(
                    "RUNTIME_UNAVAILABLE",
                    "此 Agent 的运行时不可用。",
                ));
            }
            begin(&mut next.console, &mut agent, &action);
            next.console.agents[index] = agent.clone();
            self.commit(&mut state, next)?;
            agent
        };
        self.execute(agent, action)
    }

    fn execute(&self, agent: Agent, action: RuntimeAction) -> Result<Console> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.runtime.execute(&agent, &action)
        }))
        .unwrap_or_else(|_| Err(ControlError::new("RUNTIME_FAILED", "运行时操作失败。")));
        // Failure may follow partial side effects. Reconcile before exposing the
        // state; never assume a failed start left the guest stopped.
        let observed_after_failure = if result.is_err() {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.runtime.inspect(&agent)
            }))
            .ok()
            .and_then(std::result::Result::ok)
        } else {
            None
        };
        self.finish(&agent, &action, result, observed_after_failure)
    }

    fn finish(
        &self,
        agent: &Agent,
        action: &RuntimeAction,
        result: Result<RuntimeObservation>,
        observed_after_failure: Option<RuntimeObservation>,
    ) -> Result<Console> {
        let operation_id = agent
            .operation_id
            .as_deref()
            .ok_or_else(|| ControlError::new("STATE_UNAVAILABLE", "操作记录缺失。"))?;
        let mut state = self.lock()?;
        let mut next = state.clone();
        let index = position(&next.console, &agent.id)?;
        let error = result
            .as_ref()
            .err()
            .map(|error| safe_runtime_error(error, operation_id));
        match result {
            Ok(observation) => {
                if *action == RuntimeAction::Delete {
                    next.console.agents.remove(index);
                } else {
                    let saved = &mut next.console.agents[index];
                    saved.runtime_state = observation.runtime_state;
                    saved.status = observation.agent_status;
                    saved.operation_id = None;
                    saved.last_error = None;
                    saved.last_activity = "刚刚".into();
                    match action {
                        RuntimeAction::Reset => {
                            for value in saved.permissions.values_mut() {
                                *value = false;
                            }
                        }
                        RuntimeAction::GrantPermission(key) => {
                            saved.permissions.insert(key.clone(), true);
                        }
                        RuntimeAction::RevokePermission(key) => {
                            saved.permissions.insert(key.clone(), false);
                        }
                        _ => {}
                    }
                }
                record(&mut next.console, "操作完成", action.name());
            }
            Err(_) => {
                let saved = &mut next.console.agents[index];
                saved.operation_id = None;
                if let Some(observation) = observed_after_failure {
                    // Keep actual VM state even when the operation failed. A
                    // running VM can have no running Agent process yet.
                    saved.runtime_state = observation.runtime_state;
                    saved.status = observation.agent_status;
                } else {
                    saved.runtime_state = "failed".into();
                    saved.status = "unknown".into();
                }
                saved.last_error = error.clone();
                record(&mut next.console, "操作失败", action.name());
            }
        }
        if let Some(operation) = next
            .console
            .operations
            .iter_mut()
            .find(|operation| operation.id == operation_id)
        {
            operation.state = if error.is_some() {
                "failed"
            } else {
                "succeeded"
            }
            .into();
            operation.error = error.clone();
        }
        let snapshot = self.commit(&mut state, next)?;
        match error {
            Some(error) => Err(error),
            None => Ok(snapshot),
        }
    }
}

fn position(console: &Console, id: &str) -> Result<usize> {
    console
        .agents
        .iter()
        .position(|agent| agent.id == id)
        .ok_or_else(|| ControlError::new("NOT_FOUND", "Agent 不存在，请刷新列表。"))
}
fn ensure_idle(agent: &Agent) -> Result<()> {
    if agent.operation_id.is_some() {
        Err(ControlError::new("OPERATION_PENDING", "已有操作正在处理，请稍候。").retryable())
    } else {
        Ok(())
    }
}
fn ensure_editable(agent: &Agent) -> Result<()> {
    ensure_idle(agent)?;
    if agent.status == "running" || agent.status == "unknown" || !runtime_is_stopped(agent) {
        Err(ControlError::new(
            "STOP_REQUIRED",
            "请先停止会话并确认停止结果。",
        ))
    } else {
        Ok(())
    }
}
fn runtime_is_stopped(agent: &Agent) -> bool {
    matches!(
        agent.runtime_state.as_str(),
        "ready" | "stopped" | "missing"
    )
}
fn ensure_no_approval(console: &Console, id: &str) -> Result<()> {
    if console
        .approvals
        .iter()
        .any(|approval| approval.agent_id == id)
    {
        Err(ControlError::new(
            "APPROVAL_PENDING",
            "请先处理待确认请求。",
        ))
    } else {
        Ok(())
    }
}
fn ensure_capability(info: &RuntimeInfo, key: &str) -> Result<()> {
    if info.capabilities.get(key) != Some(&true) {
        Err(ControlError::new(
            "UNSUPPORTED_CAPABILITY",
            "当前运行时尚不支持此能力。",
        ))
    } else {
        Ok(())
    }
}
fn begin(console: &mut Console, agent: &mut Agent, action: &RuntimeAction) {
    let id = Uuid::new_v4().to_string();
    agent.operation_id = Some(id.clone());
    agent.runtime_state = action.pending_state().into();
    agent.last_error = None;
    console.operations.push(Operation {
        id,
        agent_id: agent.id.clone(),
        action: action.name().into(),
        state: "pending".into(),
        error: None,
    });
    while console.operations.len() > 500 {
        if let Some(index) = console
            .operations
            .iter()
            .position(|operation| operation.state != "pending")
        {
            console.operations.remove(index);
        } else {
            break;
        }
    }
    record(console, "操作开始", action.name());
}
fn record(console: &mut Console, title: &str, detail: &str) {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    console.events.insert(
        0,
        AuditEvent {
            id: Uuid::new_v4().to_string(),
            time,
            title: title.into(),
            detail: detail.into(),
        },
    );
    console.events.truncate(500);
}
fn safe_runtime_error(error: &ControlError, operation_id: &str) -> ControlError {
    let code = match error.code.as_str() {
        "TIMEOUT"
        | "RESOURCE_MISSING"
        | "CLEANUP_FAILED"
        | "RUNTIME_UNAVAILABLE"
        | "UNSUPPORTED_CAPABILITY"
        | "IMAGE_INVALID"
        | "OWNERSHIP_DENIED"
        | "INVALID_RESPONSE"
        | "INVALID_REQUEST"
        | "PREREQUISITE_MISSING" => error.code.as_str(),
        _ => "RUNTIME_FAILED",
    };
    let mut safe = ControlError::new(
        code,
        "运行时操作未完成；资源记录已保留，请检查运行环境后重试。",
    )
    .for_operation(operation_id);
    safe.retryable = error.retryable;
    safe
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::SimulatorRuntime;
    use std::sync::{mpsc, Condvar};
    use std::time::Duration;

    struct Fixture {
        _temp: tempfile::TempDir,
        path: std::path::PathBuf,
        workspace: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
            let path = temp.path().join("state.sqlite");
            let workspace = temp.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            Self {
                _temp: temp,
                path,
                workspace,
            }
        }
        fn service(&self) -> ControlService {
            ControlService::open(&self.path, Arc::new(SimulatorRuntime::default())).unwrap()
        }
        fn create(&self, service: &ControlService) -> Agent {
            let grant = service.authorize_workspace(&self.workspace).unwrap();
            service
                .create(CreateAgentInput {
                    name: "Test".into(),
                    provider: "Codex".into(),
                    workspace_grant_id: grant.id,
                    security_mode: "Protected".into(),
                    runtime_kind: Some("simulator".into()),
                })
                .unwrap()
                .agents[0]
                .clone()
        }
    }

    #[test]
    fn approvals_are_required_scoped_and_single_use() {
        let f = Fixture::new();
        let service = f.service();
        let agent = f.create(&service);
        assert!(agent.permissions.values().all(|v| !v));
        assert_eq!(
            service.action(&agent.id, "delete").unwrap_err().code,
            "INVALID_TRANSITION"
        );
        let request = service.request(&agent.id, "network").unwrap();
        let approval = &request.approvals[0].id;
        assert!(!request.agents[0].permissions["network"]);
        assert_eq!(
            service.action(&agent.id, "start").unwrap_err().code,
            "APPROVAL_PENDING"
        );
        assert_eq!(
            service.request(&agent.id, "clipboard").unwrap_err().code,
            "APPROVAL_PENDING"
        );
        assert_eq!(
            service.revoke(&agent.id, "network").unwrap_err().code,
            "APPROVAL_PENDING"
        );
        assert!(service.decide(approval, true).unwrap().agents[0].permissions["network"]);
        assert_eq!(
            service.decide(approval, true).unwrap_err().code,
            "APPROVAL_EXPIRED"
        );
        assert_eq!(
            service.request(&agent.id, "network").unwrap_err().code,
            "ALREADY_GRANTED"
        );
        assert!(!service.revoke(&agent.id, "network").unwrap().agents[0].permissions["network"]);
    }

    #[test]
    fn golden_path_reset_delete_and_audit_are_persistent() {
        let f = Fixture::new();
        let service = f.service();
        let agent = f.create(&service);
        assert_eq!(
            service.action(&agent.id, "stop").unwrap_err().code,
            "INVALID_TRANSITION"
        );
        assert_eq!(
            service.action(&agent.id, "start").unwrap().agents[0].status,
            "running"
        );
        assert_eq!(
            service.request(&agent.id, "delete").unwrap_err().code,
            "STOP_REQUIRED"
        );
        assert_eq!(
            service.revoke(&agent.id, "network").unwrap_err().code,
            "STOP_REQUIRED"
        );
        service.action(&agent.id, "stop").unwrap();
        let request = service.request(&agent.id, "workspaceWrite").unwrap();
        service.decide(&request.approvals[0].id, true).unwrap();
        let request = service.request(&agent.id, "reset").unwrap();
        let reset = service.decide(&request.approvals[0].id, true).unwrap();
        assert!(reset.agents[0].permissions.values().all(|v| !v));
        assert_eq!(reset.agents[0].status, "ready");
        let request = service.request(&agent.id, "delete").unwrap();
        assert_eq!(
            service
                .decide(&request.approvals[0].id, false)
                .unwrap()
                .agents
                .len(),
            1
        );
        let request = service.request(&agent.id, "delete").unwrap();
        let deleted = service.decide(&request.approvals[0].id, true).unwrap();
        assert!(deleted.agents.is_empty());
        assert!(!deleted.events.is_empty());
        drop(service);
        let reopened = f.service().snapshot().unwrap();
        assert!(reopened.agents.is_empty());
        assert_eq!(reopened.events.len(), deleted.events.len());
        assert!(f.workspace.exists());
    }

    #[test]
    fn restart_does_not_claim_missing_simulator_is_running() {
        let f = Fixture::new();
        let service = f.service();
        let agent = f.create(&service);
        service.action(&agent.id, "start").unwrap();
        drop(service);
        let service = f.service();
        let recovered = service.snapshot().unwrap();
        assert_eq!(recovered.agents[0].status, "stopped");
        assert_eq!(recovered.agents[0].runtime_state, "missing");
        assert_eq!(
            recovered.agents[0].last_error.as_ref().unwrap().code,
            "RESOURCE_MISSING"
        );
        assert_eq!(
            service.action(&agent.id, "start").unwrap().agents[0].status,
            "running"
        );
    }

    #[test]
    fn validates_input_grants_runtime_and_permission_names() {
        let f = Fixture::new();
        let service = f.service();
        let grant = service.authorize_workspace(&f.workspace).unwrap();
        let valid = CreateAgentInput {
            name: "Valid".into(),
            provider: "Codex".into(),
            workspace_grant_id: grant.id,
            security_mode: "Protected".into(),
            runtime_kind: None,
        };
        for name in ["", "\n", &"a".repeat(81)] {
            let mut input = valid.clone();
            input.name = name.into();
            assert_eq!(service.create(input).unwrap_err().code, "INVALID_INPUT");
        }
        for (provider, mode) in [("Shell", "Protected"), ("Codex", "unrestricted")] {
            let mut input = valid.clone();
            input.provider = provider.into();
            input.security_mode = mode.into();
            assert_eq!(service.create(input).unwrap_err().code, "INVALID_INPUT");
        }
        let mut input = valid.clone();
        input.workspace_grant_id = "guessed".into();
        assert_eq!(
            service.create(input).unwrap_err().code,
            "WORKSPACE_NOT_AUTHORIZED"
        );
        let mut input = valid.clone();
        input.runtime_kind = Some("hyperv".into());
        assert_eq!(
            service.create(input).unwrap_err().code,
            "RUNTIME_UNAVAILABLE"
        );
        let agent = service.create(valid).unwrap().agents[0].clone();
        assert_eq!(
            service.action("missing", "start").unwrap_err().code,
            "NOT_FOUND"
        );
        assert_eq!(
            service.request(&agent.id, "hostShell").unwrap_err().code,
            "INVALID_INPUT"
        );
        assert_eq!(
            service.revoke(&agent.id, "hostShell").unwrap_err().code,
            "INVALID_INPUT"
        );
        std::fs::remove_dir(&f.workspace).unwrap();
        assert!(service.action(&agent.id, "start").is_err());
    }

    #[derive(Default)]
    struct FailingRuntime {
        inner: SimulatorRuntime,
    }
    impl Runtime for FailingRuntime {
        fn info(&self) -> RuntimeInfo {
            let mut info = self.inner.info();
            info.capabilities.insert("network".into(), false);
            info
        }
        fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation> {
            self.inner.inspect(agent)
        }
        fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation> {
            if *action == RuntimeAction::Delete {
                Err(ControlError::new("CLEANUP_FAILED", "raw API key: very-sensitive").retryable())
            } else {
                self.inner.execute(agent, action)
            }
        }
    }

    #[test]
    fn unsupported_capability_and_failed_cleanup_fail_closed() {
        let f = Fixture::new();
        let service = ControlService::open(&f.path, Arc::new(FailingRuntime::default())).unwrap();
        let agent = f.create(&service);
        assert_eq!(
            service.request(&agent.id, "network").unwrap_err().code,
            "UNSUPPORTED_CAPABILITY"
        );
        let approval = service.request(&agent.id, "delete").unwrap().approvals[0]
            .id
            .clone();
        let error = service.decide(&approval, true).unwrap_err();
        assert_eq!(error.code, "CLEANUP_FAILED");
        assert!(!error.message.contains("sensitive"));
        assert!(error.retryable);
        let snapshot = service.snapshot().unwrap();
        assert_eq!(snapshot.agents[0].resource_id, agent.resource_id);
        assert_eq!(snapshot.agents[0].runtime_state, "ready");
        assert_eq!(
            snapshot.agents[0].last_error.as_ref().unwrap().code,
            "CLEANUP_FAILED"
        );
        assert!(snapshot.approvals.is_empty());
        assert_eq!(snapshot.operations.last().unwrap().state, "failed");
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("very-sensitive"));
        assert_eq!(
            service.decide(&approval, true).unwrap_err().code,
            "APPROVAL_EXPIRED"
        );
    }

    struct BlockingRuntime {
        inner: SimulatorRuntime,
        entered: Mutex<Option<mpsc::Sender<()>>>,
        gate: (Mutex<bool>, Condvar),
    }
    impl Runtime for BlockingRuntime {
        fn info(&self) -> RuntimeInfo {
            self.inner.info()
        }
        fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation> {
            self.inner.inspect(agent)
        }
        fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation> {
            if *action == RuntimeAction::Start {
                if let Some(sender) = self.entered.lock().unwrap().take() {
                    sender.send(()).unwrap();
                }
                let (lock, cv) = &self.gate;
                let open = lock.lock().unwrap();
                let (open, _) = cv
                    .wait_timeout_while(open, Duration::from_secs(5), |open| !*open)
                    .unwrap();
                assert!(*open, "test gate timed out");
            }
            self.inner.execute(agent, action)
        }
    }

    #[test]
    fn pending_runtime_does_not_block_snapshot_and_serializes_same_agent() {
        let f = Fixture::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let runtime = Arc::new(BlockingRuntime {
            inner: SimulatorRuntime::default(),
            entered: Mutex::new(Some(entered_tx)),
            gate: (Mutex::new(false), Condvar::new()),
        });
        let service = Arc::new(ControlService::open(&f.path, runtime.clone()).unwrap());
        let agent = f.create(&service);
        let worker = {
            let service = service.clone();
            let id = agent.id.clone();
            std::thread::spawn(move || service.action(&id, "start"))
        };
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let snapshot = service.snapshot().unwrap();
        assert_eq!(snapshot.agents[0].runtime_state, "starting");
        assert_eq!(snapshot.agents[0].status, "ready");
        assert!(snapshot.agents[0].operation_id.is_some());
        assert_eq!(
            service.action(&agent.id, "start").unwrap_err().code,
            "OPERATION_PENDING"
        );
        assert_eq!(
            service.request(&agent.id, "delete").unwrap_err().code,
            "OPERATION_PENDING"
        );
        assert!(f.create(&service).id != agent.id);
        *runtime.gate.0.lock().unwrap() = true;
        runtime.gate.1.notify_one();
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn interrupted_operation_recovers_without_forgetting_ownership() {
        let f = Fixture::new();
        let service = f.service();
        let agent = f.create(&service);
        drop(service);
        let store = Store::open(&f.path).unwrap();
        let mut persisted = store.load().unwrap().unwrap();
        let mut pending = persisted.console.agents[0].clone();
        begin(&mut persisted.console, &mut pending, &RuntimeAction::Delete);
        let pending_id = pending.operation_id.clone().unwrap();
        persisted.console.agents[0] = pending;
        store.save(&persisted).unwrap();
        drop(store);
        let snapshot = f.service().snapshot().unwrap();
        assert_eq!(snapshot.agents[0].resource_id, agent.resource_id);
        assert_eq!(
            snapshot.agents[0].last_error.as_ref().unwrap().code,
            "OPERATION_INTERRUPTED"
        );
        assert!(snapshot.agents[0].operation_id.is_none());
        assert_eq!(
            snapshot
                .operations
                .iter()
                .find(|op| op.id == pending_id)
                .unwrap()
                .state,
            "failed"
        );
    }

    #[test]
    fn storage_failure_does_not_publish_or_execute_create() {
        let f = Fixture::new();
        let service = f.service();
        let grant = service.authorize_workspace(&f.workspace).unwrap();
        let db = rusqlite::Connection::open(&f.path).unwrap();
        db.execute_batch("CREATE TRIGGER fail BEFORE UPDATE ON control_state BEGIN SELECT RAISE(ABORT, 'secret'); END;").unwrap();
        let result = service.create(CreateAgentInput {
            name: "test".into(),
            provider: "Codex".into(),
            workspace_grant_id: grant.id,
            security_mode: "Protected".into(),
            runtime_kind: None,
        });
        assert_eq!(result.unwrap_err().code, "STORAGE_UNAVAILABLE");
        assert!(service.snapshot().unwrap().agents.is_empty());
    }

    struct PartialStartRuntime {
        inner: SimulatorRuntime,
        inspect_fails: bool,
    }
    impl Runtime for PartialStartRuntime {
        fn info(&self) -> RuntimeInfo {
            self.inner.info()
        }
        fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation> {
            if self.inspect_fails {
                Err(ControlError::new("TIMEOUT", "private diagnostic"))
            } else {
                self.inner.inspect(agent)
            }
        }
        fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation> {
            let result = self.inner.execute(agent, action)?;
            if *action == RuntimeAction::Start {
                Err(ControlError::new("TIMEOUT", "private diagnostic").retryable())
            } else {
                Ok(result)
            }
        }
    }

    #[test]
    fn failed_start_reconciles_partial_side_effects_before_allowing_destruction() {
        for inspect_fails in [false, true] {
            let f = Fixture::new();
            let runtime = Arc::new(PartialStartRuntime {
                inner: SimulatorRuntime::default(),
                inspect_fails,
            });
            let service = ControlService::open(&f.path, runtime).unwrap();
            let agent = f.create(&service);
            assert_eq!(
                service.action(&agent.id, "start").unwrap_err().code,
                "TIMEOUT"
            );
            let snapshot = service.snapshot().unwrap();
            assert_eq!(
                snapshot.agents[0].status,
                if inspect_fails { "unknown" } else { "running" }
            );
            assert_eq!(
                service.request(&agent.id, "delete").unwrap_err().code,
                "STOP_REQUIRED"
            );
            assert_eq!(
                service.action(&agent.id, "stop").unwrap().agents[0].status,
                "stopped"
            );
        }
    }

    /// VM lifecycle and process lifecycle are independent. This models the
    /// Hyper-V provider before any guest Agent process channel exists.
    struct VmLifecycleRuntime {
        inner: SimulatorRuntime,
        fail_start: bool,
        fail_inspect: bool,
    }
    impl Runtime for VmLifecycleRuntime {
        fn info(&self) -> RuntimeInfo {
            self.inner.info()
        }
        fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation> {
            if self.fail_inspect {
                return Err(ControlError::new("TIMEOUT", "private"));
            }
            let mut observation = self.inner.inspect(agent)?;
            observation.agent_status = "stopped".into();
            Ok(observation)
        }
        fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation> {
            let mut observation = self.inner.execute(agent, action)?;
            observation.agent_status = "stopped".into();
            if self.fail_start && *action == RuntimeAction::Start {
                Err(ControlError::new("TIMEOUT", "private").retryable())
            } else {
                Ok(observation)
            }
        }
    }

    #[test]
    fn vm_without_agent_process_can_stop_and_cannot_be_edited_while_running() {
        for (fail_start, fail_inspect) in [(false, false), (true, false), (true, true)] {
            let f = Fixture::new();
            let runtime = Arc::new(VmLifecycleRuntime {
                inner: SimulatorRuntime::default(),
                fail_start,
                fail_inspect,
            });
            let service = ControlService::open(&f.path, runtime).unwrap();
            let agent = f.create(&service);
            let result = service.action(&agent.id, "start");
            assert_eq!(result.is_err(), fail_start);
            let snapshot = service.snapshot().unwrap();
            let unknown = fail_start && fail_inspect;
            assert_eq!(
                snapshot.agents[0].status,
                if unknown { "unknown" } else { "stopped" }
            );
            assert_eq!(
                snapshot.agents[0].runtime_state,
                if unknown { "failed" } else { "running" }
            );
            assert_eq!(snapshot.agents[0].last_error.is_some(), fail_start);
            assert_eq!(
                service.action(&agent.id, "start").unwrap_err().code,
                "INVALID_TRANSITION"
            );
            for action in ["delete", "reset", "network"] {
                assert_eq!(
                    service.request(&agent.id, action).unwrap_err().code,
                    "STOP_REQUIRED"
                );
            }
            assert_eq!(
                service.revoke(&agent.id, "network").unwrap_err().code,
                "STOP_REQUIRED"
            );
            let stopped = service.action(&agent.id, "stop").unwrap();
            assert_eq!(stopped.agents[0].runtime_state, "stopped");
            assert_eq!(stopped.agents[0].status, "stopped");
            assert!(service.request(&agent.id, "delete").is_ok());
        }
    }

    #[test]
    fn preserves_safe_provider_error_codes_and_rejects_unrecognized_codes() {
        for code in [
            "IMAGE_INVALID",
            "OWNERSHIP_DENIED",
            "INVALID_RESPONSE",
            "INVALID_REQUEST",
        ] {
            let error =
                safe_runtime_error(&ControlError::new(code, "secret diagnostic"), "operation");
            assert_eq!(error.code, code);
            assert!(!error.message.contains("secret"));
        }
        assert_eq!(
            safe_runtime_error(&ControlError::new("secret code", "secret"), "operation").code,
            "RUNTIME_FAILED"
        );
    }
}

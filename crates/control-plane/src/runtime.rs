use crate::model::{Agent, ControlError, Result, RuntimeInfo, PERMISSIONS};
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAction {
    Create,
    Start,
    Stop,
    Reset,
    Delete,
    GrantPermission(String),
    RevokePermission(String),
}

impl RuntimeAction {
    pub fn name(&self) -> &str {
        match self {
            Self::Create => "create",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Reset => "reset",
            Self::Delete => "delete",
            Self::GrantPermission(_) => "grant",
            Self::RevokePermission(_) => "revoke",
        }
    }
    pub fn pending_state(&self) -> &str {
        match self {
            Self::Create => "creating",
            Self::Start => "starting",
            Self::Stop => "stopping",
            Self::Reset => "resetting",
            Self::Delete => "deleting",
            _ => "updating",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeObservation {
    pub runtime_state: String,
    pub agent_status: String,
}

impl RuntimeObservation {
    pub fn new(runtime_state: &str, agent_status: &str) -> Self {
        Self {
            runtime_state: runtime_state.into(),
            agent_status: agent_status.into(),
        }
    }
}

/// Implementations must confine side effects to `resource_id`, never interpolate
/// untrusted text into commands, and return safe errors (not process output).
/// `inspect` must check actual resources; persisted UI status is not evidence.
/// External process implementations must enforce a bounded timeout and cleanup
/// of their process tree. This synchronous interface runs on a blocking worker;
/// spawning an unbounded task and timing out its waiter is not cancellation.
pub trait Runtime: Send + Sync {
    fn info(&self) -> RuntimeInfo;
    fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation>;
    fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation>;
}

/// Explicitly an in-memory model; no processes, mounts, networking or isolation.
#[derive(Default)]
pub struct SimulatorRuntime {
    resources: Mutex<BTreeMap<String, RuntimeObservation>>,
}

impl Runtime for SimulatorRuntime {
    fn info(&self) -> RuntimeInfo {
        RuntimeInfo {
            kind: "simulator".into(),
            available: true,
            isolation_enforced: false,
            capabilities: PERMISSIONS
                .iter()
                .map(|key| ((*key).into(), true))
                .collect(),
            message: "模拟模式：仅演示生命周期与权限，不执行 Agent，不提供安全隔离。".into(),
        }
    }
    fn inspect(&self, agent: &Agent) -> Result<RuntimeObservation> {
        let resources = self
            .resources
            .lock()
            .map_err(|_| ControlError::new("RUNTIME_UNAVAILABLE", "模拟运行时不可用。"))?;
        Ok(resources
            .get(&agent.resource_id)
            .cloned()
            .unwrap_or_else(|| RuntimeObservation::new("missing", "stopped")))
    }
    fn execute(&self, agent: &Agent, action: &RuntimeAction) -> Result<RuntimeObservation> {
        let mut resources = self
            .resources
            .lock()
            .map_err(|_| ControlError::new("RUNTIME_UNAVAILABLE", "模拟运行时不可用。"))?;
        let result = match action {
            RuntimeAction::Create | RuntimeAction::Reset => {
                RuntimeObservation::new("ready", "ready")
            }
            RuntimeAction::Start => RuntimeObservation::new("running", "running"),
            RuntimeAction::Stop => RuntimeObservation::new("stopped", "stopped"),
            RuntimeAction::Delete => {
                resources.remove(&agent.resource_id);
                return Ok(RuntimeObservation::new("missing", "stopped"));
            }
            RuntimeAction::GrantPermission(_) | RuntimeAction::RevokePermission(_) => resources
                .get(&agent.resource_id)
                .cloned()
                .unwrap_or_else(|| RuntimeObservation::new("ready", "ready")),
        };
        resources.insert(agent.resource_id.clone(), result.clone());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn agent(resource_id: &str) -> Agent {
        Agent {
            id: resource_id.into(),
            name: "test".into(),
            provider: "Codex".into(),
            status: "running".into(),
            workspace: String::new(),
            workspace_grant_id: String::new(),
            security_mode: "Protected".into(),
            version: "not-installed".into(),
            last_activity: String::new(),
            accent: "blue".into(),
            permissions: BTreeMap::new(),
            runtime_kind: "simulator".into(),
            runtime_state: "running".into(),
            resource_id: resource_id.into(),
            operation_id: None,
            last_error: None,
        }
    }

    #[test]
    fn simulator_is_explicit_and_deletion_is_resource_scoped() {
        let runtime = SimulatorRuntime::default();
        let one = agent("one");
        let two = agent("two");
        assert!(!runtime.info().isolation_enforced);
        assert_eq!(
            runtime.inspect(&one).unwrap(),
            RuntimeObservation::new("missing", "stopped")
        );
        runtime.execute(&one, &RuntimeAction::Create).unwrap();
        runtime.execute(&two, &RuntimeAction::Create).unwrap();
        runtime.execute(&one, &RuntimeAction::Start).unwrap();
        runtime.execute(&one, &RuntimeAction::Delete).unwrap();
        assert_eq!(runtime.inspect(&one).unwrap().runtime_state, "missing");
        assert_eq!(runtime.inspect(&two).unwrap().runtime_state, "ready");
    }
}

use crate::workspace::WorkspaceGrant;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PERMISSIONS: [&str; 5] = [
    "workspaceWrite",
    "network",
    "mcpTools",
    "clipboard",
    "exportFiles",
];
pub type Result<T> = std::result::Result<T, ControlError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ControlError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<String>,
}

impl ControlError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            operation_id: None,
        }
    }
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
    pub fn for_operation(mut self, id: &str) -> Self {
        self.operation_id = Some(id.into());
        self
    }
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ControlError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub kind: String,
    pub available: bool,
    pub isolation_enforced: bool,
    pub capabilities: BTreeMap<String, bool>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub provider: String,
    /// Agent process status; separate from the virtual machine's state.
    pub status: String,
    pub workspace: String,
    pub workspace_grant_id: String,
    pub security_mode: String,
    pub version: String,
    pub last_activity: String,
    pub accent: String,
    pub permissions: BTreeMap<String, bool>,
    pub runtime_kind: String,
    pub runtime_state: String,
    pub resource_id: String,
    pub operation_id: Option<String>,
    pub last_error: Option<ControlError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAgentInput {
    pub name: String,
    pub provider: String,
    pub workspace_grant_id: String,
    pub security_mode: String,
    #[serde(default)]
    pub runtime_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub time: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub agent_id: String,
    pub action: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub agent_id: String,
    pub action: String,
    pub state: String,
    pub error: Option<ControlError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Console {
    pub agents: Vec<Agent>,
    pub events: Vec<AuditEvent>,
    pub approvals: Vec<Approval>,
    pub runtime: RuntimeInfo,
    pub operations: Vec<Operation>,
}

impl Console {
    pub fn new(runtime: RuntimeInfo) -> Self {
        Self {
            agents: vec![],
            events: vec![],
            approvals: vec![],
            runtime,
            operations: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub schema_version: u32,
    pub console: Console,
    pub grants: Vec<WorkspaceGrant>,
}

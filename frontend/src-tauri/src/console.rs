use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const PERMISSIONS: [&str; 5] = ["workspaceWrite", "network", "mcpTools", "clipboard", "exportFiles"];

#[derive(Debug, Serialize)]
pub struct ConsoleError {
    code: &'static str,
    message: String,
}

impl ConsoleError {
    pub fn new(code: &'static str, message: &str) -> Self {
        Self { code, message: message.into() }
    }
}

type Result<T> = std::result::Result<T, ConsoleError>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    id: String,
    name: String,
    provider: String,
    status: String,
    workspace: String,
    security_mode: String,
    version: String,
    last_activity: String,
    accent: String,
    permissions: BTreeMap<String, bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAgentInput {
    name: String,
    provider: String,
    workspace: String,
    security_mode: String,
}

#[derive(Clone, Serialize)]
pub struct AuditEvent {
    id: String,
    time: String,
    title: String,
    detail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    id: String,
    agent_id: String,
    action: String,
    title: String,
    detail: String,
}

#[derive(Clone, Default, Serialize)]
pub struct Console {
    agents: Vec<Agent>,
    events: Vec<AuditEvent>,
    approvals: Vec<Approval>,
}

impl Console {
    fn record(&mut self, title: &str, detail: &str) {
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().to_string();
        self.events.insert(0, AuditEvent { id: Uuid::new_v4().to_string(), time, title: title.into(), detail: detail.into() });
        self.events.truncate(500);
    }

    fn position(&self, id: &str) -> Result<usize> {
        self.agents.iter().position(|agent| agent.id == id)
            .ok_or_else(|| ConsoleError::new("NOT_FOUND", "Agent 不存在，请刷新列表。"))
    }

    pub fn create(&mut self, input: CreateAgentInput) -> Result<Self> {
        let name = input.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(ConsoleError::new("INVALID_INPUT", "名称必须为 1–80 个字符。"));
        }
        if !["Claude Code", "Codex", "DeepSeek Harness"].contains(&input.provider.as_str())
            || !["Protected", "Developer"].contains(&input.security_mode.as_str()) {
            return Err(ConsoleError::new("INVALID_INPUT", "不支持的 Agent 类型或安全模式。"));
        }
        let path = std::path::Path::new(input.workspace.trim());
        if !path.is_absolute() || !path.is_dir() {
            return Err(ConsoleError::new("INVALID_WORKSPACE", "请选择本机存在的绝对目录。"));
        }
        let workspace = path.canonicalize()
            .map_err(|_| ConsoleError::new("INVALID_WORKSPACE", "无法访问所选目录。"))?
            .to_string_lossy().into_owned();
        let (version, accent) = match input.provider.as_str() {
            "Claude Code" => ("1.0.84", "violet"),
            "Codex" => ("0.42.0", "blue"),
            _ => ("0.8.2", "orange"),
        };
        let agent = Agent {
            id: Uuid::new_v4().to_string(), name: name.into(), provider: input.provider,
            status: "ready".into(), workspace, security_mode: input.security_mode,
            version: version.into(), last_activity: "刚刚".into(), accent: accent.into(),
            permissions: PERMISSIONS.iter().map(|key| ((*key).into(), false)).collect(),
        };
        self.record("创建模拟 Agent", &agent.name);
        self.agents.insert(0, agent);
        Ok(self.clone())
    }

    pub fn action(&mut self, id: &str, action: &str) -> Result<Self> {
        let index = self.position(id)?;
        let status = self.agents[index].status.as_str();
        let next = match (action, status) {
            ("start", "ready" | "stopped") => "running",
            ("stop", "running") => "stopped",
            _ => return Err(ConsoleError::new("INVALID_TRANSITION", "此操作不可用；重置和删除需要单独确认。")),
        };
        if self.approvals.iter().any(|approval| approval.agent_id == id) {
            return Err(ConsoleError::new("APPROVAL_PENDING", "请先处理此 Agent 的待确认请求。"));
        }
        self.agents[index].status = next.into();
        self.agents[index].last_activity = "刚刚".into();
        let name = self.agents[index].name.clone();
        self.record(if action == "start" { "启动模拟会话" } else { "停止模拟会话" }, &name);
        Ok(self.clone())
    }

    pub fn request(&mut self, id: &str, action: &str) -> Result<Self> {
        let index = self.position(id)?;
        if self.agents[index].status == "running" {
            return Err(ConsoleError::new("STOP_REQUIRED", "请先停止模拟会话再修改权限、重置或删除。"));
        }
        if action != "reset" && action != "delete" && !PERMISSIONS.contains(&action) {
            return Err(ConsoleError::new("INVALID_INPUT", "不支持的权限操作。"));
        }
        if self.approvals.iter().any(|approval| approval.agent_id == id) {
            return Err(ConsoleError::new("APPROVAL_PENDING", "该 Agent 已有待确认请求。"));
        }
        if PERMISSIONS.contains(&action) && self.agents[index].permissions[action] {
            return Err(ConsoleError::new("ALREADY_GRANTED", "该权限已经允许。"));
        }
        let name = self.agents[index].name.clone();
        let title = match action {
            "reset" => "重置模拟环境", "delete" => "删除模拟 Agent", "workspaceWrite" => "允许修改工作区",
            "network" => "允许网络访问", "mcpTools" => "允许 MCP 工具", "clipboard" => "允许读取剪贴板", _ => "允许导出文件",
        };
        let detail = match action {
            "reset" => "清除当前模拟会话并撤销全部权限；不修改实际工作区文件。",
            "delete" => "移除此模拟 Agent 及其配置；保留审计记录和实际工作区文件。",
            "network" => "未来真实运行时可能向外部发送工作区数据；当前仅修改模拟策略。",
            _ => "仅对此 Agent 修改模拟策略；不会授予实际宿主机访问能力。",
        };
        self.approvals.push(Approval {
            id: Uuid::new_v4().to_string(), agent_id: id.into(), action: action.into(), title: title.into(),
            detail: format!("{name}：{detail}"),
        });
        self.record("请求用户确认", &format!("{name} · {title}"));
        Ok(self.clone())
    }

    pub fn decide(&mut self, approval_id: &str, allowed: bool) -> Result<Self> {
        let position = self.approvals.iter().position(|approval| approval.id == approval_id)
            .ok_or_else(|| ConsoleError::new("APPROVAL_EXPIRED", "请求已经处理或不存在，请刷新。"))?;
        let approval = self.approvals[position].clone();
        let index = self.position(&approval.agent_id)?;
        if self.agents[index].status == "running" {
            return Err(ConsoleError::new("STOP_REQUIRED", "运行中的 Agent 不能修改配置。"));
        }
        if allowed {
            match approval.action.as_str() {
                "delete" => { self.agents.remove(index); }
                "reset" => {
                    self.agents[index].status = "ready".into();
                    for value in self.agents[index].permissions.values_mut() { *value = false; }
                }
                key => { self.agents[index].permissions.insert(key.into(), true); }
            }
        }
        self.approvals.remove(position);
        self.record(if allowed { "用户确认" } else { "用户拒绝" }, &approval.detail);
        Ok(self.clone())
    }

    pub fn revoke(&mut self, id: &str, key: &str) -> Result<Self> {
        let index = self.position(id)?;
        if self.agents[index].status == "running" || self.approvals.iter().any(|approval| approval.agent_id == id) {
            return Err(ConsoleError::new("STOP_REQUIRED", "请停止会话并处理待确认请求后撤销权限。"));
        }
        let permission = self.agents[index].permissions.get_mut(key)
            .ok_or_else(|| ConsoleError::new("INVALID_INPUT", "未知权限。"))?;
        *permission = false;
        let name = self.agents[index].name.clone();
        self.record("撤销模拟权限", &format!("{name} · {key}"));
        Ok(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created() -> Console {
        let mut state = Console::default();
        state.create(CreateAgentInput { name: "test".into(), provider: "Codex".into(),
            workspace: std::env::temp_dir().to_string_lossy().into(), security_mode: "Protected".into() }).unwrap();
        state
    }

    #[test]
    fn approval_is_required_scoped_and_single_use() {
        let mut state = created();
        let id = state.agents[0].id.clone();
        assert!(state.agents[0].permissions.values().all(|value| !value));
        assert!(state.action(&id, "delete").is_err());
        state.request(&id, "network").unwrap();
        let approval = state.approvals[0].id.clone();
        assert!(!state.agents[0].permissions["network"]);
        assert!(state.action(&id, "start").is_err());
        state.decide(&approval, true).unwrap();
        assert!(state.agents[0].permissions["network"]);
        assert!(state.decide(&approval, true).is_err());
    }

    #[test]
    fn lifecycle_rejects_invalid_transitions_and_reset_revokes() {
        let mut state = created();
        let id = state.agents[0].id.clone();
        assert!(state.action(&id, "stop").is_err());
        state.action(&id, "start").unwrap();
        assert!(state.request(&id, "delete").is_err());
        state.action(&id, "stop").unwrap();
        state.request(&id, "workspaceWrite").unwrap();
        state.decide(&state.approvals[0].id.clone(), true).unwrap();
        state.request(&id, "reset").unwrap();
        state.decide(&state.approvals[0].id.clone(), true).unwrap();
        assert!(state.agents[0].permissions.values().all(|value| !value));
        assert_eq!(state.agents[0].status, "ready");
    }

    #[test]
    fn denial_and_deletion_preserve_audit() {
        let mut state = created();
        let id = state.agents[0].id.clone();
        state.request(&id, "delete").unwrap();
        state.decide(&state.approvals[0].id.clone(), false).unwrap();
        assert_eq!(state.agents.len(), 1);
        state.request(&id, "delete").unwrap();
        state.decide(&state.approvals[0].id.clone(), true).unwrap();
        assert!(state.agents.is_empty());
        assert!(state.approvals.is_empty());
        assert!(!state.events.is_empty());
    }

    #[test]
    fn rejects_untrusted_inputs() {
        let mut state = Console::default();
        let result = state.create(CreateAgentInput { name: " ".into(), provider: "Codex".into(),
            workspace: "relative".into(), security_mode: "Protected".into() });
        assert!(result.is_err());
        assert!(state.agents.is_empty());
        let mut state = created();
        assert!(state.request(&state.agents[0].id.clone(), "hostShell").is_err());
        assert!(state.action("nonexistent", "start").is_err());
    }
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use tauri::State;
use uuid::Uuid;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Agent {
    id: String,
    name: String,
    provider: String,
    version: String,
    status: String,
    workspace: String,
    security_mode: String,
    last_activity: String,
    accent: String,
}

struct AppState {
    agents: Mutex<Vec<Agent>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            agents: Mutex::new(vec![
                Agent {
                    id: "agent-frontend".into(),
                    name: "Frontend Architect".into(),
                    provider: "Claude Code".into(),
                    version: "1.0.84".into(),
                    status: "running".into(),
                    workspace: "D:/workspaces/step-one-ui".into(),
                    security_mode: "Protected".into(),
                    last_activity: "现在".into(),
                    accent: "violet".into(),
                },
                Agent {
                    id: "agent-research".into(),
                    name: "Research Runner".into(),
                    provider: "Codex".into(),
                    version: "0.42.0".into(),
                    status: "ready".into(),
                    workspace: "D:/workspaces/research".into(),
                    security_mode: "Protected".into(),
                    last_activity: "12 分钟前".into(),
                    accent: "blue".into(),
                },
                Agent {
                    id: "agent-local".into(),
                    name: "Local Builder".into(),
                    provider: "DeepSeek Harness".into(),
                    version: "0.8.2".into(),
                    status: "stopped".into(),
                    workspace: "D:/workspaces/local-builder".into(),
                    security_mode: "Developer".into(),
                    last_activity: "昨天".into(),
                    accent: "orange".into(),
                },
            ]),
        }
    }
}

#[tauri::command]
fn list_agents(state: State<'_, AppState>) -> Result<Vec<Agent>, String> {
    state
        .agents
        .lock()
        .map(|agents| agents.clone())
        .map_err(|_| "agent state is unavailable".to_string())
}

#[tauri::command]
fn create_agent(provider: String, state: State<'_, AppState>) -> Result<Agent, String> {
    let (version, accent) = match provider.as_str() {
        "Claude Code" => ("1.0.84", "violet"),
        "Codex" => ("0.42.0", "blue"),
        "DeepSeek Harness" => ("0.8.2", "orange"),
        _ => return Err("unsupported agent provider".to_string()),
    };

    let agent = Agent {
        id: format!("agent-{}", Uuid::new_v4()),
        name: format!("New {provider} Workspace"),
        provider,
        version: version.into(),
        status: "ready".into(),
        workspace: "选择工作区后显示".into(),
        security_mode: "Protected".into(),
        last_activity: "刚刚".into(),
        accent: accent.into(),
    };

    state
        .agents
        .lock()
        .map_err(|_| "agent state is unavailable".to_string())?
        .insert(0, agent.clone());

    Ok(agent)
}

#[tauri::command]
fn agent_action(agent_id: String, action: String, state: State<'_, AppState>) -> Result<Agent, String> {
    let status = match action.as_str() {
        "start" => "running",
        "stop" => "stopped",
        "reset" => "ready",
        _ => return Err("unsupported agent action".to_string()),
    };

    let mut agents = state
        .agents
        .lock()
        .map_err(|_| "agent state is unavailable".to_string())?;
    let agent = agents
        .iter_mut()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| "agent not found".to_string())?;

    agent.status = status.into();
    agent.last_activity = "刚刚".into();
    Ok(agent.clone())
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![list_agents, create_agent, agent_action])
        .run(tauri::generate_context!())
        .expect("error while running Step-One");
}

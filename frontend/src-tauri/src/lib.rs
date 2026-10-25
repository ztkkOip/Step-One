mod console;
mod runtime;

use console::{Console, ConsoleError, CreateAgentInput};
use runtime::RuntimeCapabilities;
use std::sync::{Mutex, MutexGuard};
use tauri::State;

#[derive(Default)]
struct AppState(Mutex<Console>);

fn lock(state: &AppState) -> Result<MutexGuard<'_, Console>, ConsoleError> {
    state.0.lock().map_err(|_| ConsoleError::new("STATE_UNAVAILABLE", "控制台状态不可用，请重启应用。"))
}

#[tauri::command]
fn console_snapshot(state: State<'_, AppState>) -> Result<Console, ConsoleError> {
    Ok(lock(&state)?.clone())
}

#[tauri::command]
fn create_agent(input: CreateAgentInput, state: State<'_, AppState>) -> Result<Console, ConsoleError> {
    lock(&state)?.create(input)
}

#[tauri::command]
fn agent_action(agent_id: String, action: String, state: State<'_, AppState>) -> Result<Console, ConsoleError> {
    lock(&state)?.action(&agent_id, &action)
}

#[tauri::command]
fn request_approval(agent_id: String, action: String, state: State<'_, AppState>) -> Result<Console, ConsoleError> {
    lock(&state)?.request(&agent_id, &action)
}

#[tauri::command]
fn decide_approval(approval_id: String, allowed: bool, state: State<'_, AppState>) -> Result<Console, ConsoleError> {
    lock(&state)?.decide(&approval_id, allowed)
}

#[tauri::command]
fn revoke_permission(agent_id: String, key: String, state: State<'_, AppState>) -> Result<Console, ConsoleError> {
    lock(&state)?.revoke(&agent_id, &key)
}

#[tauri::command]
async fn runtime_capabilities() -> Result<RuntimeCapabilities, ConsoleError> {
    tauri::async_runtime::spawn_blocking(runtime::capabilities)
        .await
        .map_err(|_| ConsoleError::new("DIAGNOSTICS_FAILED", "运行环境检查失败，请重试。"))
}

#[tauri::command]
async fn choose_workspace() -> Result<Option<String>, ConsoleError> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new().set_title("选择模拟工作区（不会挂载或读取文件）")
            .pick_folder().map(|path| path.to_string_lossy().into_owned())
    }).await.map_err(|_| ConsoleError::new("DIALOG_FAILED", "无法打开目录选择器。"))
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![console_snapshot, create_agent, agent_action,
            request_approval, decide_approval, revoke_permission, runtime_capabilities, choose_workspace])
        .run(tauri::generate_context!())
        .expect("error while running Step-One");
}

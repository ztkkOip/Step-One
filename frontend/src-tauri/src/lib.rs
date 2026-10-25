use std::sync::Arc;
use step_one_control::diagnostics::{self, RuntimeCapabilities};
use step_one_control::workspace::WorkspaceSelection;
use step_one_control::{Console, ControlError, ControlService, CreateAgentInput, SimulatorRuntime};
use tauri::{Manager, State};

struct AppState(Arc<ControlService>);

async fn blocking<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce(&ControlService) -> Result<T, ControlError> + Send + 'static,
) -> Result<T, ControlError> {
    let service = Arc::clone(&state.0);
    tauri::async_runtime::spawn_blocking(move || operation(&service))
        .await
        .map_err(|_| ControlError::new("STATE_UNAVAILABLE", "操作中断，请刷新状态。"))?
}

#[tauri::command]
async fn console_snapshot(state: State<'_, AppState>) -> Result<Console, ControlError> {
    blocking(&state, |service| service.snapshot()).await
}

#[tauri::command]
async fn create_agent(
    input: CreateAgentInput,
    state: State<'_, AppState>,
) -> Result<Console, ControlError> {
    blocking(&state, move |service| service.create(input)).await
}

#[tauri::command]
async fn agent_action(
    agent_id: String,
    action: String,
    state: State<'_, AppState>,
) -> Result<Console, ControlError> {
    blocking(&state, move |service| service.action(&agent_id, &action)).await
}

#[tauri::command]
async fn request_approval(
    agent_id: String,
    action: String,
    state: State<'_, AppState>,
) -> Result<Console, ControlError> {
    blocking(&state, move |service| service.request(&agent_id, &action)).await
}

#[tauri::command]
async fn decide_approval(
    approval_id: String,
    allowed: bool,
    state: State<'_, AppState>,
) -> Result<Console, ControlError> {
    blocking(&state, move |service| service.decide(&approval_id, allowed)).await
}

#[tauri::command]
async fn revoke_permission(
    agent_id: String,
    key: String,
    state: State<'_, AppState>,
) -> Result<Console, ControlError> {
    blocking(&state, move |service| service.revoke(&agent_id, &key)).await
}

#[tauri::command]
async fn choose_workspace(
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSelection>, ControlError> {
    blocking(&state, |service| {
        rfd::FileDialog::new()
            .set_title("选择允许 Agent 使用的工作区")
            .pick_folder()
            .map(|path| {
                service
                    .authorize_workspace(&path)
                    .map(|grant| grant.public())
            })
            .transpose()
    })
    .await
}

#[tauri::command]
async fn runtime_capabilities() -> Result<RuntimeCapabilities, ControlError> {
    tauri::async_runtime::spawn_blocking(diagnostics::capabilities)
        .await
        .map_err(|_| ControlError::new("DIAGNOSTICS_FAILED", "运行环境检查失败，请重试。"))
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let service = ControlService::open(
                &data.join("control.sqlite3"),
                Arc::new(SimulatorRuntime::default()),
            )
            .map_err(|_| {
                std::io::Error::other("Step-One control database could not be opened safely")
            })?;
            app.manage(AppState(Arc::new(service)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            console_snapshot,
            create_agent,
            agent_action,
            request_approval,
            decide_approval,
            revoke_permission,
            choose_workspace,
            runtime_capabilities
        ])
        .run(tauri::generate_context!())
        .expect("error while running Step-One");
}

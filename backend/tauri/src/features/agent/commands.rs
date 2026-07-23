use tauri::{AppHandle, Manager, State, WebviewWindow};

use super::{
    AgentActionRequest, AgentActionResult, AgentCommandError, AgentFeatureState,
    AgentNetworkSnapshot, AgentProposal, collect_network_snapshot,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_get_network_snapshot(app: AppHandle) -> AgentNetworkSnapshot {
    collect_network_snapshot(&app).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_propose_network_action(
    window: WebviewWindow,
    state: State<'_, AgentFeatureState>,
    action: AgentActionRequest,
) -> Result<AgentProposal, AgentCommandError> {
    state
        .propose(window.app_handle(), window.label(), action)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_execute_network_action(
    window: WebviewWindow,
    state: State<'_, AgentFeatureState>,
    proposal_id: String,
    digest: String,
) -> Result<AgentActionResult, AgentCommandError> {
    state
        .execute(window.app_handle(), &window, &proposal_id, &digest)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_cancel_network_action(
    window: WebviewWindow,
    state: State<'_, AgentFeatureState>,
    proposal_id: String,
) -> Result<bool, AgentCommandError> {
    Ok(state.cancel(window.label(), &proposal_id).await)
}

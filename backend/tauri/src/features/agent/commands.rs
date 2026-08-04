use tauri::{State, WebviewWindow};

use crate::client::NyanpasuClient;

use super::{
    AgentActionRequest, AgentActionResult, AgentBridgeStartResult, AgentBridgeStatus,
    AgentCommandError, AgentHistorySnapshot, AgentIntentRequest, AgentIntentResolution,
    AgentManifest, AgentNetworkSnapshot, AgentProposal,
};

#[tauri::command]
#[specta::specta]
pub(crate) fn agent_get_manifest(client: State<'_, NyanpasuClient>) -> AgentManifest {
    client.agent_manifest()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_get_network_snapshot(
    client: State<'_, NyanpasuClient>,
) -> Result<AgentNetworkSnapshot, AgentCommandError> {
    client.agent_network_snapshot().await
}

#[tauri::command]
#[specta::specta]
pub(crate) fn agent_resolve_intent(
    client: State<'_, NyanpasuClient>,
    request: AgentIntentRequest,
) -> AgentIntentResolution {
    client.agent_resolve_intent(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_get_history(
    client: State<'_, NyanpasuClient>,
) -> Result<AgentHistorySnapshot, AgentCommandError> {
    client.agent_history().await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_clear_history(
    window: WebviewWindow,
    client: State<'_, NyanpasuClient>,
) -> Result<AgentHistorySnapshot, AgentCommandError> {
    client.agent_clear_history(window.label()).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_propose_network_action(
    window: WebviewWindow,
    client: State<'_, NyanpasuClient>,
    action: AgentActionRequest,
) -> Result<AgentProposal, AgentCommandError> {
    client.agent_propose_action(window.label(), action).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_execute_network_action(
    window: WebviewWindow,
    client: State<'_, NyanpasuClient>,
    proposal_id: String,
    digest: String,
) -> Result<AgentActionResult, AgentCommandError> {
    client
        .agent_execute_action(window.label(), &proposal_id, &digest)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_cancel_network_action(
    window: WebviewWindow,
    client: State<'_, NyanpasuClient>,
    proposal_id: String,
) -> Result<bool, AgentCommandError> {
    client
        .agent_cancel_action(window.label(), &proposal_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn start_agent_bridge(
    client: State<'_, NyanpasuClient>,
) -> Result<AgentBridgeStartResult, AgentCommandError> {
    client.agent_start_bridge().await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_agent_bridge_status(
    client: State<'_, NyanpasuClient>,
) -> Result<AgentBridgeStatus, AgentCommandError> {
    client.agent_bridge_status().await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn stop_agent_bridge(
    client: State<'_, NyanpasuClient>,
) -> Result<AgentBridgeStatus, AgentCommandError> {
    client.agent_stop_bridge().await
}

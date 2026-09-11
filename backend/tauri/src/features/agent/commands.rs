use tauri::{AppHandle, Manager, State, WebviewWindow};

use super::{
    AgentActionRequest, AgentActionResult, AgentCommandError, AgentFeatureState,
    AgentIntentRequest, AgentIntentResolution, AgentManifest, AgentNetworkProbeRequest,
    AgentNetworkProbeResult, AgentNetworkSnapshot, AgentProposal, AgentToolError, AgentToolName,
    AgentToolResult, agent_manifest, collect_network_snapshot, execute_network_probe,
    execute_readonly_tool, resolve_intent,
};

#[tauri::command]
#[specta::specta]
pub(crate) fn agent_get_manifest() -> AgentManifest {
    agent_manifest()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_execute_readonly_tool(
    app: AppHandle,
    tool: AgentToolName,
) -> Result<AgentToolResult, AgentToolError> {
    execute_readonly_tool(&app, tool).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_probe_network(
    request: AgentNetworkProbeRequest,
) -> Result<AgentNetworkProbeResult, AgentToolError> {
    execute_network_probe(request).await
}

#[tauri::command]
#[specta::specta]
pub(crate) fn agent_resolve_intent(request: AgentIntentRequest) -> AgentIntentResolution {
    resolve_intent(request)
}

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

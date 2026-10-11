use tauri::{AppHandle, Manager, State, WebviewWindow};

use super::history::{self, AgentAuditHistoryView, AgentDiagnosticHistoryView};
use super::{
    AgentActionRequest, AgentActionResult, AgentCommandError, AgentFeatureState,
    AgentIntentRequest, AgentIntentResolution, AgentManifest, AgentNetworkProbeRequest,
    AgentNetworkProbeResult, AgentNetworkSnapshot, AgentProposal, AgentToolError, AgentToolName,
    AgentToolRequest, AgentToolResult, agent_manifest, collect_network_snapshot,
    execute_network_probe, execute_readonly_tool, execute_tool, resolve_intent,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_get_audit_history() -> Result<AgentAuditHistoryView, String> {
    history::get_audit_history()
        .await
        .map_err(|_| "agent history unavailable".to_owned())
}

/// Explicit persistence command. The backend captures an authoritative fresh
/// snapshot rather than accepting an untrusted frontend-provided diagnostic.
#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_capture_diagnostic_history(
    app: AppHandle,
) -> Result<AgentDiagnosticHistoryView, String> {
    let snapshot = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        collect_network_snapshot(&app),
    )
    .await
    .map_err(|_| "agent diagnosis timed out".to_owned())?;
    history::record_diagnostic_snapshot(&snapshot)
        .await
        .map_err(|_| "agent diagnostic history unavailable".to_owned())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_get_diagnostic_history() -> Result<AgentDiagnosticHistoryView, String> {
    history::get_diagnostic_history()
        .await
        .map_err(|_| "agent diagnostic history unavailable".to_owned())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn agent_get_manifest() -> AgentManifest {
    agent_manifest()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn agent_execute_tool(
    app: AppHandle,
    request: AgentToolRequest,
) -> Result<AgentToolResult, AgentToolError> {
    execute_tool(&app, request).await
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

use std::sync::Arc;

use tauri::Manager;

use crate::{
    client::NyanpasuClient,
    features::agent::{
        AgentClient, FsAgentHistoryPersistence, HttpAgentBridge, HttpBridgeHealth,
        HttpNetworkProbe, LegacyAgentConfiguration, LegacyAgentMutation, LegacyAgentRuntime,
        LegacyCoreLifecycle, LegacyCoreRoutingProbe, LegacyServiceControl, LegacySystemProxy,
        RegistryAgentToolExecutor, TauriAgentConfirmation, TauriAgentTelemetry,
    },
};

pub(crate) fn setup(app: &tauri::App) -> anyhow::Result<()> {
    let app_handle = app.handle().clone();
    let configuration = Arc::new(LegacyAgentConfiguration::new());
    let core = Arc::new(LegacyCoreLifecycle::new());
    let mutation = Arc::new(LegacyAgentMutation::new());
    let routing_probe = Arc::new(LegacyCoreRoutingProbe::new());
    let service = Arc::new(LegacyServiceControl::new());
    let system_proxy = Arc::new(LegacySystemProxy::new(mutation.clone()));
    let telemetry = Arc::new(TauriAgentTelemetry::new(app_handle.clone()));
    let runtime = Arc::new(LegacyAgentRuntime::new(
        configuration,
        core,
        mutation,
        routing_probe,
        service,
        system_proxy,
        telemetry,
    ));
    let bridge_health = Arc::new(HttpBridgeHealth::new());
    let network_probe = Arc::new(HttpNetworkProbe::new());
    let tool_executor = Arc::new(RegistryAgentToolExecutor::new(
        runtime.clone(),
        network_probe,
    ));
    let bridge = Box::new(HttpAgentBridge::new(tool_executor, bridge_health));
    let agent = AgentClient::new(
        runtime,
        Arc::new(TauriAgentConfirmation::new(app_handle.clone())),
        bridge,
        Arc::new(FsAgentHistoryPersistence::from_app_data_dir()?),
    )?;
    app.manage(NyanpasuClient::new(agent));
    Ok(())
}

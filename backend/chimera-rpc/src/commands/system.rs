//! System commands that already have shared Core implementations.
use crate::registry::RpcState;
use chimera_core::client::ChimeraClient;
use chimera_runtime_values::{core_status::CoreStatusInfo, observation::CoreStatusSummary};

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_config_features(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_config::clash::features::ConfigFeatures> {
    Ok(client.config_features())
}

#[chimera_rpc::rpc(query, expose = [http, local])]
pub async fn get_core_status_summary(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<CoreStatusSummary> {
    let status = client
        .routed_core_status()
        .ok_or_else(|| anyhow::anyhow!("CoreActor is not connected"))?;
    Ok(CoreStatusSummary::from(&CoreStatusInfo::from(status)))
}

#[chimera_rpc::rpc(optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn retry_configuration_runtime(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<()> {
    client.retry_configuration_runtime().await
}

#[chimera_rpc::rpc(expose = [http, local], query)]
pub async fn clash_api_get_version(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_core::clash::api::ClashVersion> {
    Ok(client.clash_version().await?)
}

#[chimera_rpc::rpc(operation = chimera_rpc_protocol::operations::GetCoreStatus, query, sensitive, expose = [http, local])]
pub async fn get_core_status(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_runtime_values::core_status::CoreStatusInfo> {
    Ok(client
        .routed_core_status()
        .ok_or_else(|| anyhow::anyhow!("CoreActor is not connected"))?
        .into())
}

#[chimera_rpc::rpc(query, expose = [http])]
pub fn get_service_status_summary(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_runtime_values::observation::ServiceStatusSummary> {
    let status = client
        .service_actor_status()
        .ok_or_else(|| anyhow::anyhow!("ServiceActor is not connected"))?;
    Ok((&status).into())
}

#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub async fn status_service(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_runtime_values::core_status::ServiceHostStatus> {
    client
        .service_actor_status()
        .ok_or_else(|| anyhow::anyhow!("ServiceActor is not connected"))
}

#[chimera_rpc::rpc(query, expose = [http])]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use chimera_core::client::hotkey::ports::{HotkeyAction, action_available};
        HotkeyAction::all()
            .iter()
            .filter(|action| action_available(**action))
            .map(|action| action.as_str())
            .collect()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    Vec::new()
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_hotkeys(client: RpcState<'_, ChimeraClient>) -> anyhow::Result<Vec<String>> {
    Ok(client.get_app_config()?.hotkeys)
}

use chimera_config::{application::SystemSettingsPatch, clash::config::ClashSystemPatch};
use chimera_core::client::configuration_mutations::ConfigurationMutations;
use chimera_rpc_protocol::types::RpcUnitMutationOutcome;

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http, local])]
pub async fn patch_clash_system_config(
    host: RpcState<'_, dyn ConfigurationMutations>,
    patch: ClashSystemPatch,
) -> anyhow::Result<RpcUnitMutationOutcome> {
    Ok(host.patch_clash(patch.into()).await?.into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(os_integration, presentation, elevation), expose = [http, local])]
pub async fn patch_system_settings(
    host: RpcState<'_, dyn ConfigurationMutations>,
    patch: SystemSettingsPatch,
) -> anyhow::Result<RpcUnitMutationOutcome> {
    // Chimera has not installed the dedicated network-policy authorization flow.
    anyhow::ensure!(
        patch.network_automation.is_none(),
        "network automation policy edits require the dedicated authorized settings command"
    );
    Ok(host.patch_application(patch.into()).await?.into())
}

use chimera_config::clash::config::overrides::ClashAccessPatch;

/// Patch LAN exposure and the controller secret, without logging either value.
#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn patch_runtime_access(
    host: RpcState<'_, dyn ConfigurationMutations>,
    patch: ClashAccessPatch,
) -> anyhow::Result<RpcUnitMutationOutcome> {
    Ok(host.patch_overrides(patch.into()).await?.into())
}

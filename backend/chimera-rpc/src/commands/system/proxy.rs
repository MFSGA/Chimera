//! The system proxy, TUN and transparent proxy switches.
use crate::registry::RpcState;
use chimera_config::{
    application::SystemSettingsPatch,
    clash::config::{ClashSystemPatch, overrides::ClashAccessPatch},
};
use chimera_core::client::configuration_mutations::ConfigurationMutations;
use chimera_rpc_protocol::types::RpcUnitMutationOutcome;

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http, local])]
pub async fn patch_clash_system_config(
    host: RpcState<'_, dyn ConfigurationMutations>,
    patch: ClashSystemPatch,
) -> anyhow::Result<RpcUnitMutationOutcome> {
    Ok(host.patch_clash(patch.into()).await?.into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn patch_runtime_access(
    host: RpcState<'_, dyn ConfigurationMutations>,
    patch: ClashAccessPatch,
) -> anyhow::Result<RpcUnitMutationOutcome> {
    Ok(host.patch_overrides(patch.into()).await?.into())
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

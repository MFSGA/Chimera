//! Shared declarations from the pinned ref settings.clash.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn clash_api_get_settings(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::clash::api::ClashApiSettings> {
    Ok(client.clash_api_settings().await?)
}

#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub async fn clash_api_get_configs(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::clash::api::ClashApiConfig> {
    Ok(client.clash_configs().await?)
}

#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub async fn get_clash_config(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_config::clash::config::ClashConfig> {
    Ok(client.get_clash_config()?)
}

#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub fn get_clash_info(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::client::clash_info::ClashInfo> {
    Ok(client.clash_info())
}

#[chimera_rpc::rpc(query, expose = [http, local])]
pub async fn get_clash_settings(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_config::clash::config::ClashSettings> {
    Ok((&client.get_clash_config()?).into())
}

use chimera_config::clash::config::ClashSettingsPatch;
use chimera_rpc_protocol::types::RpcUnitMutationOutcome;

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn patch_clash_config(
    client: RpcState<'_, ChimeraClient>,
    patch: ClashSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_clash_config(patch.into()).await?.into())
}

use chimera_config::clash::config::overrides::ClashOverridesSettingsPatch;

/// Patch the ordinary overrides, keeping managed/unmanaged fields intact.
#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn patch_runtime_overrides(
    client: RpcState<'_, ChimeraClient>,
    patch: ClashOverridesSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_runtime_overrides(patch.into()).await?.into())
}

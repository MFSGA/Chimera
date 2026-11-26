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

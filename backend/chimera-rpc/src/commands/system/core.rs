//! The core: its status, version, lifecycle, updates and directory.
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

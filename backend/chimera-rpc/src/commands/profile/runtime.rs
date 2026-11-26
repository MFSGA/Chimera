//! The runtime configuration the profiles produce.

use chimera_core::client::ChimeraClient;
use chimera_profile_transform::PostProcessingOutput;

use crate::registry::RpcState;
use anyhow::Result;

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn get_postprocessing_output(
    client: RpcState<'_, ChimeraClient>,
) -> Result<PostProcessingOutput> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.postprocessing_output.clone())
        .unwrap_or_default())
}

/// get the runtime config
#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn get_runtime_config(
    client: RpcState<'_, ChimeraClient>,
) -> Result<Option<serde_json::Value>> {
    Ok(client.runtime_config().await?)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_runtime_exists(client: RpcState<'_, ChimeraClient>) -> Result<Vec<String>> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.exists_keys.clone())
        .unwrap_or_default())
}

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn get_runtime_yaml(client: RpcState<'_, ChimeraClient>) -> Result<String> {
    Ok(client.runtime_yaml().await?)
}

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn inspect_applied_runtime(
    client: RpcState<'_, ChimeraClient>,
) -> Result<Option<chimera_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn inspect_runtime(
    client: RpcState<'_, ChimeraClient>,
) -> Result<Option<chimera_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_runtime().await)
}

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn inspect_runtime_node(
    client: RpcState<'_, ChimeraClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<chimera_core::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_runtime_node(&snapshot_id, node_id).await?)
}

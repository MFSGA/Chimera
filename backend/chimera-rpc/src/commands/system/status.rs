//! The configuration status and retrying effects that failed.
use crate::registry::RpcState;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn retry_configuration_runtime(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<()> {
    client.retry_configuration_runtime().await
}

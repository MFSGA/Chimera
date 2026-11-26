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

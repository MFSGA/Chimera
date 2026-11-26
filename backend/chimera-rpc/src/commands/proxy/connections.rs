//! Shared declarations from the pinned ref proxy.connections.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http, local])]
pub async fn clash_api_delete_connections(
    client: RpcState<'_, ChimeraClient>,
    id: Option<String>,
) -> Result<()> {
    Ok(client.close_clash_connections(id).await?)
}

//! Shared declarations from the pinned ref settings.network.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_contracts::net::DirectEgress;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http])]
pub async fn probe_direct_egress(client: RpcState<'_, ChimeraClient>) -> Result<DirectEgress> {
    Ok(client.probe_direct_egress().await?)
}

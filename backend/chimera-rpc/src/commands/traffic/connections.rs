//! Shared declarations from the pinned ref traffic.connections.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http, local], query)]
pub async fn query_traffic_active_connection_ids(
    client: RpcState<'_, ChimeraClient>,
    filters: Vec<chimera_traffic::TrafficFilter>,
) -> Result<Vec<String>> {
    Ok(client.query_traffic_active_connection_ids(filters).await?)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn query_traffic_closed_connections(
    client: RpcState<'_, ChimeraClient>,
    range: chimera_traffic::TrafficRange,
    filters: Vec<chimera_traffic::TrafficFilter>,
    before: Option<chimera_traffic::ClosedCursor>,
    limit: usize,
) -> Result<chimera_traffic::ClosedPage> {
    Ok(client
        .query_traffic_closed_connections(range, filters, before, limit)
        .await?)
}

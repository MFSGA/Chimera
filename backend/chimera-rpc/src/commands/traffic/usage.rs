//! Shared declarations from the pinned ref traffic.usage.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http, local], query)]
pub async fn get_traffic_summary(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_traffic::TrafficSummary> {
    Ok(client.traffic_summary().await?)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn query_traffic_report(
    client: RpcState<'_, ChimeraClient>,
    request: chimera_traffic::ReportRequest,
) -> Result<chimera_traffic::TrafficReport> {
    Ok(client.query_traffic_report(request).await?)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn query_traffic_usage(
    client: RpcState<'_, ChimeraClient>,
    query: chimera_traffic::TrafficQuery,
    group_by: chimera_traffic::Dimension,
    metric: chimera_traffic::Metric,
    after: Option<chimera_traffic::UsageCursor>,
    limit: usize,
) -> Result<chimera_traffic::UsagePage> {
    Ok(client
        .query_traffic_usage(query, group_by, metric, after, limit)
        .await?)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn query_traffic_usage_by_keys(
    client: RpcState<'_, ChimeraClient>,
    query: chimera_traffic::TrafficQuery,
    group_by: chimera_traffic::Dimension,
    keys: Vec<String>,
) -> Result<Vec<chimera_traffic::UsageGroup>> {
    Ok(client
        .query_traffic_usage_by_keys(query, group_by, keys)
        .await?)
}

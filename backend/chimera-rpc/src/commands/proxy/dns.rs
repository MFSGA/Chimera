//! Shared declarations from the pinned ref proxy.dns.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http], query)]
pub async fn clash_api_query_dns(
    client: RpcState<'_, ChimeraClient>,
    name: String,
    record_type: String,
) -> Result<clash_api::DnsResponse> {
    Ok(client.clash_api_query_dns(name, record_type).await?)
}

#[chimera_rpc::rpc(expose = [http])]
pub async fn clash_api_flush_fake_ip_cache(client: RpcState<'_, ChimeraClient>) -> Result<()> {
    Ok(client.clash_api_flush_fake_ip_cache().await?)
}

#[chimera_rpc::rpc(expose = [http])]
pub async fn clash_api_flush_dns_cache(client: RpcState<'_, ChimeraClient>) -> Result<()> {
    Ok(client.clash_api_flush_dns_cache().await?)
}

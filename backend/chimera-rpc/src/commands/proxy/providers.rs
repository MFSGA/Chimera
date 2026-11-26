//! Shared declarations from the pinned ref proxy.providers.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;
use indexmap::IndexMap;

#[chimera_rpc::rpc(expose = [http], optional_capabilities(presentation), query)]
pub async fn clash_api_get_providers_proxies(
    client: RpcState<'_, ChimeraClient>,
) -> Result<IndexMap<clash_api::ProviderName, chimera_core::clash::proxies::ProxyProviderSummary>> {
    Ok(client.proxy_providers().await?)
}

#[chimera_rpc::rpc(expose = [http], optional_capabilities(presentation))]
pub async fn clash_api_healthcheck_proxy_provider(
    client: RpcState<'_, ChimeraClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}

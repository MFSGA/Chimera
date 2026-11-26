//! Shared declarations from the pinned ref proxy.groups.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http, local], optional_capabilities(presentation), query)]
pub async fn get_proxies(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::clash::proxies::Proxies> {
    Ok(client.get_proxies().await?)
}

#[chimera_rpc::rpc(optional_capabilities(presentation), expose = [http])]
pub async fn mutate_proxies(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::clash::proxies::Proxies> {
    Ok(client.refresh_proxies().await?)
}

#[chimera_rpc::rpc(expose = [http, local], optional_capabilities(presentation), privacy)]
pub async fn select_proxy(
    client: RpcState<'_, ChimeraClient>,
    group: String,
    name: String,
) -> Result<chimera_rpc_protocol::types::RpcUnitMutationOutcome> {
    let strategy = client.get_clash_config()?.break_connection.on_proxy_change;
    Ok(client.select_proxy(group, name, strategy).await?.into())
}

#[chimera_rpc::rpc(optional_capabilities(presentation), privacy, expose = [http])]
pub async fn clear_proxy_fixed(
    client: RpcState<'_, ChimeraClient>,
    group: String,
) -> Result<chimera_rpc_protocol::types::RpcUnitMutationOutcome> {
    let strategy = client.get_clash_config()?.break_connection.on_proxy_change;
    Ok(client.clear_proxy_fixed(group, strategy).await?.into())
}

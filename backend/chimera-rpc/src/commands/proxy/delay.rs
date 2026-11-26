//! Shared declarations from the pinned ref proxy.delay.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;
use indexmap::IndexMap;

#[chimera_rpc::rpc(expose = [http, local], optional_capabilities(presentation), query, access = write)]
pub async fn clash_api_get_group_delay(
    client: RpcState<'_, ChimeraClient>,
    group: String,
    url: Option<String>,
    expected: Option<String>,
) -> Result<IndexMap<clash_api::ProxyName, u16>> {
    Ok(client.group_delay(group, url, expected).await?)
}

//! Shared declarations from the pinned ref profile.providers.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http])]
pub async fn clash_api_update_providers_rules(
    client: RpcState<'_, ChimeraClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[chimera_rpc::rpc(expose = [http], optional_capabilities(presentation))]
pub async fn update_proxy_provider(
    client: RpcState<'_, ChimeraClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}

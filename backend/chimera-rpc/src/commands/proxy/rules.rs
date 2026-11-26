//! Shared declarations from the pinned ref proxy.rules.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http], query)]
pub async fn clash_api_get_providers_rules(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::clash::api::ProvidersRulesRes> {
    Ok(client.clash_rule_providers().await?)
}

#[chimera_rpc::rpc(expose = [http], query)]
pub async fn clash_api_get_rules(
    client: RpcState<'_, ChimeraClient>,
) -> Result<chimera_core::clash::api::RulesRes> {
    Ok(client.clash_rules().await?)
}

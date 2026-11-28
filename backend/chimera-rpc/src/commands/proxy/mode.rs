//! The proxy mode.

use chimera_config::clash::config::overrides::Mode;
use chimera_core::client::ChimeraClient;
use chimera_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::registry::RpcState;
use anyhow::Result;

/// Switch the proxy mode (rule, global, direct).
#[chimera_rpc::rpc(
    privacy,
    optional_capabilities(elevation, os_integration, presentation),
    expose = [http, local]
)]
pub async fn set_proxy_mode(
    client: RpcState<'_, ChimeraClient>,
    mode: Mode,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_proxy_mode(mode).await?.into())
}

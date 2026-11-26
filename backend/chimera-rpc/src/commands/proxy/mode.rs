//! The proxy mode.

use chimera_config::clash::config::overrides::Mode;
use chimera_core::client::configuration_mutations::ConfigurationMutations;
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
    host: RpcState<'_, dyn ConfigurationMutations>,
    mode: Mode,
) -> Result<RpcUnitMutationOutcome> {
    Ok(host.patch_overrides(mode.into()).await?.into())
}

//! Shared declarations from the pinned ref profile.content.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_config::profile::ProfileId;
use chimera_core::client::ChimeraClient;
use chimera_rpc_protocol::types::RpcUnitMutationOutcome;

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn save_profile_file(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    file_data: String,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.save_profile_file(uid, file_data).await?.into())
}

#[chimera_rpc::rpc(operation = chimera_rpc_protocol::operations::ReadProfileFile, query, sensitive, expose = [http, local])]
pub async fn read_profile_file(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

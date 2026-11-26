//! Shared declarations from the pinned ref profile.layout.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_config::profile::{ProfileId, ProfileLayout, ProxyGroupLayout};
use chimera_core::client::ChimeraClient;
use chimera_rpc_protocol::types::{RpcFolderMutationOutcome, RpcUnitMutationOutcome};

#[chimera_rpc::rpc(expose = [http], privacy, optional_capabilities(presentation))]
pub async fn create_profile_folder(
    client: RpcState<'_, ChimeraClient>,
    name: String,
    members: Vec<ProfileId>,
) -> Result<RpcFolderMutationOutcome> {
    Ok(client.create_profile_folder(name, members).await?.into())
}

#[chimera_rpc::rpc(expose = [http], privacy, optional_capabilities(presentation))]
pub async fn create_proxy_group_folder(
    client: RpcState<'_, ChimeraClient>,
    profile: ProfileId,
    name: String,
    members: Vec<String>,
) -> Result<RpcFolderMutationOutcome> {
    Ok(client
        .create_proxy_group_folder(profile, name, members)
        .await?
        .into())
}

#[chimera_rpc::rpc(expose = [http], privacy, optional_capabilities(presentation))]
pub async fn set_profile_layout(
    client: RpcState<'_, ChimeraClient>,
    layout: ProfileLayout,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_profile_layout(layout).await?.into())
}

#[chimera_rpc::rpc(expose = [http], privacy, optional_capabilities(presentation))]
pub async fn set_proxy_group_layout(
    client: RpcState<'_, ChimeraClient>,
    profile: ProfileId,
    layout: ProxyGroupLayout,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_proxy_group_layout(profile, layout).await?.into())
}

//! Shared declarations from the pinned ref profile.store.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileMetadataPatch,
    ProfilesListing, RemoteProfileOptionsPatch, TransformKind,
};
use chimera_core::client::ChimeraClient;
use chimera_rpc_protocol::types::{RpcProfileMutationOutcome, RpcUnitMutationOutcome};

#[chimera_rpc::rpc(expose = [http, local],
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
pub async fn activate_profile(
    client: RpcState<'_, ChimeraClient>,
    uid: Option<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.activate_profile(uid).await?.into())
}

#[chimera_rpc::rpc(expose = [http, local], privacy, operation = chimera_rpc_protocol::operations::CreateProfile, optional_capabilities(elevation, os_integration, presentation))]
pub async fn create_profile(
    client: RpcState<'_, ChimeraClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<RpcProfileMutationOutcome> {
    // Must return the created ProfileId; never drop it on the wire.
    Ok(client.create_profile(request, file_data).await?.into())
}

#[chimera_rpc::rpc(expose = [http, local], privacy, operation = chimera_rpc_protocol::operations::DeleteProfile, optional_capabilities(elevation, os_integration, presentation))]
pub async fn delete_profile(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.delete_profile(uid).await?.into())
}

#[chimera_rpc::rpc(optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn enhance_profiles(client: RpcState<'_, ChimeraClient>) -> Result<()> {
    client.retry_configuration_runtime().await?;
    Ok(())
}

#[chimera_rpc::rpc(expose = [http, local], query)]
pub fn get_profiles_listing(client: RpcState<'_, ChimeraClient>) -> Result<ProfilesListing> {
    client.get_profiles_listing()
}

#[chimera_rpc::rpc(expose = [http, local],
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
pub async fn import_profile(
    client: RpcState<'_, ChimeraClient>,
    url: String,
    name: Option<String>,
    option: Option<RemoteProfileOptionsPatch>,
    transform: Option<TransformKind>,
    folder: Option<FolderId>,
) -> Result<RpcProfileMutationOutcome> {
    let url = url::Url::parse(&url)?;
    // `name` carries deep-link intent (e.g. an install-config `name=` param);
    // when absent the facade derives the name from the url server-side. Return
    // MutationOutcome so a degraded post-import rebuild still carries the uid.
    Ok(client
        .import_profile_with_transform(url, name, option, transform, folder)
        .await?
        .into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(presentation), expose = [http])]
pub async fn patch_profile_metadata(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_profile_metadata(uid, patch).await?.into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn patch_remote_profile_options(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client
        .patch_remote_profile_options(uid, patch)
        .await?
        .into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(presentation), expose = [http])]
pub async fn reorder_profile(
    client: RpcState<'_, ChimeraClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reorder_profile(active_id, over_id).await?.into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(presentation), expose = [http])]
pub async fn reorder_profiles_by_list(
    client: RpcState<'_, ChimeraClient>,
    list: Vec<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reorder_profiles_by_list(list).await?.into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn replace_profile_definition(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client
        .replace_profile_definition(uid, definition)
        .await?
        .into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn set_global_transforms(
    client: RpcState<'_, ChimeraClient>,
    ids: Vec<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_global_transforms(ids).await?.into())
}

#[chimera_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
pub async fn set_profile_valid_fields(
    client: RpcState<'_, ChimeraClient>,
    fields: Vec<String>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_profile_valid_fields(fields).await?.into())
}

#[chimera_rpc::rpc(expose = [http, local],
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
pub async fn update_profile(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.refresh_profile(uid, option).await?.into())
}

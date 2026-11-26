//! Shared Profile sync queries.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_config::profile::ProfileId;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn get_profile_sync_logs(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    run: String,
    after: Option<String>,
) -> Result<chimera_jobs::dto::LogPageDto> {
    Ok(client.profile_sync_logs(uid, run, after).await?)
}

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn get_profile_sync_runs(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    after: Option<chimera_jobs::dto::RunCursorDto>,
) -> Result<chimera_jobs::dto::RunPageDto> {
    Ok(client.profile_sync_runs(uid, after).await?)
}

#[chimera_rpc::rpc(query, sensitive, expose = [http])]
pub async fn get_profile_sync_status(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
) -> Result<chimera_core::client::profiles::ProfileSyncStatus> {
    Ok(client.profile_sync_status(uid).await?)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_profile_sync_runs_summary(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
    after: Option<chimera_jobs::dto::RunCursorDto>,
) -> Result<chimera_core::client::profile_sync_summary::SyncRunPageSummary> {
    Ok((&client.profile_sync_runs(uid, after).await?).into())
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_profile_sync_status_summary(
    client: RpcState<'_, ChimeraClient>,
    uid: ProfileId,
) -> Result<chimera_core::client::profile_sync_summary::ProfileSyncSummary> {
    Ok((&client.profile_sync_status(uid).await?).into())
}

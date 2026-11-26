use std::{collections::HashMap, path::PathBuf, result::Result as StdResult};

use anyhow::{Context, anyhow};
use indexmap::IndexMap;
use specta_typescript::Any;

use chimera_ipc::api::status::CoreState;
use sysproxy::Sysproxy;
use tauri::{AppHandle, Emitter, Manager, State, Webview};
use tauri_plugin_dialog::DialogExt;

use crate::{
    client::{
        ChimeraClient, MutationOutcome, RuntimeInspection, RuntimeInspectionContent,
        RuntimeTransformDiagnostics,
    },
    config::{
        chimera::{self, IVerge},
        clash::ClashInfo,
        profile::item::remote::RemoteProfileImportMode,
        profile::item_type::ProfileUid,
        runtime::{ClashConfigOverrides, PatchClashCoreConfig, PatchRuntimeConfig},
    },
    core::{
        clash::{self, core::RunType},
        storage::{Storage, StorageOperationError, WebStorage},
        updater::{self, ManifestVersionLatest},
    },
    enhance::PostProcessingOutput,
    feat,
    network_probe::{NetworkProbeRequest, NetworkProbeResult},
    utils::{dirs, help, resolve},
};

type Result<T = ()> = StdResult<T, IpcError>;

#[derive(Clone, Debug, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct PendingDeepLinkEntry {
    pub id: u32,
    pub url: String,
}

#[derive(Default)]
struct PendingDeepLinkState {
    next_id: u32,
    entries: std::collections::VecDeque<PendingDeepLinkEntry>,
}

#[derive(Default)]
pub struct PendingDeepLink(std::sync::Mutex<PendingDeepLinkState>);

impl PendingDeepLink {
    pub fn store(&self, url: String) -> PendingDeepLinkEntry {
        let mut pending = self.0.lock().unwrap();
        pending.next_id = pending.next_id.wrapping_add(1).max(1);
        let entry = PendingDeepLinkEntry {
            id: pending.next_id,
            url,
        };
        pending.entries.push_back(entry.clone());
        entry
    }

    fn list_all(&self) -> Vec<PendingDeepLinkEntry> {
        self.0.lock().unwrap().entries.iter().cloned().collect()
    }

    fn claim(&self, id: u32) -> bool {
        let mut pending = self.0.lock().unwrap();
        let Some(index) = pending.entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        pending.entries.remove(index);
        true
    }
}

pub use crate::window::kinds::EditorWindowType;

#[derive(specta::Type, serde::Serialize)]
pub struct GetSysProxyResponse {
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub bypass: String,
    pub server: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, specta::Type)]
pub struct IpsbResponse {
    pub organization: String,
    pub longitude: f64,
    pub timezone: String,
    pub isp: String,
    pub offset: i64,
    pub asn: i64,
    pub asn_organization: String,
    pub country: String,
    pub ip: String,
    pub latitude: f64,
    pub continent_code: String,
    pub country_code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    Storage(#[from] StorageOperationError),
    #[error("{0}")]
    Custom(String),
    #[error("the file {} of profile {uid} does not exist", path.display())]
    ProfileFileMissing {
        uid: chimera_config::profile::ProfileId,
        path: PathBuf,
    },
}

impl serde::Serialize for IpcError {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(format!("{self:#?}").as_str())
    }
}

impl specta::Type for IpcError {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        let _ = types;
        specta::datatype::DataType::Primitive(specta::datatype::Primitive::str)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_profiles(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_config::profile::Profiles> {
    let profiles = client.get_profiles()?;
    Ok((*profiles).clone())
}

/// Detailed read for the trusted local desktop, not the broadcast event.
#[tauri::command]
#[specta::specta]
pub fn get_configuration_status(
    client: State<'_, ChimeraClient>,
) -> chimera_core::client::configuration::ConfigurationStatus {
    client.configuration_status()
}

#[tauri::command]
#[specta::specta]
pub fn get_configuration_status_summary(
    client: State<'_, ChimeraClient>,
) -> chimera_core::client::configuration_observation::ConfigurationStatusSummary {
    (&client.configuration_status()).into()
}

#[tauri::command]
#[specta::specta]
pub async fn retry_configuration_runtime(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<()> {
    Ok(client
        .retry_configuration_runtime_rpc(window.label())
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn retry_configuration_effect(
    client: State<'_, ChimeraClient>,
    kind: chimera_core::client::configuration::EffectKind,
) -> Result<()> {
    Ok(client.retry_configuration_effect(kind)?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
) -> Result<crate::client::jobs::ProfileSyncStatus> {
    Ok(client
        .profile_sync_status(chimera_config::profile::ProfileId(uid))
        .await?)
}

/// Dashboard-safe status: do not expose registration errors containing profile URLs.
#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_status_summary(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
) -> Result<crate::client::jobs::ProfileSyncSummary> {
    let status = client
        .profile_sync_status(chimera_config::profile::ProfileId(uid))
        .await
        .map_err(|_| IpcError::Custom("profile sync summary unavailable".into()))?;
    Ok((&status).into())
}

#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_runs(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    after: Option<chimera_jobs::dto::RunCursorDto>,
) -> Result<chimera_jobs::dto::RunPageDto> {
    Ok(client
        .profile_sync_runs(chimera_config::profile::ProfileId(uid), after)
        .await?)
}

/// Dashboard-safe history: omit failure text, outputs and job identifiers.
#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_runs_summary(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    after: Option<chimera_jobs::dto::RunCursorDto>,
) -> Result<crate::client::jobs::SyncRunPageSummary> {
    let page = client
        .profile_sync_runs(chimera_config::profile::ProfileId(uid), after)
        .await
        .map_err(|_| IpcError::Custom("profile sync summary unavailable".into()))?;
    Ok((&page).into())
}

#[tauri::command]
#[specta::specta]
pub async fn get_profile_sync_logs(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    run: String,
    after: Option<String>,
) -> Result<chimera_jobs::dto::LogPageDto> {
    Ok(client
        .profile_sync_logs(chimera_config::profile::ProfileId(uid), run, after)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_runtime_transform_diagnostics(
    client: State<'_, ChimeraClient>,
) -> Result<Option<RuntimeTransformDiagnostics>> {
    Ok(client.runtime_transform_diagnostics()?)
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_runtime(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<Option<RuntimeInspection>> {
    Ok(client
        .call_shared_rpc(window.label(), "inspect_runtime", serde_json::json!({}))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_applied_runtime(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<Option<RuntimeInspection>> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "inspect_applied_runtime",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_runtime_exists(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<Vec<String>> {
    Ok(client
        .call_shared_rpc(window.label(), "get_runtime_exists", serde_json::json!({}))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_postprocessing_output(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<PostProcessingOutput> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "get_postprocessing_output",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
    snapshot_id: String,
    node_id: u32,
) -> Result<RuntimeInspectionContent> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "inspect_runtime_node",
            serde_json::json!({"snapshotId": snapshot_id, "nodeId": node_id}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn flush_system_dns_cache(client: State<'_, ChimeraClient>) -> Result {
    client.flush_system_dns_cache().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn probe_network(request: NetworkProbeRequest) -> StdResult<NetworkProbeResult, String> {
    crate::network_probe::execute_user_target(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_sys_proxy() -> Result<GetSysProxyResponse> {
    let current = (Sysproxy::get_system_proxy()).context("failed to get system proxy")?;
    let server = format!("{}:{}", current.host, current.port);
    Ok(GetSysProxyResponse {
        enable: current.enable,
        host: current.host,
        port: current.port,
        bypass: current.bypass,
        server,
    })
}

#[cfg(target_os = "windows")]
#[tauri::command]
#[specta::specta]
pub fn is_portable() -> Result<bool> {
    Ok(crate::utils::dirs::get_portable_flag())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
#[specta::specta]
pub fn is_portable() -> Result<bool> {
    Ok(false)
}

/// later: check in the frontend
#[tauri::command]
#[specta::specta]
pub async fn get_pending_deep_links(
    pending: State<'_, PendingDeepLink>,
) -> Result<Vec<PendingDeepLinkEntry>> {
    Ok(pending.list_all())
}

#[tauri::command]
#[specta::specta]
pub async fn claim_pending_deep_link(pending: State<'_, PendingDeepLink>, id: u32) -> Result<bool> {
    Ok(pending.claim(id))
}

#[tauri::command]
#[specta::specta]
pub async fn import_profile(
    client: State<'_, ChimeraClient>,
    url: String,
    name: Option<String>,
    option: Option<chimera_config::profile::RemoteProfileOptionsPatch>,
    folder: Option<chimera_config::profile::FolderId>,
) -> Result<MutationOutcome<chimera_config::profile::ProfileId>> {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    Ok(client
        .import_profile_to_folder(url, name, option, folder)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn import_profile_with_mode(
    client: State<'_, ChimeraClient>,
    url: String,
    name: Option<String>,
    mut option: Option<chimera_config::profile::RemoteProfileOptionsPatch>,
    mode: RemoteProfileImportMode,
    folder: Option<chimera_config::profile::FolderId>,
) -> Result<MutationOutcome<chimera_config::profile::ProfileId>> {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    if matches!(mode, RemoteProfileImportMode::Direct) {
        let patch = option.get_or_insert_with(Default::default);
        patch.with_proxy = Some(false);
        patch.self_proxy = Some(false);
    }
    Ok(client
        .import_profile_to_folder(url, name, option, folder)
        .await?)
}

fn ensure_viewable_profile_file(
    uid: chimera_config::profile::ProfileId,
    path: &std::path::Path,
) -> Result {
    if !path.exists() {
        return Err(IpcError::ProfileFileMissing {
            uid,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn view_profile(
    app_handle: tauri::AppHandle,
    client: State<'_, ChimeraClient>,
    uid: String,
) -> Result {
    let uid = chimera_config::profile::ProfileId(uid);
    let path = client.get_profile_materialized_path(uid.clone()).await?;
    ensure_viewable_profile_file(uid, &path)?;
    help::open_file(app_handle, path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn create_editor_window(
    windows: State<'_, crate::window::WindowManager>,
    window_type: EditorWindowType,
    uid: Option<String>,
) -> Result {
    let (kind, params) = crate::window::kinds::EditorWindow::for_type(window_type, uid.as_deref())?;
    windows.open(&kind, params)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_verge_config(client: State<'_, ChimeraClient>) -> Result<IVerge> {
    Ok(client.application_config())
}

#[tauri::command]
#[specta::specta]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        crate::client::hotkey::ports::HotkeyAction::all()
            .iter()
            .filter(|action| crate::client::hotkey::ports::action_available(**action))
            .map(|action| action.as_str())
            .collect()
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    Vec::new()
}

#[tauri::command]
#[specta::specta]
pub async fn patch_verge_config(
    app_handle: AppHandle,
    client: State<'_, ChimeraClient>,
    payload: IVerge,
) -> Result {
    client.patch_verge(payload).await?;
    if let Err(error) = app_handle.emit("verge-config-updated", ()) {
        log::warn!(target: "app", "failed to emit verge config update: {error:?}");
    }
    Ok(())
}

/// An explicitly opted-in STUN probe of the OS direct route. Never uses an
/// HTTP proxy, and refuses to probe while TUN or local capture is active.
#[tauri::command]
#[specta::specta]
pub fn get_ip_lookup_providers() -> Vec<chimera_config::application::IpLookupProviderInfo> {
    crate::network_info::ip_lookup_providers()
}

#[tauri::command]
#[specta::specta]
pub async fn test_stun_server(
    host: String,
    port: u16,
) -> Result<crate::network_info::StunServerTest> {
    Ok(crate::network_info::test_stun_server(host, port).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn probe_direct_egress(
    client: State<'_, ChimeraClient>,
) -> Result<crate::network_info::DirectEgress> {
    Ok(crate::network_info::probe_direct_egress(&client).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_config(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_config::application::ChimeraAppConfig> {
    // The generic IPC is not scoped to sensitive_read. SSIDs, gateway MACs,
    // rule IDs and policy actions require a dedicated authorized read.
    Ok(client.get_app_config()?.unprivileged_settings_view())
}

/// Ordinary preference edits cannot overwrite network rules. A dedicated
/// system:write RPC is required even to edit disabled network conditions.
fn ensure_general_settings_patch_scope(
    patch: &chimera_config::application::ChimeraAppConfigPatch,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        patch.network_automation.is_none(),
        "network automation policy edits require the dedicated authorized settings command"
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_app_config(
    client: State<'_, ChimeraClient>,
    patch: chimera_config::application::ChimeraAppConfigPatch,
) -> Result<MutationOutcome<()>> {
    ensure_general_settings_patch_scope(&patch)?;
    Ok(client.patch_app_config(patch).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_config(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_config::clash::config::ClashConfig> {
    Ok(client.get_clash_config()?)
}

/// DNS key and value support for the currently selected core. The renderer
/// uses this same matrix, so the editor never guesses capabilities by name.
#[tauri::command]
#[specta::specta]
pub fn get_dns_features(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_config::clash::features::DnsFeatures> {
    Ok(client.get_app_config()?.core.dns_features())
}

/// Mirror ref's read-only system.core query over the runtime capability matrix.
#[tauri::command]
#[specta::specta]
pub fn get_config_features(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_config::clash::features::ConfigFeatures> {
    Ok(client.config_features())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_clash_config(
    client: State<'_, ChimeraClient>,
    patch: chimera_config::clash::config::ClashConfigPatch,
) -> Result<MutationOutcome<()>> {
    Ok(client.patch_clash_config(patch).await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_hotkeys(client: State<'_, ChimeraClient>) -> Result<Vec<String>> {
    Ok(client.get_app_config()?.hotkeys)
}

#[tauri::command]
#[specta::specta]
pub async fn set_hotkeys(
    client: State<'_, ChimeraClient>,
    hotkeys: Vec<String>,
) -> Result<MutationOutcome<()>> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Ok(client.set_hotkeys(hotkeys).await?)
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (client, hotkeys);
        Err(anyhow!("global shortcuts are only supported on desktop").into())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(
    client: State<'_, ChimeraClient>,
    active_id: ProfileUid,
    over_id: ProfileUid,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .reorder_profile(
            chimera_config::profile::ProfileId(active_id),
            chimera_config::profile::ProfileId(over_id),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, ChimeraClient>,
    list: Vec<ProfileUid>,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .reorder_profiles_by_list(
            list.into_iter()
                .map(chimera_config::profile::ProfileId)
                .collect(),
        )
        .await?)
}

/// Replace the full Config folder layout in a single profile mutation.
#[tauri::command]
#[specta::specta]
pub async fn set_profile_layout(
    client: State<'_, ChimeraClient>,
    layout: chimera_config::profile::ProfileLayout,
) -> Result<MutationOutcome<()>> {
    Ok(client.set_profile_layout(layout).await?)
}

/// Create a folder and return its server-generated stable id.
#[tauri::command]
#[specta::specta]
pub async fn create_profile_folder(
    client: State<'_, ChimeraClient>,
    name: String,
    members: Vec<chimera_config::profile::ProfileId>,
) -> Result<MutationOutcome<chimera_config::profile::FolderId>> {
    Ok(client.create_profile_folder(name, members).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_proxy_group_layout(
    client: State<'_, ChimeraClient>,
    profile: chimera_config::profile::ProfileId,
    layout: chimera_config::profile::ProxyGroupLayout,
) -> Result<MutationOutcome<()>> {
    Ok(client.set_proxy_group_layout(profile, layout).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn create_proxy_group_folder(
    client: State<'_, ChimeraClient>,
    profile: chimera_config::profile::ProfileId,
    name: String,
    members: Vec<String>,
) -> Result<MutationOutcome<chimera_config::profile::FolderId>> {
    Ok(client
        .create_proxy_group_folder(profile, name, members)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activate_profile(
    client: State<'_, ChimeraClient>,
    uid: Option<ProfileUid>,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .activate_profile(uid.map(chimera_config::profile::ProfileId))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, ChimeraClient>,
    fields: Vec<String>,
) -> Result<MutationOutcome<()>> {
    Ok(client.set_profile_valid_fields(fields).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_global_transforms(
    client: State<'_, ChimeraClient>,
    ids: Vec<chimera_config::profile::ProfileId>,
) -> Result<MutationOutcome<()>> {
    Ok(client.set_global_transforms(ids).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    patch: chimera_config::profile::ProfileMetadataPatch,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .patch_profile_metadata(chimera_config::profile::ProfileId(uid), patch)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    patch: chimera_config::profile::RemoteProfileOptionsPatch,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .patch_remote_profile_options(chimera_config::profile::ProfileId(uid), patch)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn replace_profile_definition(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    definition: chimera_config::profile::ProfileDefinition,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .replace_profile_definition(chimera_config::profile::ProfileId(uid), definition)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_clash_info(client: State<'_, ChimeraClient>) -> Result<ClashInfo> {
    Ok(client.clash_info())
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_update_state(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.get_app_update_state().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_update_changelog(
    client: State<'_, ChimeraClient>,
    from: String,
    to: String,
) -> Result<chimera_core::client::app_update::changelog::AppUpdateChangelog> {
    Ok(client.get_app_update_changelog(&from, &to).await?)
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReleaseChannelInfo {
    pub current: chimera_config::application::ReleaseChannel,
    pub installed: chimera_config::application::ReleaseChannel,
}

#[tauri::command]
#[specta::specta]
pub fn get_release_channel(client: State<'_, ChimeraClient>) -> Result<ReleaseChannelInfo> {
    Ok(ReleaseChannelInfo {
        current: client.release_channel()?,
        installed: client
            .installed_release_channel()
            .ok_or_else(|| anyhow::anyhow!("application bundle metadata is unavailable"))?,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn set_release_channel(
    client: State<'_, ChimeraClient>,
    channel: chimera_config::application::ReleaseChannel,
) -> Result<MutationOutcome<()>> {
    Ok(client.set_release_channel(channel).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn check_app_update(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.check_app_update().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn download_app_update(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.download_app_update().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_app_update_download(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.cancel_app_update_download().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn install_app_update(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.install_app_update().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn discard_app_update_package(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::client::app_update::AppUpdateSnapshot> {
    Ok(client.discard_app_update_package().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn is_appimage(client: State<'_, ChimeraClient>, window: tauri::Window) -> Result<bool> {
    Ok(client
        .call_shared_rpc(window.label(), "is_appimage", serde_json::json!({}))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn open_that(path: String) -> Result {
    crate::utils::open::that(path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cleanup_processes(app_handle: AppHandle) -> Result {
    // The updater invokes this after downloading and immediately before
    // installation, which may terminate the process without a Tauri exit event.
    crate::utils::exit::shutdown_before_exit(&app_handle).await?;
    Ok(())
}

use chimera_core::storage::frontend::{
    clear_frontend_storage, frontend_storage_entries, get_web_storage_item,
    remove_web_storage_item, set_web_storage_item,
};

pub mod service {
    use super::{ChimeraClient, Result, State};
    use crate::core::service;

    fn ensure_service_can_stop_for_tun(client: &ChimeraClient) -> Result {
        #[cfg(target_os = "windows")]
        if client.get_clash_config()?.enable_tun_mode {
            return Err(anyhow::anyhow!(
                "disable TUN before stopping or uninstalling Chimera Service on Windows"
            )
            .into());
        }
        #[cfg(not(target_os = "windows"))]
        let _ = client;
        Ok(())
    }

    /// Cached service-host projection for UI consumers. It preserves the service
    /// wire fields while exposing lifecycle phase, compatibility, and runtime ownership.
    #[derive(serde::Serialize, specta::Type)]
    pub struct ServiceStatusInfo<'a> {
        pub name: std::borrow::Cow<'a, str>,
        pub version: std::borrow::Cow<'a, str>,
        pub status: chimera_ipc::types::ServiceStatus,
        pub server: Option<chimera_ipc::api::status::StatusResBody<'a>>,
        pub phase: chimera_core::actor_v2::service_policy::ServicePhase,
        pub compat: chimera_core::service::compat::ServiceCompat,
        pub runtime_owned: bool,
        pub restart_attempts: u8,
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn status_service(
        client: State<'_, ChimeraClient>,
    ) -> Result<ServiceStatusInfo<'static>> {
        let status = client.service_status();
        Ok(ServiceStatusInfo {
            name: status.name,
            version: status.version,
            status: status.status,
            server: status.server,
            phase: status.phase,
            compat: status.compat,
            runtime_owned: status.runtime_owned,
            restart_attempts: status.restart_attempts,
        })
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn install_service(client: State<'_, ChimeraClient>) -> Result {
        service::install_service_and_converge(&client).await?;
        Ok(())
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn uninstall_service(client: State<'_, ChimeraClient>) -> Result {
        ensure_service_can_stop_for_tun(&client)?;
        service::uninstall_service_and_converge(&client).await?;
        Ok(())
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn start_service(client: State<'_, ChimeraClient>) -> Result {
        service::start_service_and_converge(&client, std::time::Duration::from_secs(8)).await?;
        Ok(())
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service(client: State<'_, ChimeraClient>) -> Result {
        ensure_service_can_stop_for_tun(&client)?;
        service::stop_service_and_converge(&client).await?;
        Ok(())
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn restart_service(client: State<'_, ChimeraClient>) -> Result {
        service::restart_service_and_converge(&client, std::time::Duration::from_secs(8)).await?;
        Ok(())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_service_install_prompt(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<String> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "get_service_install_prompt",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_runtime_overrides(
    client: State<'_, ChimeraClient>,
    patch: PatchRuntimeConfig,
) -> Result {
    tracing::debug!(
        allow_lan = ?patch.allow_lan,
        ipv6 = ?patch.ipv6,
        log_level = ?patch.log_level,
        mode = ?patch.mode,
        "patch clash runtime config"
    );
    let overrides = ClashConfigOverrides::from(patch);
    let outcome = feat::patch_running_clash_overrides(&client, overrides).await;
    match &outcome {
        clash::transaction::TransactionOutcome::Committed => {}
        clash::transaction::TransactionOutcome::CommittedDeferred { message } => {
            tracing::warn!(%message, "Clash override source committed; runtime reconciliation deferred");
        }
        clash::transaction::TransactionOutcome::Rejected { primary_error } => {
            tracing::warn!(%primary_error, "runtime patch rejected before core mutation");
        }
    }
    if let Err(error) = outcome.into_result() {
        return Err(IpcError::from(error));
    }
    feat::update_proxies_buff((*client).clone(), None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_clash_core_config(
    client: State<'_, ChimeraClient>,
    payload: PatchClashCoreConfig,
) -> Result {
    tracing::debug!("patch clash core config: {payload:?}");
    let mapping = match serde_yaml::to_value(&payload)? {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Err(IpcError::Custom("Expected a mapping".to_string())),
    };
    if let Err(e) = feat::patch_clash(&client, mapping).await {
        tracing::error!("{e}");
        return Err(IpcError::from(e));
    }
    feat::update_proxies_buff((*client).clone(), None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_proxies(
    client: State<'_, ChimeraClient>,
) -> Result<crate::core::clash::proxies::LegacyProxies> {
    Ok(client.clash_proxies().await?)
}

/// Additive native proxy view for typed consumers. The legacy `get_proxies`
/// wire shape remains available to existing UI/Agent consumers, but credential-
/// bearing icon URLs are omitted and read through `get_proxy_group_icons`.
#[tauri::command]
#[specta::specta]
pub async fn get_native_proxies(
    client: State<'_, ChimeraClient>,
) -> Result<crate::core::clash::proxies::Proxies> {
    Ok(client.clash_native_proxies().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_proxy_group_icons(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<IndexMap<clash_api::ProxyName, String>> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "get_proxy_group_icons",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn select_proxy(
    group: String,
    name: String,
    client: State<'_, ChimeraClient>,
) -> Result<()> {
    let break_when = client.get_clash_config()?.break_connection.on_proxy_change;
    let outcome = client.select_proxy(group, name, break_when).await?;
    for degradation in outcome.degradations() {
        tracing::warn!(
            code = %degradation.code,
            message = %degradation.message,
            retryable = degradation.retryable,
            "proxy selection committed with a degraded follow-up",
        );
    }
    Ok(())
}

/// Clear an automatic group's pinned node on the bound core. A missing
/// mutation reply is unconfirmed, so never replay it automatically.
#[tauri::command]
#[specta::specta]
pub async fn clear_proxy_fixed(group: String, client: State<'_, ChimeraClient>) -> Result<()> {
    let break_when = client.get_clash_config()?.break_connection.on_proxy_change;
    let outcome = client.clear_proxy_fixed(group, break_when).await?;
    for degradation in outcome.degradations() {
        tracing::warn!(
            code = %degradation.code,
            message = %degradation.message,
            retryable = degradation.retryable,
            "proxy pin cleared with a degraded follow-up",
        );
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_server_port() -> Result<u16> {
    Ok(*crate::core::server::SERVER_PORT)
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_dir(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<String> {
    Ok(client
        .call_shared_rpc(window.label(), "get_core_dir", serde_json::json!({}))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_status(
    client: State<'_, ChimeraClient>,
) -> Result<(CoreState, i64, RunType)> {
    let status = client.core_status().await?;
    Ok((status.state, status.state_changed_at, status.run_type))
}

/// Ref-style typed Core Actor projection. Deliberately separate from the
/// legacy get_core_status tuple: the latter cannot describe transitional or
/// degraded host ownership without pretending the runtime is Stopped.
#[tauri::command]
#[specta::specta]
pub fn get_core_actor_status(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::actor_v2::router::CoreStatusInfo> {
    Ok(client
        .routed_core_status()
        .ok_or_else(|| anyhow!("Core Actor is unavailable"))?
        .into())
}

#[tauri::command]
#[specta::specta]
pub async fn get_transparent_proxy_status(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus> {
    Ok(client.get_transparent_proxy_status().await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_core_lifecycle_status(
    client: State<'_, ChimeraClient>,
) -> Result<crate::client::core_lifecycle::CoreLifecycleStatus> {
    Ok(client.core_lifecycle_status())
}

#[tauri::command]
#[specta::specta]
pub async fn url_delay_test(
    client: State<'_, ChimeraClient>,
    url: &str,
    expected_status: u16,
) -> Result<Option<u64>> {
    Ok(client.url_delay_test(url, expected_status).await)
}

#[tauri::command]
#[specta::specta]
pub async fn get_ipsb_asn(client: State<'_, ChimeraClient>) -> Result<IpsbResponse> {
    let value = client.get_ipsb_asn().await?;
    Ok(serde_json::from_value(value).map_err(anyhow::Error::from)?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_version(
    app_handle: AppHandle,
    core_type: chimera::ClashCore,
) -> Result<String> {
    match resolve::resolve_core_version(&app_handle, &core_type).await {
        Ok(version) => Ok(version),
        Err(err) => Err(IpcError::from(err)),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn collect_logs(client: State<'_, ChimeraClient>) -> Result<String> {
    let (_, archive) =
        crate::utils::collect::collect(client.inner().clone(), Default::default(), true).await?;
    let archive = archive.ok_or_else(|| anyhow::anyhow!("diagnostic archive unavailable"))?;
    Ok(archive.to_string_lossy().into_owned())
}

#[tauri::command]
#[specta::specta]
pub async fn collect_envs(client: State<'_, ChimeraClient>) -> Result<String> {
    Ok(
        crate::utils::collect::collect(client.inner().clone(), Default::default(), false)
            .await?
            .0,
    )
}

#[tauri::command]
#[specta::specta]
pub fn get_custom_app_dir() -> Result<Option<String>> {
    #[cfg(windows)]
    {
        return Ok(
            crate::utils::winreg::get_app_dir()?.map(|path| path.to_string_lossy().to_string())
        );
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn set_custom_app_dir(_app_handle: AppHandle, path: String) -> Result {
    #[cfg(windows)]
    {
        let target = PathBuf::from(path);
        if !target.is_absolute() {
            return Err(IpcError::from(anyhow!("custom app dir must be absolute")));
        }
        let current = dirs::app_config_dir()?;
        if current != target {
            if target.starts_with(&current) {
                return Err(IpcError::from(anyhow!(
                    "custom app dir cannot be inside the current app config dir"
                )));
            }
            fs_extra::dir::create_all(&target, false)
                .map_err(|err| anyhow!("failed to create custom app dir: {err:?}"))?;
            let mut options = fs_extra::dir::CopyOptions::new();
            options.overwrite = true;
            options.copy_inside = true;
            fs_extra::dir::copy(&current, &target, &options)
                .map_err(|err| anyhow!("failed to migrate app config dir: {err:?}"))?;
        }
        crate::utils::winreg::set_app_dir(&target)?;
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(IpcError::from(anyhow!(
            "custom app dir is only supported on Windows"
        )))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn update_core(
    client: State<'_, ChimeraClient>,
    updater: State<'_, tokio::sync::RwLock<updater::UpdaterManager>>,
    core_type: chimera::ClashCore,
) -> Result<usize> {
    let event_id = updater
        .write()
        .await
        .update_core(&client, &core_type)
        .await?;
    Ok(event_id)
}

#[tauri::command]
#[specta::specta]
pub async fn change_clash_core(
    client: State<'_, ChimeraClient>,
    clash_core: Option<chimera::ClashCore>,
) -> Result<MutationOutcome<()>> {
    log::debug!("change_clash_core: {clash_core:?}");
    let clash_core = clash_core.ok_or_else(|| anyhow!("clash core is null"))?;
    Ok(client.change_core(clash_core).await?)
}

/// restart the sidecar
#[tauri::command]
#[specta::specta]
pub async fn restart_sidecar(client: State<'_, ChimeraClient>) -> Result {
    client.rebuild_running_config().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_latest_core_versions(
    updater: State<'_, tokio::sync::RwLock<updater::UpdaterManager>>,
) -> Result<ManifestVersionLatest> {
    let mut updater = updater.write().await;
    updater.fetch_latest().await?;
    Ok(updater.get_latest_versions())
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_updater(
    updater: State<'_, tokio::sync::RwLock<updater::UpdaterManager>>,
    updater_id: usize,
) -> Result<updater::UpdaterSummary> {
    let updater = updater
        .read()
        .await
        .inspect_updater(updater_id)
        .ok_or_else(|| anyhow!("updater not found"))?;
    Ok(updater)
}

#[tauri::command]
#[specta::specta]
pub async fn update_profile(
    client: State<'_, ChimeraClient>,
    uid: String,
    option: Option<chimera_config::profile::RemoteProfileOptionsPatch>,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .refresh_profile(chimera_config::profile::ProfileId(uid), option)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile(
    client: State<'_, ChimeraClient>,
    uid: String,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .delete_profile(chimera_config::profile::ProfileId(uid))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn read_profile_file(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    window: tauri::Window,
) -> Result<String> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "read_profile_file",
            serde_json::json!({"uid": uid}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn save_profile_file(
    client: State<'_, ChimeraClient>,
    uid: ProfileUid,
    file_data: String,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .save_profile_file(chimera_config::profile::ProfileId(uid), file_data)
        .await?)
}

/// create a new profile
#[tauri::command]
#[specta::specta]
pub async fn create_profile(
    client: State<'_, ChimeraClient>,
    request: chimera_config::profile::NewProfileRequest,
    file_data: Option<String>,
) -> Result<MutationOutcome<chimera_config::profile::ProfileId>> {
    Ok(client.create_profile(request, file_data).await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_storage_item(
    app_handle: AppHandle,
    key: String,
) -> Result<crate::core::storage::StorageItem<String>> {
    #[cfg(feature = "e2e")]
    crate::e2e_test::fail_storage_operation("get_storage_item", &key)?;

    let storage = app_handle.state::<Storage>();
    Ok(get_web_storage_item(&storage, &key)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_storage_item(
    app_handle: AppHandle,
    key: String,
    value: String,
    expected_revision: Option<crate::core::storage::StorageRevision>,
) -> Result<crate::core::storage::StorageRevision> {
    #[cfg(feature = "e2e")]
    crate::e2e_test::fail_storage_operation("set_storage_item", &key)?;

    let storage = app_handle.state::<Storage>();
    Ok(set_web_storage_item(
        &storage,
        &key,
        &value,
        expected_revision,
    )?)
}

#[tauri::command]
#[specta::specta]
pub fn remove_storage_item(
    app_handle: AppHandle,
    key: String,
) -> Result<crate::core::storage::StorageRevision> {
    let storage = app_handle.state::<Storage>();
    Ok(remove_web_storage_item(&storage, &key)?)
}

#[tauri::command]
#[specta::specta]
pub async fn save_window_size_state(
    client: State<'_, ChimeraClient>,
    app_handle: AppHandle,
    label: String,
) -> Result {
    crate::window::persist_window_state(&app_handle, client.inner(), &label).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn create_main_window(windows: State<'_, crate::window::WindowManager>) -> Result<()> {
    windows.open(&crate::window::kinds::MainWindow, None)?;
    Ok(())
}

/// Status of this process's initial GUI startup task, independent of the
/// ongoing core lifecycle and safe to query before core reconciliation ends.
#[tauri::command]
#[specta::specta]
pub fn get_app_startup_status(status: State<'_, resolve::AppStartupStatus>) -> String {
    status.phase().to_owned()
}

/// The invoking webview can only report its own readiness.
#[tauri::command]
#[specta::specta]
pub fn report_window_ready(
    windows: State<'_, crate::window::WindowManager>,
    webview: Webview,
) -> Result<()> {
    windows.report_ready(webview.label());
    resolve::mark_frontend_mounted(webview.label());
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn create_legacy_window(windows: State<'_, crate::window::WindowManager>) -> Result<()> {
    windows.open(&crate::window::kinds::LegacyWindow, None)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_runtime_config(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<Option<Any<serde_json::Value>>> {
    let value: Option<serde_json::Value> = client
        .call_shared_rpc(window.label(), "get_runtime_config", serde_json::json!({}))
        .await?;
    Ok(value
        .map(serde_json::from_value)
        .transpose()
        .map_err(anyhow::Error::from)?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_runtime_yaml(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<String> {
    Ok(client
        .call_shared_rpc(window.label(), "get_runtime_yaml", serde_json::json!({}))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn open_app_config_dir() -> Result<()> {
    crate::utils::open::that(dirs::app_config_dir()?)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn open_app_data_dir() -> Result<()> {
    crate::utils::open::that(dirs::app_data_dir()?)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn open_core_dir() -> Result<()> {
    let core_dir = tauri::utils::platform::current_exe()?;
    let core_dir = core_dir.parent().context("failed to get core dir")?;
    crate::utils::open::that(core_dir)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn open_logs_dir() -> Result<()> {
    crate::utils::open::that(dirs::app_logs_dir()?)?;
    Ok(())
}

#[cfg(windows)]
pub mod uwp {
    use super::Result;
    use crate::core::win_uwp;
    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        win_uwp::invoke_uwptools().await?;
        Ok(())
    }
}

#[cfg(not(windows))]
pub mod uwp {
    use super::*;
    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        Ok(())
    }
}

pub use chimera_core::storage::frontend::StorageEntry;

/// Debug: returns namespaced frontend KV entries and the known pre-namespace UI keys.
/// Internal storage entries used by other subsystems are excluded.
#[tauri::command]
#[specta::specta]
pub fn get_all_storage_items(app_handle: AppHandle) -> Result<Vec<StorageEntry>> {
    let storage = app_handle.state::<Storage>();
    let items = storage.get_all()?;
    Ok(frontend_storage_entries(&items))
}

/// Debug: clears namespaced frontend KV entries and known pre-namespace UI keys.
/// Internal storage entries used by other subsystems are left intact.
#[tauri::command]
#[specta::specta]
pub fn clear_storage(app_handle: AppHandle) -> Result {
    let storage = app_handle.state::<Storage>();
    Ok(clear_frontend_storage(&storage)?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_connections_state(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::clash::ws::ClashConnectionsConnectorState> {
    Ok(client.streams().snapshot().await?.state)
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: State<'_, ChimeraClient>,
) -> Result<chimera_core::clash::ws::ClashWsSnapshot> {
    Ok(client.streams().snapshot().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_clash_ws_recording(
    client: State<'_, ChimeraClient>,
    kind: chimera_core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<chimera_core::clash::ws::ClashWsRecording> {
    Ok(client.streams().set_recording(kind, enabled).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: State<'_, ChimeraClient>,
    kind: chimera_core::clash::ws::ClashWsKind,
) -> Result {
    client.streams().clear_history(kind).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn list_log_files(
    client: State<'_, ChimeraClient>,
    source: chimera_contracts::logs::LogSource,
) -> chimera_logging::LogResult<Vec<chimera_logging::LogFileInfo>> {
    client.file_logs()?.list_log_files(source).await
}
#[tauri::command]
#[specta::specta]
pub async fn open_log_session(
    client: State<'_, ChimeraClient>,
    window: tauri::WebviewWindow,
    source: chimera_contracts::logs::LogSource,
    request: chimera_logging::OpenLogs,
) -> chimera_logging::LogResult<chimera_logging::LogSession> {
    client
        .file_logs()?
        .open_log_session(source, window.label().to_owned(), request)
        .await
}
#[tauri::command]
#[specta::specta]
pub async fn query_logs(
    client: State<'_, ChimeraClient>,
    window: tauri::WebviewWindow,
    source: chimera_contracts::logs::LogSource,
    request: chimera_logging::QueryLogs,
) -> chimera_logging::LogResult<chimera_logging::LogPage> {
    client
        .file_logs()?
        .query_logs(source, window.label().to_owned(), request)
        .await
}
#[tauri::command]
#[specta::specta]
pub async fn query_logs_summary(
    client: State<'_, ChimeraClient>,
    window: tauri::WebviewWindow,
    source: chimera_contracts::logs::LogSource,
    request: chimera_core::client::logs::QueryLogSummary,
) -> chimera_logging::LogResult<chimera_core::client::logs::LogPageSummary> {
    client
        .file_logs()?
        .query_logs_summary(source, window.label().to_owned(), request)
        .await
}
#[tauri::command]
#[specta::specta]
pub async fn close_log_session(
    client: State<'_, ChimeraClient>,
    window: tauri::WebviewWindow,
    source: chimera_contracts::logs::LogSource,
    session: String,
) -> chimera_logging::LogResult<()> {
    client
        .file_logs()?
        .close_log_session(source, window.label().to_owned(), session)
        .await
}

#[tauri::command]
#[specta::specta]
pub fn report_frontend_events(
    window: tauri::WebviewWindow,
    batch: chimera_core::client::frontend_events::FrontendEventBatch,
) {
    use chimera_core::client::frontend_events::{TracingFrontendLogSink, report};
    report(&TracingFrontendLogSink, window.label(), batch);
}

fn core_log_reader(
    client: &ChimeraClient,
) -> chimera_core::logs::query::CoreLogResult<chimera_core::logs::query::CoreLogReader> {
    Ok(chimera_core::logs::query::CoreLogReader::new(
        client.streams().core_logs(),
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn query_core_logs(
    client: State<'_, ChimeraClient>,
    query: chimera_core::logs::query::CoreLogQuery,
) -> chimera_core::logs::query::CoreLogResult<chimera_core::logs::query::CoreLogPage> {
    core_log_reader(&client)?.query(query).await
}

#[tauri::command]
#[specta::specta]
pub async fn query_core_logs_summary(
    client: State<'_, ChimeraClient>,
    query: chimera_core::logs::query::CoreLogSummaryQuery,
) -> chimera_core::logs::query::CoreLogResult<chimera_core::logs::query::CoreLogPageSummary> {
    core_log_reader(&client)?.summary(query).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_log(
    client: State<'_, ChimeraClient>,
    cursor: chimera_contracts::logs::CoreLogCursor,
) -> chimera_core::logs::query::CoreLogResult<chimera_core::logs::query::CoreLogRecord> {
    core_log_reader(&client)?.detail(cursor).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_log_status(
    client: State<'_, ChimeraClient>,
) -> chimera_core::logs::query::CoreLogResult<chimera_contracts::logs::CoreLogStatus> {
    core_log_reader(&client)?.status().await
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_log_status_summary(
    client: State<'_, ChimeraClient>,
) -> chimera_core::logs::query::CoreLogResult<chimera_contracts::logs::CoreLogStatusSummary> {
    core_log_reader(&client)?.status_summary().await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_core_logs(
    client: State<'_, ChimeraClient>,
) -> chimera_core::logs::query::CoreLogResult<()> {
    let _ = core_log_reader(&client)?;
    client
        .streams()
        .clear_history(chimera_core::clash::ws::ClashWsKind::Logs)
        .await
        .map_err(|_| {
            chimera_core::logs::query::CoreLogError::Unavailable(
                "core log storage unavailable".into(),
            )
        })
}

// Only a page that consumes full connection rows opens a native Channel.
// Each subscriber is owned by its window and the connector keeps only the latest frame.
#[tauri::command]
#[specta::specta]
pub async fn subscribe_clash_connection_details(
    window: tauri::Window,
    client: State<'_, ChimeraClient>,
    subscriptions: State<'_, crate::core::clash::connection_details::ConnectionDetailSubscriptions>,
    on_frame: tauri::ipc::Channel<std::sync::Arc<chimera_core::clash::ws::ClashConnectionDetails>>,
) -> Result<crate::core::clash::connection_details::SubscriptionId> {
    let receiver = client.streams().subscribe_connection_details().await?;
    let task = tokio::spawn(crate::core::clash::connection_details::forward_details(
        receiver, on_frame,
    ));
    Ok(subscriptions.register(window.label().to_owned(), task))
}

#[tauri::command]
#[specta::specta]
pub fn unsubscribe_clash_connection_details(
    window: tauri::Window,
    subscriptions: State<'_, crate::core::clash::connection_details::ConnectionDetailSubscriptions>,
    id: crate::core::clash::connection_details::SubscriptionId,
) -> Result {
    subscriptions.unsubscribe(id, window.label())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_configs(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<clash::api::ClashApiConfig> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_get_configs",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_version(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<clash::api::ClashVersion> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_get_version",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_query_dns(
    name: String,
    record_type: String,
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<clash_api::DnsResponse> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_query_dns",
            serde_json::json!({"name": name, "recordType": record_type}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_flush_dns_cache(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_flush_dns_cache",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_flush_fake_ip_cache(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_flush_fake_ip_cache",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_rules(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<clash::api::RulesRes> {
    Ok(client
        .call_shared_rpc(window.label(), "clash_api_get_rules", serde_json::json!({}))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_providers_rules(
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<clash::api::ProvidersRulesRes> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_get_providers_rules",
            serde_json::json!({}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: State<'_, ChimeraClient>,
    name: String,
    window: tauri::Window,
) -> Result<()> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_update_providers_rules",
            serde_json::json!({"name": name}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_providers_proxies(
    client: State<'_, ChimeraClient>,
) -> Result<IndexMap<clash_api::ProviderName, clash::proxies::ProxyProviderSummary>> {
    Ok(client.proxy_providers().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn update_proxy_provider(
    client: State<'_, ChimeraClient>,
    name: String,
    window: tauri::Window,
) -> Result<()> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "update_proxy_provider",
            serde_json::json!({"name": name}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn healthcheck_proxy_provider(
    client: State<'_, ChimeraClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    name: String,
    url: Option<String>,
    provider: Option<String>,
    expected: Option<String>,
    client: State<'_, ChimeraClient>,
) -> Result<clash::api::DelayRes> {
    let delay = client.proxy_delay(name, provider, url, expected).await?;
    Ok(clash::api::DelayRes {
        delay: u64::from(delay.delay),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    group: String,
    url: Option<String>,
    expected: Option<String>,
    client: State<'_, ChimeraClient>,
) -> Result<HashMap<String, u32>> {
    let delays = client.group_delay(group, url, expected).await?;
    Ok(delays
        .into_iter()
        .map(|(name, delay)| (name.to_string(), u32::from(delay)))
        .collect())
}

/// Test one member using its live group's own URL or the app default.
/// The URL is never accepted from or returned to the native UI.
#[tauri::command]
#[specta::specta]
pub async fn test_native_proxy_node_delay(
    group: String,
    name: String,
    client: State<'_, ChimeraClient>,
) -> Result<u16> {
    Ok(client.test_native_proxy_node_delay(group, name).await?)
}

/// One instance-bound group delay operation. All member requests share the
/// same running core even if an external profile switch races the test.
#[tauri::command]
#[specta::specta]
pub async fn test_native_proxy_group_delay(
    group: String,
    client: State<'_, ChimeraClient>,
) -> Result<HashMap<String, u32>> {
    let delays = client.test_native_proxy_group_delay(group).await?;
    Ok(delays
        .into_iter()
        .map(|(name, delay)| (name.to_string(), u32::from(delay)))
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_delete_connections(
    id: Option<String>,
    client: State<'_, ChimeraClient>,
    window: tauri::Window,
) -> Result<()> {
    Ok(client
        .call_shared_rpc(
            window.label(),
            "clash_api_delete_connections",
            serde_json::json!({"id": id}),
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_traffic_summary(
    client: tauri::State<'_, ChimeraClient>,
) -> Result<chimera_traffic::TrafficSummary> {
    Ok(client.traffic_summary().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn query_traffic_report(
    client: tauri::State<'_, ChimeraClient>,
    request: chimera_traffic::ReportRequest,
) -> Result<chimera_traffic::TrafficReport> {
    Ok(client.query_traffic_report(request).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn query_traffic_usage(
    client: tauri::State<'_, ChimeraClient>,
    query: chimera_traffic::TrafficQuery,
    group_by: chimera_traffic::Dimension,
    metric: chimera_traffic::Metric,
    after: Option<chimera_traffic::UsageCursor>,
    limit: usize,
) -> Result<chimera_traffic::UsagePage> {
    Ok(client
        .query_traffic_usage(query, group_by, metric, after, limit)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn query_traffic_usage_by_keys(
    client: tauri::State<'_, ChimeraClient>,
    query: chimera_traffic::TrafficQuery,
    group_by: chimera_traffic::Dimension,
    keys: Vec<String>,
) -> Result<Vec<chimera_traffic::UsageGroup>> {
    Ok(client
        .query_traffic_usage_by_keys(query, group_by, keys)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn query_traffic_closed_connections(
    client: tauri::State<'_, ChimeraClient>,
    range: chimera_traffic::TrafficRange,
    filters: Vec<chimera_traffic::TrafficFilter>,
    before: Option<chimera_traffic::ClosedCursor>,
    limit: usize,
) -> Result<chimera_traffic::ClosedPage> {
    Ok(client
        .query_traffic_closed_connections(range, filters, before, limit)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn query_traffic_active_connection_ids(
    client: tauri::State<'_, ChimeraClient>,
    filters: Vec<chimera_traffic::TrafficFilter>,
) -> Result<Vec<String>> {
    Ok(client.query_traffic_active_connection_ids(filters).await?)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::{
        IpcError, PendingDeepLink, clear_frontend_storage, ensure_general_settings_patch_scope,
        ensure_viewable_profile_file, frontend_storage_entries, get_web_storage_item,
        remove_web_storage_item,
    };
    use crate::core::storage::{Storage, StorageOperationError, StorageRevision, WebStorage};

    #[test]
    fn generic_settings_patch_cannot_edit_sensitive_network_automation_policy() {
        use chimera_config::application::ChimeraAppConfigPatch;
        let blocked: ChimeraAppConfigPatch = serde_json::from_value(serde_json::json!({
            "network_automation": {
                "enabled": false,
                "rules": [{
                    "id": "home",
                    "name": "Home",
                    "conditions": { "wifi_ssid": "Home" }
                }]
            }
        }))
        .unwrap();
        let err = ensure_general_settings_patch_scope(&blocked)
            .expect_err("generic settings cannot write network policy");
        assert!(
            err.to_string()
                .contains("dedicated authorized settings command")
        );

        let ordinary: ChimeraAppConfigPatch =
            serde_json::from_value(serde_json::json!({"enable_system_proxy": true})).unwrap();
        ensure_general_settings_patch_scope(&ordinary).unwrap();
    }

    #[test]
    fn storage_revision_conflicts_keep_a_stable_ipc_marker() {
        let revision: StorageRevision = serde_json::from_str("1").unwrap();
        let error = IpcError::from(StorageOperationError::RevisionConflict {
            expected: revision,
            current: revision,
        });

        let wire = serde_json::to_value(error).unwrap();
        let wire = wire.as_str().expect("IPC errors serialize as strings");
        assert!(wire.contains("RevisionConflict"), "{wire}");
    }

    #[test]
    fn legacy_storage_reads_are_limited_to_known_frontend_keys() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::try_new(&temp.path().join("storage.redb")).unwrap();
        storage
            .set_item("connections_table_columns", &"legacy")
            .unwrap();
        storage.set_item("internal", &"preserved").unwrap();

        let legacy = get_web_storage_item(&storage, "connections_table_columns").unwrap();
        assert_eq!(legacy.value.as_deref(), Some("legacy"));
        let internal = get_web_storage_item(&storage, "internal").unwrap();
        assert_eq!(internal.value, None);
    }

    #[test]
    fn frontend_storage_listing_prefers_namespaced_values_and_includes_known_legacy_values() {
        let items = vec![
            ("connections_table_columns".to_owned(), "legacy".to_owned()),
            (
                "web:connections_table_columns".to_owned(),
                "current".to_owned(),
            ),
            ("dashboard-widget-configs".to_owned(), "widgets".to_owned()),
            ("internal".to_owned(), "preserved".to_owned()),
        ];

        let entries = frontend_storage_entries(&items);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "connections_table_columns");
        assert_eq!(entries[0].value, "current");
        assert_eq!(entries[1].key, "dashboard-widget-configs");
        assert_eq!(entries[1].value, "widgets");
    }

    #[test]
    fn remove_and_clear_delete_legacy_frontend_values_but_preserve_internal_values() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::try_new(&temp.path().join("storage.redb")).unwrap();
        storage
            .set_item("connections_table_columns", &"legacy")
            .unwrap();
        storage
            .set_item("web:connections_table_columns", &"current")
            .unwrap();
        storage
            .set_item("dashboard-widget-configs", &"legacy widgets")
            .unwrap();
        storage.set_item("internal", &"preserved").unwrap();

        remove_web_storage_item(&storage, "connections_table_columns").unwrap();
        assert_eq!(
            storage
                .get_item::<String>("connections_table_columns")
                .unwrap()
                .value,
            None
        );
        assert_eq!(
            get_web_storage_item(&storage, "connections_table_columns")
                .unwrap()
                .value,
            None
        );

        clear_frontend_storage(&storage).unwrap();
        assert_eq!(
            storage
                .get_item::<String>("dashboard-widget-configs")
                .unwrap()
                .value,
            None
        );
        assert_eq!(
            storage
                .get_item::<String>("internal")
                .unwrap()
                .value
                .as_deref(),
            Some("preserved")
        );
    }

    #[test]
    fn missing_profile_view_reports_profile_identity_and_path() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing-profile.yaml");
        let uid = chimera_config::profile::ProfileId("profile-42".into());

        let error = ensure_viewable_profile_file(uid.clone(), &missing).unwrap_err();
        assert!(matches!(
            &error,
            IpcError::ProfileFileMissing { uid: actual, path }
                if actual == &uid && path == &missing
        ));
        assert_eq!(
            error.to_string(),
            format!(
                "the file {} of profile {uid} does not exist",
                missing.display()
            )
        );
        let wire = serde_json::to_value(&error).unwrap();
        let wire = wire.as_str().expect("IPC must serialize errors as strings");
        assert!(wire.contains("profile-42"), "{wire}");
        assert!(wire.contains("missing-profile.yaml"), "{wire}");

        std::fs::write(&missing, "mode: rule\n").unwrap();
        ensure_viewable_profile_file(uid, &missing).unwrap();
    }

    #[test]
    fn pending_deep_links_are_listed_without_claiming() {
        let pending = PendingDeepLink::default();
        let first = pending.store("chimera://install-config?url=https%3A%2F%2Fone.example".into());
        let second = pending.store("chimera://install-config?url=https%3A%2F%2Ftwo.example".into());

        assert_eq!(pending.list_all(), vec![first.clone(), second.clone()]);
        assert_eq!(pending.list_all(), vec![first.clone(), second.clone()]);
        assert!(pending.claim(first.id));
        assert_eq!(pending.list_all(), vec![second]);
    }

    #[test]
    fn pending_deep_link_can_only_be_claimed_once() {
        let pending = PendingDeepLink::default();
        let entry = pending.store("chimera://install-config?url=https%3A%2F%2Fexample.com".into());

        assert!(pending.claim(entry.id));
        assert!(!pending.claim(entry.id));
    }

    #[test]
    fn identical_deep_links_have_independent_claims() {
        let pending = PendingDeepLink::default();
        let url = "chimera://install-config?url=https%3A%2F%2Fexample.com";
        let first = pending.store(url.into());
        let second = pending.store(url.into());

        assert_ne!(first.id, second.id);
        assert!(pending.claim(first.id));
        assert!(!pending.claim(first.id));
        assert!(pending.claim(second.id));
    }

    #[test]
    fn competing_webviews_only_claim_a_deep_link_once() {
        let pending = Arc::new(PendingDeepLink::default());
        let entry = pending.store("chimera://system-proxy?mode=on".into());
        let barrier = Arc::new(Barrier::new(3));

        let claims = (0..2)
            .map(|_| {
                let pending = Arc::clone(&pending);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    pending.claim(entry.id)
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        let successes = claims
            .into_iter()
            .map(|claim| claim.join().unwrap())
            .filter(|claimed| *claimed)
            .count();
        assert_eq!(successes, 1);
    }
}

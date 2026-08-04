use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    result::Result as StdResult,
};

use anyhow::{Context, anyhow, bail};
use specta_typescript::Any;

use chimera_ipc::api::status::CoreState;
use serde_yaml::Mapping;
#[cfg(not(feature = "e2e"))]
use sysproxy::Sysproxy;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::{
    config::{
        chimera::{self, IVerge},
        clash::ClashInfo,
        core::Config,
        profile::{
            builder::ProfileBuilder,
            item::{
                MAX_PROFILE_YAML_BYTES, Profile, ProfileKindGetter, ProfileMetaGetter,
                local::{LocalProfile, LocalProfileBuilder},
                profile_cleanup_path, profile_file_path, profile_materialized_path,
                read_file_bytes_with_limit,
                remote::{
                    RemoteProfile, RemoteProfileBuilder, RemoteProfileOptions,
                    RemoteProfileOptionsBuilder, SubscriptionInfo,
                },
                shared::validate_profile_uid,
                validate_profile_mapping_keys, write_profile_bytes_atomic,
            },
            item_type::{ProfileItemType, ProfileUid},
            profile_mutation_lock,
            profiles::Profiles,
        },
        runtime::{PatchClashCoreConfig, PatchRuntimeConfig},
    },
    core::{
        clash,
        clash::core::{CoreManager, RunType},
        handle,
        storage::{Storage, StorageOperationError, WebStorage},
        updater::{self, ManifestVersionLatest},
    },
    feat,
    transaction::{TransactionOutcome, commit_then_apply_with_rollback},
    utils::{candy, collect::EnvInfo, dirs, help, resolve},
};

type Result<T = ()> = StdResult<T, IpcError>;

#[allow(dead_code)]
#[derive(Debug, specta::Type, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Ok,
    Degraded { error: String },
}

#[derive(specta::Type, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditorWindowType {
    Profile,
    CssEditor,
}

fn deserialize_optional_field<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(<Option<T> as serde::Deserialize>::deserialize(
        deserializer,
    )?))
}

#[derive(specta::Type, serde::Deserialize)]
pub struct ProfileMetadataPatch {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    #[specta(type = Option<Option<String>>)]
    pub desc: Option<Option<String>>,
}

#[derive(specta::Type, serde::Deserialize)]
pub struct RemoteProfileOptionsPatch {
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    #[specta(type = Option<Option<String>>)]
    pub user_agent: Option<Option<String>>,
    pub with_proxy: Option<bool>,
    pub self_proxy: Option<bool>,
    pub update_interval_minutes: Option<u64>,
}

#[derive(specta::Type, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileDefinition {
    Config { config: ConfigDefinition },
}

#[derive(specta::Type, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigDefinition {
    File {
        source: ProfileSource,
        #[serde(default)]
        transforms: Vec<ProfileUid>,
    },
}

#[derive(specta::Type, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSource {
    Remote {
        file: String,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
    },
}

#[derive(specta::Type, serde::Serialize)]
pub struct ProfilesResponse {
    pub current: Option<ProfileUid>,
    pub items: Vec<ProfileResponse>,
    pub valid: Vec<String>,
    pub global_transforms: Vec<ProfileUid>,
}

#[derive(specta::Type, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileResponse {
    Remote {
        #[serde(flatten)]
        profile: RemoteProfile,
    },
    Local {
        #[serde(flatten)]
        profile: LocalProfile,
    },
}

impl From<Profiles> for ProfilesResponse {
    fn from(profiles: Profiles) -> Self {
        let Profiles {
            current,
            items,
            valid,
            chain,
        } = profiles;
        Self {
            current: current.into_iter().next(),
            items: items.into_iter().map(ProfileResponse::from).collect(),
            valid,
            global_transforms: chain,
        }
    }
}

impl From<Profile> for ProfileResponse {
    fn from(profile: Profile) -> Self {
        match profile {
            Profile::Remote(profile) => Self::Remote { profile },
            Profile::Local(profile) => Self::Local { profile },
        }
    }
}

#[derive(specta::Type, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileBuilderRequest {
    Remote {
        #[serde(flatten)]
        profile: RemoteProfileBuilder,
    },
    Local {
        #[serde(flatten)]
        profile: LocalProfileBuilder,
    },
}

impl From<ProfileBuilderRequest> for ProfileBuilder {
    fn from(request: ProfileBuilderRequest) -> Self {
        match request {
            ProfileBuilderRequest::Remote { profile } => Self::Remote(profile),
            ProfileBuilderRequest::Local { profile } => Self::Local(profile),
        }
    }
}

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
    /// first used for open_that
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// first used for read_profile_file
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    Storage(#[from] StorageOperationError),
    #[error("{0}")]
    Custom(String),
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
pub fn get_profiles() -> Result<ProfilesResponse> {
    Ok(Config::profiles().data().clone().into())
}

#[cfg(feature = "e2e")]
fn isolated_sys_proxy_response() -> GetSysProxyResponse {
    GetSysProxyResponse {
        enable: false,
        host: "127.0.0.1".to_string(),
        port: 0,
        bypass: String::new(),
        server: "127.0.0.1:0".to_string(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_sys_proxy() -> Result<GetSysProxyResponse> {
    #[cfg(feature = "e2e")]
    return Ok(isolated_sys_proxy_response());

    #[cfg(not(feature = "e2e"))]
    {
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

fn ensure_remote_profile_url(url: &url::Url) -> anyhow::Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("remote profile URL must use HTTP or HTTPS");
    }
    if url.host_str().is_none() {
        bail!("remote profile URL must include a host");
    }
    Ok(())
}

fn validate_remote_profile_url(url: url::Url) -> Result<url::Url> {
    ensure_remote_profile_url(&url)?;
    Ok(url)
}

fn parse_remote_profile_url(url: &str) -> Result<url::Url> {
    let url = url::Url::parse(url).context("failed to parse the url")?;
    validate_remote_profile_url(url)
}

#[tauri::command]
#[specta::specta]
/// later: check in the frontend
pub async fn import_profile(url: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    let _profile_guard = profile_mutation_lock().lock().await;
    let url = parse_remote_profile_url(&url)?;
    let mut builder = RemoteProfileBuilder::default();
    builder.url(url);
    if let Some(option) = option {
        builder.option(option.clone());
    }
    let (profile, content) = builder
        .build_no_blocking_unpersisted()
        .await
        .context("failed to build a remote profile")?;
    let profile: Profile = profile.into();
    let snapshot = ProfileMaterializationSnapshot::capture(profile.file())?;
    profile.save_file(content)?;
    log::debug!("import_profile 3");
    persist_created_profile(profile, &snapshot, "profile import").await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn view_profile(app_handle: tauri::AppHandle, uid: String) -> Result {
    validate_profile_uid(&uid)?;
    let file = {
        Config::profiles()
            .latest()
            .get_item(&uid)?
            .file()
            .to_string()
    };

    let path = profile_file_path(file)?;
    if !path.exists() {
        return Err(anyhow!("file not exists: {:#?}", path).into());
    }

    help::open_file(app_handle, path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn create_editor_window(
    app_handle: AppHandle,
    window_type: EditorWindowType,
    uid: Option<String>,
) -> Result {
    let uid = match window_type {
        EditorWindowType::Profile => {
            uid.ok_or_else(|| anyhow!("uid required for profile editor"))?
        }
        EditorWindowType::CssEditor => {
            return Err(anyhow!("CSS editor is not supported yet").into());
        }
    };
    validate_profile_uid(&uid)?;

    let (sender, receiver) = tokio::sync::oneshot::channel();
    let handle = app_handle.clone();
    app_handle
        .run_on_main_thread(move || {
            let result = resolve::create_profile_editor_window(&handle, &uid)
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        })
        .context("failed to schedule profile editor window creation")?;

    receiver
        .await
        .context("profile editor window creation was cancelled")?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_verge_config() -> Result<IVerge> {
    Ok(Config::verge().data().clone())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_verge_config(payload: IVerge) -> Result {
    (feat::patch_verge(payload).await)?;
    Ok(())
}

fn update_profiles_draft(update: impl FnOnce(&mut Profiles) -> anyhow::Result<()>) -> Result {
    let result = {
        let mut draft = Config::profiles().draft();
        update(&mut draft)
    };
    if let Err(error) = result {
        Config::profiles().discard();
        return Err(IpcError::from(error));
    }
    Ok(())
}

fn commit_profiles_draft() -> Result {
    Config::profiles().persist_draft_with(Profiles::save_file)?;
    handle::Handle::refresh_profiles();
    Ok(())
}

fn restore_profiles_snapshot(snapshot: Profiles) -> Result {
    Config::profiles()
        .update_and_persist_with(
            move |profiles| {
                *profiles = snapshot;
                Ok::<(), anyhow::Error>(())
            },
            Profiles::save_file,
        )
        .map_err(IpcError::from)?;
    handle::Handle::refresh_profiles();
    Ok(())
}

async fn commit_apply_and_report_profile_change<C, A, AFut, R, RFut>(
    operation: &str,
    commit: C,
    apply: A,
    rollback: R,
) -> Result<RebuildOutcome>
where
    C: FnOnce() -> Result,
    A: FnOnce() -> AFut,
    AFut: Future<Output = Result>,
    R: FnOnce() -> RFut,
    RFut: Future<Output = Result>,
{
    match commit_then_apply_with_rollback(commit, apply, rollback).await? {
        TransactionOutcome::Committed => Ok(RebuildOutcome::Ok),
        TransactionOutcome::RolledBack { primary_error } => Ok(RebuildOutcome::Degraded {
            error: format!(
                "failed to apply {operation}; previous state was restored: {primary_error}"
            ),
        }),
        TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error,
        } => Err(anyhow!(
            "failed to apply {operation}: {primary_error}; rollback also failed: {rollback_error}"
        )
        .into()),
    }
}

async fn restart_core_for_profile_state() -> Result {
    CoreManager::global()
        .restart_core_with_generated_config()
        .await
        .map_err(IpcError::from)?;
    handle::Handle::refresh_clash();
    Ok(())
}

#[cfg(feature = "e2e")]
fn forced_e2e_profile_rebuild_outcome(operation: &str) -> Option<RebuildOutcome> {
    let requested = std::env::var("CHIMERA_E2E_DEGRADED_PROFILE_OPERATION").ok()?;
    if requested != operation {
        return None;
    }

    Config::profiles().discard();
    Some(RebuildOutcome::Degraded {
        error: format!("E2E forced {operation} failure; previous profile state was restored"),
    })
}

async fn commit_profile_draft_then_rebuild(
    previous_profiles: Profiles,
    operation: &str,
) -> Result<RebuildOutcome> {
    #[cfg(feature = "e2e")]
    if let Some(outcome) = forced_e2e_profile_rebuild_outcome(operation) {
        return Ok(outcome);
    }

    let outcome = commit_apply_and_report_profile_change(
        operation,
        commit_profiles_draft,
        restart_core_for_profile_state,
        move || async move {
            restore_profiles_snapshot(previous_profiles)?;
            restart_core_for_profile_state().await
        },
    )
    .await?;

    if matches!(&outcome, RebuildOutcome::Ok) {
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(
            )
            .await;
    }
    Ok(outcome)
}

fn require_profile_change_applied(outcome: RebuildOutcome) -> Result {
    match outcome {
        RebuildOutcome::Ok => Ok(()),
        RebuildOutcome::Degraded { error } => Err(anyhow!(error).into()),
    }
}

async fn commit_then_apply_profile_change<C, A, AFut, R, RFut>(
    commit: C,
    apply: A,
    rollback: R,
) -> Result
where
    C: FnOnce() -> Result,
    A: FnOnce() -> AFut,
    AFut: Future<Output = Result>,
    R: FnOnce() -> RFut,
    RFut: Future<Output = Result>,
{
    match commit_then_apply_with_rollback(commit, apply, rollback).await? {
        TransactionOutcome::Committed => Ok(()),
        TransactionOutcome::RolledBack { primary_error } => Err(anyhow!(
            "failed to apply committed profile change; previous state was restored: {primary_error}"
        )
        .into()),
        TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error,
        } => Err(anyhow!(
            "failed to apply committed profile change: {primary_error}; rollback also failed: {rollback_error}"
        )
        .into()),
    }
}

fn persist_profiles(update: impl FnOnce(&mut Profiles) -> anyhow::Result<()>) -> Result {
    Config::profiles()
        .update_and_persist_with(update, Profiles::save_file)
        .map_err(IpcError::from)?;
    handle::Handle::refresh_profiles();
    Ok(())
}

#[cfg(test)]
fn profile_file_key(file: impl AsRef<Path>) -> String {
    let key = file.as_ref().to_string_lossy();
    #[cfg(target_os = "windows")]
    return key.to_ascii_lowercase();

    #[cfg(not(target_os = "windows"))]
    key.into_owned()
}

fn path_entry_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect profile path {}", path.display()))
            .map_err(IpcError::from),
    }
}

fn remove_profile_path_entry(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::FileTypeExt;

        if file_type.is_symlink_dir() {
            return std::fs::remove_dir(path);
        }
    }

    if file_type.is_symlink() && std::fs::metadata(path).is_ok_and(|target| target.is_dir()) {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[derive(Debug)]
struct ProfileMaterializationSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

impl ProfileMaterializationSnapshot {
    fn capture(file: &str) -> Result<Self> {
        Self::capture_path(profile_materialized_path(file)?)
    }

    fn capture_path(path: PathBuf) -> Result<Self> {
        let content = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => Some(
                read_file_bytes_with_limit(&path, MAX_PROFILE_YAML_BYTES).with_context(|| {
                    format!(
                        "failed to snapshot profile materialization {}",
                        path.display()
                    )
                })?,
            ),
            Ok(_) => {
                return Err(anyhow!(
                    "profile materialized path is not a regular file: {}",
                    path.display()
                )
                .into());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| {
                        format!(
                            "failed to inspect profile materialization {}",
                            path.display()
                        )
                    })
                    .map_err(IpcError::from);
            }
        };
        Ok(Self { path, content })
    }

    fn restore(&self) -> Result {
        match &self.content {
            Some(content) => write_profile_bytes_atomic(&self.path, content)
                .with_context(|| {
                    format!(
                        "failed to restore profile materialization {}",
                        self.path.display()
                    )
                })
                .map_err(IpcError::from),
            None => match std::fs::symlink_metadata(&self.path) {
                Ok(metadata) if metadata.file_type().is_file() => std::fs::remove_file(&self.path)
                    .with_context(|| {
                        format!(
                            "failed to remove new profile materialization {}",
                            self.path.display()
                        )
                    })
                    .map_err(IpcError::from),
                Ok(_) => Err(anyhow!(
                    "refusing to remove non-file profile replacement: {}",
                    self.path.display()
                )
                .into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error)
                    .with_context(|| {
                        format!(
                            "failed to inspect profile materialization rollback path {}",
                            self.path.display()
                        )
                    })
                    .map_err(IpcError::from),
            },
        }
    }
}

fn restore_profile_materialization_after_failed_persistence<T>(
    result: Result<T>,
    snapshot: &ProfileMaterializationSnapshot,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(primary) => match snapshot.restore() {
            Ok(()) => Err(primary),
            Err(rollback) => Err(anyhow!(
                "profile persistence failed: {primary}; materialization rollback also failed: {rollback}"
            )
            .into()),
        },
    }
}

#[cfg(test)]
fn cleanup_profile_file_after_failed_persistence<T>(
    result: Result<T>,
    materialized_path: &Path,
    existed_before: bool,
) -> Result<T> {
    if result.is_err() && !existed_before && path_entry_exists(materialized_path)? {
        remove_profile_path_entry(materialized_path).with_context(|| {
            format!(
                "failed to remove orphaned profile file {}",
                materialized_path.display()
            )
        })?;
    }
    result
}

fn cleanup_deleted_profile_file_after_persistence<T>(
    result: Result<T>,
    materialized_path: Option<&Path>,
) -> Result<T> {
    if let Some(materialized_path) = materialized_path
        && result.is_ok()
        && path_entry_exists(materialized_path)?
    {
        remove_profile_path_entry(materialized_path).with_context(|| {
            format!(
                "profile list was committed but failed to remove materialized file {}",
                materialized_path.display()
            )
        })?;
    }
    result
}

fn stage_created_profile(profiles: &mut Profiles, profile: Profile) -> anyhow::Result<bool> {
    let should_activate = profiles.current.is_empty();
    let uid = profile.uid().to_string();
    profiles.append_item(profile)?;
    if should_activate {
        profiles.activate(Some(&uid))?;
    }
    Ok(should_activate)
}

async fn persist_created_profile(
    profile: Profile,
    snapshot: &ProfileMaterializationSnapshot,
    operation: &str,
) -> Result {
    let previous_profiles = Config::profiles().data().clone();
    let mut should_activate = false;
    update_profiles_draft(|profiles| {
        should_activate = stage_created_profile(profiles, profile)?;
        Ok(())
    })?;

    let result = if should_activate {
        commit_profile_draft_then_rebuild(previous_profiles, operation)
            .await
            .and_then(require_profile_change_applied)
    } else {
        commit_profiles_draft()
    };
    restore_profile_materialization_after_failed_persistence(result, snapshot)
}

fn persist_profile_order(
    update: impl FnOnce(&mut Profiles) -> anyhow::Result<()>,
) -> Result<RebuildOutcome> {
    persist_profiles(update)?;
    Ok(RebuildOutcome::Ok)
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(active_id: ProfileUid, over_id: ProfileUid) -> Result<RebuildOutcome> {
    validate_profile_uid(&active_id)?;
    validate_profile_uid(&over_id)?;
    let _profile_guard = profile_mutation_lock().lock().await;
    persist_profile_order(|profiles| profiles.reorder(&active_id, &over_id))
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(list: Vec<ProfileUid>) -> Result<RebuildOutcome> {
    for uid in &list {
        validate_profile_uid(uid)?;
    }
    let _profile_guard = profile_mutation_lock().lock().await;
    persist_profile_order(|profiles| profiles.reorder_by_list(&list))
}

#[tauri::command]
#[specta::specta]
pub async fn activate_profile(uid: Option<ProfileUid>) -> Result<RebuildOutcome> {
    if let Some(uid) = uid.as_deref() {
        validate_profile_uid(uid)?;
    }
    let _profile_guard = profile_mutation_lock().lock().await;
    let previous_profiles = Config::profiles().data().clone();
    update_profiles_draft(|profiles| profiles.activate(uid.as_deref()))?;
    commit_profile_draft_then_rebuild(previous_profiles, "profile activation").await
}

#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(fields: Vec<String>) -> Result<RebuildOutcome> {
    let _profile_guard = profile_mutation_lock().lock().await;
    let previous_profiles = Config::profiles().data().clone();
    update_profiles_draft(|profiles| {
        profiles.valid = fields;
        Ok(())
    })?;
    commit_profile_draft_then_rebuild(previous_profiles, "profile valid fields update").await
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    uid: ProfileUid,
    patch: ProfileMetadataPatch,
) -> Result<RebuildOutcome> {
    validate_profile_uid(&uid)?;
    let _profile_guard = profile_mutation_lock().lock().await;
    persist_profiles(|profiles| profiles.patch_metadata(&uid, patch.name, patch.desc))?;
    Ok(RebuildOutcome::Ok)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    uid: ProfileUid,
    patch: RemoteProfileOptionsPatch,
) -> Result<RebuildOutcome> {
    validate_profile_uid(&uid)?;
    let _profile_guard = profile_mutation_lock().lock().await;
    persist_profiles(|profiles| {
        profiles.patch_remote_options(
            &uid,
            patch.user_agent,
            patch.with_proxy,
            patch.self_proxy,
            patch.update_interval_minutes,
        )
    })?;
    Ok(RebuildOutcome::Ok)
}

#[tauri::command]
#[specta::specta]
pub async fn replace_profile_definition(
    uid: ProfileUid,
    definition: ProfileDefinition,
) -> Result<RebuildOutcome> {
    validate_profile_uid(&uid)?;
    let _profile_guard = profile_mutation_lock().lock().await;
    let ProfileDefinition::Config {
        config:
            ConfigDefinition::File {
                source:
                    ProfileSource::Remote {
                        file,
                        updated_at,
                        url,
                        option,
                        subscription,
                    },
                transforms,
            },
    } = definition;

    if !transforms.is_empty() {
        return Err(anyhow!("scoped profile transforms are not supported yet").into());
    }
    let url = validate_remote_profile_url(url)?;

    persist_profiles(|profiles| {
        profiles.replace_remote_definition(&uid, &file, updated_at, url, option, subscription)
    })?;
    Ok(RebuildOutcome::Ok)
}

#[tauri::command]
#[specta::specta]
pub fn get_clash_info() -> Result<ClashInfo> {
    Ok(Config::clash().latest().get_client_info())
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
// TODO: a copied from updater metadata, and should be moved a separate updater module
pub struct UpdateWrapper {
    rid: tauri::ResourceId,
    available: bool,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    // TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value).
    #[specta(type = Any)]
    raw_json: serde_json::Value,
}

#[tauri::command]
#[specta::specta]
pub async fn check_update(webview: tauri::Webview) -> Result<Option<UpdateWrapper>> {
    use crate::utils::config::{get_self_proxy, get_system_proxy};
    use std::cmp::Ordering;
    use tauri_plugin_updater::UpdaterExt;

    let build_time = time::OffsetDateTime::parse(
        crate::consts::BUILD_INFO.build_date,
        &time::format_description::well_known::Rfc3339,
    )
    .context("failed to parse build time")?;

    let mut builder = webview
        .updater_builder()
        .version_comparator(move |_, remote| {
            use semver::Version;
            let local = Version::parse(crate::consts::BUILD_INFO.pkg_version).ok();
            log::trace!("[check] local: {:?}, remote: {:?}", local, remote.version);
            match local {
                Some(local) => {
                    if !local.build.is_empty() && !remote.version.build.is_empty() {
                        // ignore build info to compare the version directly
                        match local.cmp_precedence(&remote.version) {
                            Ordering::Less => true,
                            Ordering::Equal => match remote.pub_date {
                                // prefer newer build if pub_date is available
                                Some(pub_date) => {
                                    local.build != remote.version.build && pub_date > build_time
                                }
                                None => local.build != remote.version.build,
                            },
                            Ordering::Greater => false,
                        }
                    } else {
                        local < remote.version
                    }
                }
                None => false,
            }
        });

    // apply proxy
    if let Ok(proxy) = get_self_proxy() {
        builder = builder.proxy(proxy.parse().context("failed to parse proxy")?);
    }

    if let Ok(Some(proxy)) = get_system_proxy() {
        builder = builder.proxy(proxy.parse().context("failed to parse system proxy")?);
    }

    let updater = builder.build().context("failed to build updater")?;
    let update = updater.check().await.context("failed to check update")?;
    Ok(update.map(|u| {
        let mut wrapper = UpdateWrapper {
            available: true,
            current_version: u.current_version.clone(),
            version: u.version.clone(),
            date: u.date.and_then(|d| {
                d.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            }),
            body: u.body.clone(),
            raw_json: u.raw_json.clone(),
            ..Default::default()
        };
        wrapper.rid = webview.resources_table().add(u);
        wrapper
    }))
}

#[tauri::command]
#[specta::specta]
pub fn is_appimage() -> Result<bool> {
    Ok(*crate::consts::IS_APPIMAGE)
}

#[tauri::command]
#[specta::specta]
pub fn open_that(path: String) -> Result {
    (crate::utils::open::that(path))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn cleanup_processes(app_handle: AppHandle) -> Result {
    crate::utils::help::cleanup_processes(&app_handle);
    Ok(())
}

/// Namespace prefix for all frontend-visible KV entries.
/// Internal subsystems (e.g. task storage) use un-prefixed keys and are
/// never exposed to the frontend through these IPC commands.
const WEB_STORAGE_KEY_PREFIX: &str = "web:";
const LEGACY_FRONTEND_STORAGE_KEYS: &[&str] = &[
    "connections_table_columns",
    "custom-css",
    "custom-css-compiled",
    "dashboard-widgets",
];

pub mod service {
    use super::Result;
    use crate::core::service;

    #[tauri::command]
    #[specta::specta]
    pub async fn status_service<'a>() -> Result<chimera_ipc::types::StatusInfo<'a>> {
        Ok(service::control::status().await?)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn install_service() -> Result {
        service::control::install_service().await?;
        Ok(())
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn uninstall_service() -> Result {
        service::control::uninstall_service().await?;
        Ok(())
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn start_service() -> Result {
        let result = service::control::start_service().await;
        let enabled_service = *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false);
        if enabled_service
            && let Err(err) = crate::core::clash::core::CoreManager::global()
                .run_core()
                .await
        {
            log::error!(target: "app", "{err}");
        }
        Ok(result?)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service() -> Result {
        let result = service::control::stop_service().await;
        let enabled_service = *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false);
        if enabled_service
            && let Err(err) = crate::core::clash::core::CoreManager::global()
                .run_core()
                .await
        {
            log::error!(target: "app", "{err}");
        }
        Ok(result?)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn restart_service() -> Result {
        let result = service::control::restart_service().await;
        let enabled_service = *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false);
        if enabled_service
            && let Err(err) = crate::core::clash::core::CoreManager::global()
                .run_core()
                .await
        {
            log::error!(target: "app", "{err}");
        }
        Ok(result?)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_service_install_prompt() -> Result<String> {
    let args = crate::core::service::control::get_service_install_args()
        .await?
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut prompt = format!("./chimera-service {args}");
    if cfg!(not(windows)) {
        prompt = format!("sudo {prompt}");
    }
    Ok(prompt)
}

// #[tracing_attributes::instrument]
// patch clash runtime config
#[tauri::command]
#[specta::specta]
pub async fn patch_clash_config(payload: PatchRuntimeConfig) -> Result {
    tracing::debug!("todo: set for chimera_client core patch_clash_config: {payload:?}");
    let mapping = match serde_yaml::to_value(&payload)? {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Err(IpcError::Custom("Expected a mapping".to_string())),
    };
    if let Err(e) = feat::patch_clash_runtime(mapping).await {
        tracing::error!("{e}");
        return Err(IpcError::from(e));
    }

    feat::update_proxies_buff(None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_clash_core_config(payload: PatchClashCoreConfig) -> Result {
    tracing::debug!("patch clash core config: {payload:?}");
    let mapping = match serde_yaml::to_value(&payload)? {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Err(IpcError::Custom("Expected a mapping".to_string())),
    };

    if let Err(e) = feat::patch_clash(mapping).await {
        tracing::error!("{e}");
        return Err(IpcError::from(e));
    }

    feat::update_proxies_buff(None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_proxies() -> Result<crate::core::clash::proxies::Proxies> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    {
        let guard = ProxiesGuard::global().read();
        if guard.is_updated() {
            return Ok(guard.inner().clone());
        }
    }

    match ProxiesGuard::global().update().await {
        Ok(_) => {
            let proxies = ProxiesGuard::global().read().inner().clone();
            Ok(proxies)
        }
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn select_proxy(group: String, name: String) -> Result<()> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    (ProxiesGuard::global().select_proxy(&group, &name).await)?;
    handle::Handle::mutate_proxies();

    // Interrupt connections based on configuration
    let _ = crate::core::connection_interruption::ConnectionInterruptionService::on_proxy_change()
        .await;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_server_port() -> Result<u16> {
    Ok(*crate::core::server::SERVER_PORT)
}

#[tauri::command]
#[specta::specta]
pub fn get_core_dir() -> Result<String> {
    let core_dir = tauri::utils::platform::current_exe()?;
    let core_dir = core_dir
        .parent()
        .ok_or_else(|| anyhow!("failed to get core dir"))?;
    let core_dir = dunce::canonicalize(core_dir)?;
    Ok(core_dir.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_core_status() -> Result<(CoreState, i64, RunType)> {
    let (state, ts, run_type) = CoreManager::global().status().await;
    Ok((state.into_owned(), ts, run_type))
}

#[tauri::command]
#[specta::specta]
pub async fn url_delay_test(url: &str, expected_status: u16) -> Result<Option<u64>> {
    Ok(crate::utils::net::url_delay_test(url, expected_status).await)
}

#[tauri::command]
#[specta::specta]
pub async fn get_ipsb_asn() -> Result<IpsbResponse> {
    let value = crate::utils::net::get_ipsb_asn().await?;
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
pub async fn collect_logs(app_handle: AppHandle) -> Result {
    let now = chrono::Local::now().format("%Y-%m-%d");
    let fname = format!("{now}-log.zip");
    let Some(path) = app_handle
        .dialog()
        .file()
        .add_filter("archive files", &["zip"])
        .set_file_name(fname)
        .set_title("Save log archive")
        .blocking_save_file()
        .and_then(|path| path.as_path().map(PathBuf::from))
    else {
        return Ok(());
    };

    candy::collect_logs(&path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn collect_envs() -> Result<EnvInfo> {
    Ok(crate::utils::collect::collect_envs())
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
pub async fn update_core(core_type: chimera::ClashCore) -> Result<usize> {
    let event_id = updater::UpdaterManager::global()
        .write()
        .await
        .update_core(&core_type)
        .await?;
    Ok(event_id)
}

#[tauri::command]
#[specta::specta]
pub async fn change_clash_core(clash_core: Option<chimera::ClashCore>) -> Result {
    log::debug!("change_clash_core: {clash_core:?}");
    (feat::change_clash_core(clash_core).await)?;
    Ok(())
}

/// restart the sidecar
#[tauri::command]
#[specta::specta]
pub async fn restart_sidecar() -> Result {
    (CoreManager::global().run_core().await)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_latest_core_versions() -> Result<ManifestVersionLatest> {
    let mut updater = updater::UpdaterManager::global().write().await; // It is intended to block here
    (updater.fetch_latest().await)?;
    // TODO: result key should be kebab-case
    Ok(updater.get_latest_versions())
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_updater(updater_id: usize) -> Result<updater::UpdaterSummary> {
    let updater = updater::UpdaterManager::global()
        .read()
        .await
        .inspect_updater(updater_id)
        .ok_or_else(|| anyhow!("updater not found"))?;
    Ok(updater)
}

#[tauri::command]
#[specta::specta]
pub async fn update_profile(uid: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    validate_profile_uid(&uid)?;
    (feat::update_profile(uid, option).await)?;
    Ok(())
}

fn apply_profile_builder_patch(
    current: &mut Profile,
    profile: ProfileBuilder,
) -> anyhow::Result<()> {
    let original_uid = current.uid().to_string();
    let original_file = current.file().to_string();
    let mut updated = current.clone();

    match (&mut updated, profile) {
        (Profile::Remote(item), ProfileBuilder::Remote(builder)) => builder
            .patch_profile(item)
            .context("failed to patch remote profile"),
        (Profile::Local(item), ProfileBuilder::Local(builder)) => {
            item.apply(builder);
            Ok(())
        }
        _ => Err(anyhow!("profile type mismatch")),
    }?;

    if updated.uid() != original_uid {
        bail!("profile identifier cannot be changed");
    }
    if updated.file() != original_file {
        bail!("profile materialized file cannot be changed");
    }
    if let Profile::Remote(item) = &updated {
        ensure_remote_profile_url(&item.url)?;
    }

    *current = updated;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile(uid: String, profile: ProfileBuilderRequest) -> Result {
    validate_profile_uid(&uid)?;
    let _profile_guard = profile_mutation_lock().lock().await;
    let previous_profiles = Config::profiles().data().clone();
    let profile = ProfileBuilder::from(profile);
    update_profiles_draft(|profiles| {
        let current = profiles
            .items
            .iter_mut()
            .find(|item| item.uid() == uid)
            .ok_or_else(|| anyhow!("failed to get the profile item \"uid:{uid}\""))?;
        apply_profile_builder_patch(current, profile)
    })?;

    require_profile_change_applied(
        commit_profile_draft_then_rebuild(previous_profiles, "profile edit").await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile(uid: String) -> Result {
    validate_profile_uid(&uid)?;
    let _profile_guard = profile_mutation_lock().lock().await;
    let previous_profiles = Config::profiles().data().clone();
    let materialized_path = profile_cleanup_path(previous_profiles.get_item(&uid)?.file())?;
    if materialized_path.is_none() {
        log::warn!(target: "app", "removing profile metadata without touching an invalid historical materialized path");
    }
    let (should_update, _materialized_file) = {
        let mut profiles = Config::profiles().draft();
        profiles.delete_item(&uid)?
    };

    let transaction_result = commit_then_apply_profile_change(
        commit_profiles_draft,
        || async {
            if should_update {
                CoreManager::global()
                    .restart_core_with_generated_config()
                    .await
                    .map_err(IpcError::from)?;
                handle::Handle::refresh_clash();
            }
            Ok(())
        },
        move || async move {
            restore_profiles_snapshot(previous_profiles)?;
            if should_update {
                CoreManager::global()
                    .restart_core_with_generated_config()
                    .await
                    .map_err(IpcError::from)?;
                handle::Handle::refresh_clash();
            }
            Ok(())
        },
    )
    .await;

    cleanup_deleted_profile_file_after_persistence(transaction_result, materialized_path.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn read_profile_file(uid: String) -> Result<String> {
    validate_profile_uid(&uid)?;
    let profiles = Config::profiles();
    let profiles = profiles.latest();
    let item = profiles.get_item(&uid)?;
    let raw = item.read_file()?;
    let data = serde_yaml::from_str::<Mapping>(&raw)?;
    Ok(serde_yaml::to_string(&data).context("failed to convert yaml to string")?)
}

fn validate_profile_yaml_with_limit(file_data: &str, max_bytes: usize) -> Result<()> {
    if file_data.len() > max_bytes {
        return Err(anyhow!("profile YAML exceeds the maximum size of {max_bytes} bytes").into());
    }
    if file_data.trim().is_empty() {
        return Err(anyhow!("profile YAML must not be empty").into());
    }
    let mapping =
        serde_yaml::from_str::<Mapping>(file_data).context("failed to parse profile YAML")?;
    validate_profile_mapping_keys(&mapping).context("invalid profile YAML keys")?;
    Ok(())
}

fn validate_profile_yaml(file_data: &str) -> Result<()> {
    validate_profile_yaml_with_limit(file_data, MAX_PROFILE_YAML_BYTES)
}

fn local_profile_file_data_to_save<'a>(
    is_remote: bool,
    file_data: Option<&'a str>,
) -> Result<Option<&'a str>> {
    if is_remote {
        return Ok(None);
    }
    let Some(file_data) = file_data.filter(|data| !data.is_empty()) else {
        return Ok(None);
    };
    validate_profile_yaml(file_data)?;
    Ok(Some(file_data))
}

#[tauri::command]
#[specta::specta]
pub async fn save_profile_file(uid: String, file_data: String) -> Result {
    validate_profile_uid(&uid)?;
    {
        let profiles = Config::profiles();
        let profiles = profiles.latest();
        let item = profiles.get_item(&uid)?;
        if matches!(item.kind(), ProfileItemType::Remote) {
            return Err(anyhow!("remote profiles are updater-owned").into());
        }
    }
    validate_profile_yaml(&file_data)?;
    feat::save_local_profile_file(uid, file_data).await?;
    Ok(())
}

/// create a new profile
#[tauri::command]
#[specta::specta]
pub async fn create_profile(item: ProfileBuilderRequest, file_data: Option<String>) -> Result {
    let _profile_guard = profile_mutation_lock().lock().await;
    let item = ProfileBuilder::from(item);
    let is_remote = matches!(&item, ProfileBuilder::Remote(_));
    let local_file_data =
        local_profile_file_data_to_save(is_remote, file_data.as_deref())?.map(str::to_owned);

    let (profile, materialized_content): (Profile, Option<String>) = match item {
        ProfileBuilder::Remote(mut builder) => {
            let (profile, content) = builder
                .build_no_blocking_unpersisted()
                .await
                .context("failed to build remote profile")?;
            (profile.into(), Some(content))
        }
        ProfileBuilder::Local(builder) => (
            builder
                .build()
                .context("failed to build local profile")?
                .into(),
            local_file_data,
        ),
    };

    let snapshot = ProfileMaterializationSnapshot::capture(profile.file())?;
    if let Some(content) = materialized_content {
        profile.save_file(content)?;
    }

    persist_created_profile(profile, &snapshot, "profile creation").await?;

    Ok(())
}

fn frontend_storage_key(key: &str) -> String {
    if key.starts_with(WEB_STORAGE_KEY_PREFIX) {
        key.to_string()
    } else {
        format!("{WEB_STORAGE_KEY_PREFIX}{key}")
    }
}

fn is_legacy_frontend_storage_key(key: &str) -> bool {
    LEGACY_FRONTEND_STORAGE_KEYS.contains(&key)
}

fn get_frontend_storage_item(storage: &Storage, key: &str) -> Result<Option<String>> {
    let namespaced_key = frontend_storage_key(key);
    if let Some(value) = storage.get_item(&namespaced_key)? {
        return Ok(Some(value));
    }

    if is_legacy_frontend_storage_key(key) {
        let legacy_value = storage.get_item::<String>(key)?;
        if let Some(value) = legacy_value {
            storage.set_item(&namespaced_key, &value)?;
            storage.remove_item(key)?;
            return Ok(Some(value));
        }
    }

    Ok(None)
}

fn set_frontend_storage_item(storage: &Storage, key: &str, value: &str) -> Result {
    storage.set_item(frontend_storage_key(key), &value)?;
    if is_legacy_frontend_storage_key(key) && storage.get_item::<String>(key)?.is_some() {
        storage.remove_item(key)?;
    }
    Ok(())
}

fn remove_frontend_storage_item(storage: &Storage, key: &str) -> Result {
    storage.remove_item(frontend_storage_key(key))?;
    if is_legacy_frontend_storage_key(key) && storage.get_item::<String>(key)?.is_some() {
        storage.remove_item(key)?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_storage_item(app_handle: AppHandle, key: String) -> Result<Option<String>> {
    get_frontend_storage_item(&app_handle.state::<Storage>(), &key)
}

#[tauri::command]
#[specta::specta]
pub fn set_storage_item(app_handle: AppHandle, key: String, value: String) -> Result {
    set_frontend_storage_item(&app_handle.state::<Storage>(), &key, &value)
}

#[tauri::command]
#[specta::specta]
pub fn remove_storage_item(app_handle: AppHandle, key: String) -> Result {
    remove_frontend_storage_item(&app_handle.state::<Storage>(), &key)
}

#[tauri::command]
#[specta::specta]
pub fn save_window_size_state(app_handle: AppHandle, label: String) -> Result {
    match label.as_str() {
        crate::consts::LEGACY_WINDOW_LABEL => {
            resolve::save_legacy_window_state(&app_handle, true)?;
        }
        crate::consts::MAIN_WINDOW_LABEL => {
            resolve::save_main_window_state(&app_handle, true)?;
        }
        _ => {
            return Err(IpcError::Custom(format!("unknown window label: {label}")));
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn create_main_window(app_handle: AppHandle) -> Result<()> {
    // Spawn window creation to avoid blocking
    std::thread::spawn(move || {
        // Small delay to let the IPC return first
        std::thread::sleep(std::time::Duration::from_millis(10));
        let handle_inner = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            resolve::create_main_window(&handle_inner);
        });
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn create_legacy_window(app_handle: AppHandle) -> Result<()> {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let handle_inner = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            resolve::create_legacy_window(&handle_inner);
        });
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_runtime_yaml() -> Result<String> {
    let runtime = Config::runtime();
    let runtime = runtime.latest();
    let config = runtime.config.as_ref();
    let mapping = (config
        .ok_or(anyhow::anyhow!("failed to parse config to yaml file"))
        .and_then(|config| {
            serde_yaml::to_string(config).context("failed to convert config to yaml")
        }))?;
    Ok(mapping)
}

#[tauri::command]
#[specta::specta]
pub fn open_app_config_dir() -> Result<()> {
    let config_dir = (dirs::app_config_dir())?;
    (crate::utils::open::that(config_dir))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn open_app_data_dir() -> Result<()> {
    let data_dir = (dirs::app_data_dir())?;
    (crate::utils::open::that(data_dir))?;
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
    let log_dir = (dirs::app_logs_dir())?;
    (crate::utils::open::that(log_dir))?;
    Ok(())
}

#[cfg(windows)]
pub mod uwp {
    use super::Result;
    use crate::core::win_uwp;

    #[tauri::command]
    #[specta::specta]
    pub async fn invoke_uwp_tool() -> Result {
        (win_uwp::invoke_uwptools().await)?;
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
fn frontend_storage_entries(storage: &Storage) -> Result<Vec<StorageEntry>> {
    Ok(storage
        .get_all()?
        .into_iter()
        .filter_map(|(raw_key, value)| {
            raw_key
                .strip_prefix(WEB_STORAGE_KEY_PREFIX)
                .map(|key| StorageEntry {
                    key: key.to_string(),
                    value,
                })
        })
        .collect())
}

fn clear_frontend_storage(storage: &Storage) -> Result {
    let mut frontend_keys: Vec<String> = storage
        .get_all()?
        .into_iter()
        .filter(|(key, _)| {
            key.starts_with(WEB_STORAGE_KEY_PREFIX) || is_legacy_frontend_storage_key(key)
        })
        .map(|(key, _)| key)
        .collect();
    frontend_keys.sort();
    storage.remove_items(&frontend_keys)?;
    Ok(())
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[tauri::command]
#[specta::specta]
pub fn get_all_storage_items(app_handle: AppHandle) -> Result<Vec<StorageEntry>> {
    frontend_storage_entries(&app_handle.state::<Storage>())
}

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[tauri::command]
#[specta::specta]
pub fn clear_storage(app_handle: AppHandle) -> Result {
    clear_frontend_storage(&app_handle.state::<Storage>())
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_connections_state(
    app_handle: AppHandle,
) -> Result<crate::core::clash::ws::ClashConnectionsConnectorState> {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    Ok(ws_connector.state())
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    app_handle: AppHandle,
) -> Result<crate::core::clash::ws::ClashWsSnapshot> {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    Ok(ws_connector.snapshot())
}

#[tauri::command]
#[specta::specta]
pub async fn set_clash_ws_recording(
    app_handle: AppHandle,
    kind: crate::core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<crate::core::clash::ws::ClashWsRecording> {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    Ok(ws_connector.set_recording(kind, enabled))
}

#[tauri::command]
#[specta::specta]
pub async fn clear_clash_ws_history(
    app_handle: AppHandle,
    kind: crate::core::clash::ws::ClashWsKind,
) -> Result {
    let ws_connector = app_handle.state::<crate::core::clash::ws::ClashConnectionsConnector>();
    ws_connector.clear_history(kind);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    name: String,
    url: Option<String>,
) -> Result<clash::api::DelayRes> {
    match clash::api::get_proxy_delay(name, url).await {
        Ok(res) => Ok(res),
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    group: String,
    url: Option<String>,
) -> Result<HashMap<String, u32>> {
    Ok(clash::api::get_group_delay(group, url).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_delete_connections(id: Option<String>) -> Result<()> {
    Ok(clash::api::delete_connections(id.as_deref()).await?)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    #[cfg(target_os = "windows")]
    fn create_directory_junction(target: &std::path::Path, junction: &std::path::Path) {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(junction)
            .arg(target)
            .output()
            .expect("failed to invoke mklink for junction fixture");

        assert!(
            output.status.success(),
            "failed to create junction fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(feature = "e2e")]
    use super::isolated_sys_proxy_response;
    use super::{
        ProfileMaterializationSnapshot, activate_profile, apply_profile_builder_patch,
        cleanup_deleted_profile_file_after_persistence,
        cleanup_profile_file_after_failed_persistence, clear_frontend_storage,
        commit_apply_and_report_profile_change, commit_then_apply_profile_change,
        frontend_storage_entries, frontend_storage_key, get_frontend_storage_item,
        local_profile_file_data_to_save, parse_remote_profile_url, path_entry_exists,
        profile_file_key, read_profile_file, remove_frontend_storage_item,
        restore_profile_materialization_after_failed_persistence, set_frontend_storage_item,
        stage_created_profile, update_profile, validate_profile_yaml,
        validate_profile_yaml_with_limit, validate_remote_profile_url,
    };
    use crate::{
        config::profile::{
            builder::ProfileBuilder,
            item::{
                Profile,
                local::{LocalProfile, LocalProfileBuilder},
                remote::{
                    RemoteProfile, RemoteProfileBuilder, RemoteProfileOptions, SubscriptionInfo,
                },
                shared::{ProfileShared, ProfileSharedBuilder},
            },
            profiles::Profiles,
        },
        core::storage::{Storage, WebStorage},
    };

    #[tokio::test]
    async fn profile_ipc_rejects_control_character_uids_before_state_access() {
        let invalid = "profile\nforged-log-entry".to_string();
        let errors = [
            read_profile_file(invalid.clone())
                .expect_err("profile reads must reject invalid identifiers before state access"),
            activate_profile(Some(invalid.clone()))
                .await
                .expect_err("profile activation must reject invalid identifiers before locking"),
            update_profile(invalid, None)
                .await
                .expect_err("profile updates must reject invalid identifiers before leasing"),
        ];

        for error in errors {
            let message = error.to_string();
            assert!(message.contains("control characters"));
            assert!(!message.contains("forged-log-entry"));
        }
    }

    fn local_profile_fixture() -> Profile {
        let mut profile = LocalProfile::builder()
            .build()
            .expect("build local profile fixture");
        profile.shared.uid = "profile-a".into();
        profile.shared.file = "profile-a.yaml".into();
        profile.shared.name = "Original".into();
        Profile::Local(profile)
    }

    fn local_profile_fixture_with_uid(uid: &str) -> Profile {
        let mut profile = local_profile_fixture();
        let Profile::Local(profile) = &mut profile else {
            unreachable!("local profile fixture must remain local");
        };
        profile.shared.uid = uid.into();
        profile.shared.file = format!("{uid}.yaml");
        profile.shared.name = uid.into();
        Profile::Local(profile.clone())
    }

    #[test]
    fn staging_the_first_created_profile_activates_it_atomically() {
        let mut profiles = Profiles::default();
        let profile = local_profile_fixture_with_uid("profile-first");

        let should_activate = stage_created_profile(&mut profiles, profile)
            .expect("first created profile must stage successfully");

        assert!(should_activate);
        assert_eq!(profiles.items.len(), 1);
        assert_eq!(profiles.current, vec!["profile-first"]);
    }

    #[test]
    fn staging_an_additional_profile_preserves_the_active_profile() {
        let mut profiles = Profiles::default();
        profiles
            .append_item(local_profile_fixture_with_uid("profile-active"))
            .expect("append active profile fixture");
        profiles
            .activate(Some("profile-active"))
            .expect("activate profile fixture");

        let should_activate = stage_created_profile(
            &mut profiles,
            local_profile_fixture_with_uid("profile-added"),
        )
        .expect("additional created profile must stage successfully");

        assert!(!should_activate);
        assert_eq!(profiles.items.len(), 2);
        assert_eq!(profiles.current, vec!["profile-active"]);
    }

    #[test]
    fn failed_created_profile_staging_preserves_the_complete_collection() {
        let mut profiles = Profiles::default();
        profiles
            .append_item(local_profile_fixture_with_uid("profile-existing"))
            .expect("append existing profile fixture");
        let before = serde_yaml::to_string(&profiles).expect("serialize profile staging fixture");

        let error = stage_created_profile(
            &mut profiles,
            local_profile_fixture_with_uid("profile-existing"),
        )
        .expect_err("duplicate created profile must be rejected");

        assert!(error.to_string().contains("duplicate"));
        assert_eq!(
            serde_yaml::to_string(&profiles).expect("serialize preserved profile collection"),
            before
        );
    }

    fn local_patch(uid: Option<&str>, file: Option<&str>, name: Option<&str>) -> ProfileBuilder {
        let mut shared = ProfileSharedBuilder::default();
        if let Some(uid) = uid {
            shared.uid(uid.to_string());
        }
        if let Some(file) = file {
            shared.file(file.to_string());
        }
        if let Some(name) = name {
            shared.name(name.to_string());
        }
        let mut builder = LocalProfileBuilder::default();
        builder.shared(shared);
        ProfileBuilder::Local(builder)
    }

    fn remote_profile_fixture() -> Profile {
        Profile::Remote(RemoteProfile {
            url: url::Url::parse("https://example.com/profile.yaml")
                .expect("valid remote fixture URL"),
            option: RemoteProfileOptions::default(),
            shared: ProfileShared {
                uid: "profile-a".into(),
                name: "Remote".into(),
                file: "profile-a.yaml".into(),
                desc: None,
                updated: 0,
            },
            chain: vec![],
            extra: SubscriptionInfo::default(),
        })
    }

    #[test]
    fn profile_patch_rejects_identifier_changes_without_mutating_the_profile() {
        let mut profile = local_profile_fixture();
        let before = serde_yaml::to_string(&profile).unwrap();

        let error = apply_profile_builder_patch(
            &mut profile,
            local_patch(Some("changed-profile"), None, Some("Changed")),
        )
        .expect_err("profile identifier change must be rejected");

        assert!(error.to_string().contains("identifier cannot be changed"));
        assert_eq!(serde_yaml::to_string(&profile).unwrap(), before);
    }

    #[test]
    fn profile_patch_rejects_materialized_file_changes_without_mutation() {
        let mut profile = local_profile_fixture();
        let before = serde_yaml::to_string(&profile).unwrap();

        let error = apply_profile_builder_patch(
            &mut profile,
            local_patch(None, Some("other.yaml"), Some("Changed")),
        )
        .expect_err("profile materialized file change must be rejected");

        assert!(error.to_string().contains("file cannot be changed"));
        assert_eq!(serde_yaml::to_string(&profile).unwrap(), before);
    }

    #[test]
    fn profile_patch_rejects_non_http_remote_urls_without_mutation() {
        let mut profile = remote_profile_fixture();
        let before = serde_yaml::to_string(&profile).unwrap();
        let mut builder = RemoteProfileBuilder::default();
        builder.url(url::Url::parse("file:///C:/profile.yaml").unwrap());

        let error = apply_profile_builder_patch(&mut profile, ProfileBuilder::Remote(builder))
            .expect_err("non-HTTP remote URL patch must be rejected");

        assert!(error.to_string().contains("must use HTTP or HTTPS"));
        assert_eq!(serde_yaml::to_string(&profile).unwrap(), before);
    }

    #[test]
    fn profile_patch_rejects_type_mismatches_without_mutation() {
        let mut profile = local_profile_fixture();
        let before = serde_yaml::to_string(&profile).unwrap();

        let error = apply_profile_builder_patch(
            &mut profile,
            ProfileBuilder::Remote(RemoteProfileBuilder::default()),
        )
        .expect_err("profile type mismatch must be rejected");

        assert!(error.to_string().contains("profile type mismatch"));
        assert_eq!(serde_yaml::to_string(&profile).unwrap(), before);
    }

    #[test]
    fn profile_patch_commits_a_valid_metadata_change_atomically() {
        let mut profile = local_profile_fixture();

        apply_profile_builder_patch(&mut profile, local_patch(None, None, Some("Changed")))
            .expect("valid metadata patch must succeed");

        let Profile::Local(profile) = profile else {
            panic!("local fixture changed profile type");
        };
        assert_eq!(profile.shared.uid, "profile-a");
        assert_eq!(profile.shared.file, "profile-a.yaml");
        assert_eq!(profile.shared.name, "Changed");
    }

    #[test]
    fn profile_path_entry_detection_distinguishes_missing_and_existing_entries() {
        let directory = tempdir().expect("temporary profile path directory");
        let file = directory.path().join("profile.yaml");
        let folder = directory.path().join("folder.yaml");
        let missing = directory.path().join("missing.yaml");
        std::fs::write(&file, "mixed-port: 7890").expect("write profile file fixture");
        std::fs::create_dir(&folder).expect("create profile directory fixture");

        assert!(path_entry_exists(&file).expect("inspect existing profile file"));
        assert!(path_entry_exists(&folder).expect("inspect existing profile directory"));
        assert!(!path_entry_exists(&missing).expect("inspect missing profile path"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_path_entry_detection_counts_a_broken_symlink_as_existing() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("temporary profile symlink directory");
        let link = directory.path().join("profile.yaml");
        symlink_file(directory.path().join("missing-target.yaml"), &link)
            .expect("create broken profile symlink fixture");

        assert!(path_entry_exists(&link).expect("inspect broken profile symlink"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_file_keys_are_case_insensitive_on_windows() {
        assert_eq!(
            profile_file_key("C:/Profiles/Profile.YAML"),
            profile_file_key("c:/profiles/profile.yaml")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn profile_file_keys_preserve_case_on_case_sensitive_platforms() {
        assert_ne!(
            profile_file_key("/profiles/Profile.yaml"),
            profile_file_key("/profiles/profile.yaml")
        );
    }

    #[test]
    fn remote_profile_url_accepts_http_and_https() {
        for value in [
            "http://example.com/profile.yaml",
            "https://example.com/profile.yaml?token=value",
        ] {
            let parsed =
                parse_remote_profile_url(value).expect("HTTP remote profile URL must be accepted");
            assert!(matches!(parsed.scheme(), "http" | "https"));
            assert_eq!(parsed.host_str(), Some("example.com"));
        }
    }

    #[test]
    fn remote_profile_url_rejects_non_network_schemes() {
        for value in [
            "file:///C:/profile.yaml",
            "ftp://example.com/profile.yaml",
            "data:text/yaml,mixed-port%3A7890",
        ] {
            let error = parse_remote_profile_url(value)
                .expect_err("non-HTTP remote profile URL must be rejected");
            assert!(error.to_string().contains("must use HTTP or HTTPS"));
        }
    }

    #[test]
    fn remote_profile_url_rejects_relative_and_malformed_values() {
        for value in ["profile.yaml", "://missing-scheme", "https://"] {
            parse_remote_profile_url(value)
                .expect_err("relative or malformed remote profile URL must be rejected");
        }
    }

    #[test]
    fn validated_remote_profile_url_preserves_the_original_url() {
        let url = url::Url::parse("https://user:pass@example.com/a.yaml#fragment")
            .expect("valid remote URL fixture");
        let expected = url.clone();

        let validated =
            validate_remote_profile_url(url).expect("valid remote profile URL must remain valid");

        assert_eq!(validated, expected);
    }

    #[test]
    fn local_profile_initial_content_accepts_a_yaml_mapping() {
        let yaml = "mixed-port: 7890\nmode: rule\n";

        assert_eq!(
            local_profile_file_data_to_save(false, Some(yaml))
                .expect("valid local profile YAML must be accepted"),
            Some(yaml)
        );
    }

    #[test]
    fn local_profile_initial_content_skips_empty_and_missing_values() {
        assert_eq!(
            local_profile_file_data_to_save(false, None)
                .expect("missing local profile content must be accepted"),
            None
        );
        assert_eq!(
            local_profile_file_data_to_save(false, Some(""))
                .expect("empty local profile content must be accepted"),
            None
        );
    }

    #[test]
    fn remote_profile_initial_content_is_ignored_even_when_invalid() {
        assert_eq!(
            local_profile_file_data_to_save(true, Some("not: [valid"))
                .expect("remote profile materialization is updater-owned"),
            None
        );
    }

    #[test]
    fn profile_yaml_rejects_empty_and_whitespace_only_documents() {
        for yaml in ["", "   \r\n\t"] {
            let error =
                validate_profile_yaml(yaml).expect_err("empty profile YAML must be rejected");
            assert!(error.to_string().contains("must not be empty"));
        }
    }

    #[test]
    fn profile_yaml_rejects_non_mapping_and_malformed_documents() {
        for yaml in [
            "null",
            "- one\n- two\n",
            "plain scalar",
            "not: [valid",
            "first: document\n---\nsecond: document\n",
        ] {
            let error =
                validate_profile_yaml(yaml).expect_err("profile YAML root must be a valid mapping");
            assert!(error.to_string().contains("failed to parse profile YAML"));
        }
    }

    #[test]
    fn profile_yaml_rejects_non_string_and_empty_top_level_keys() {
        for yaml in ["1: value\n", "\"\": value\n", "\"   \": value\n"] {
            let error = validate_profile_yaml(yaml)
                .expect_err("invalid profile YAML top-level key must be rejected");
            assert!(format!("{error:#}").contains("top-level keys"));
        }
    }

    #[test]
    fn profile_yaml_accepts_nonempty_unicode_top_level_keys() {
        validate_profile_yaml("代理: true\n")
            .expect("nonempty Unicode profile YAML key must be accepted");
    }

    #[test]
    fn profile_yaml_size_limit_accepts_exactly_the_limit() {
        let yaml = format!("value: {}", "x".repeat(9));
        assert_eq!(yaml.len(), 16);

        validate_profile_yaml_with_limit(&yaml, 16)
            .expect("profile YAML exactly at the byte limit must be accepted");
    }

    #[test]
    fn profile_yaml_size_limit_rejects_oversized_content_before_parsing() {
        let error = validate_profile_yaml_with_limit("not: [valid and oversized", 8)
            .expect_err("oversized profile YAML must be rejected");

        let message = error.to_string();
        assert!(message.contains("exceeds the maximum size"));
        assert!(!message.contains("failed to parse"));
    }

    #[test]
    fn profile_yaml_size_limit_counts_utf8_bytes_not_characters() {
        let yaml = "name: 测试";
        assert!(yaml.chars().count() < yaml.len());

        validate_profile_yaml_with_limit(yaml, yaml.len())
            .expect("UTF-8 YAML exactly at its byte length must be accepted");
        validate_profile_yaml_with_limit(yaml, yaml.len() - 1)
            .expect_err("UTF-8 YAML over the byte limit must be rejected");
    }

    #[test]
    fn profile_yaml_accepts_an_empty_mapping_and_nested_values() {
        for yaml in ["{}", "dns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\n"] {
            validate_profile_yaml(yaml).expect("valid profile mapping must be accepted");
        }
    }

    #[tokio::test]
    async fn reported_profile_change_returns_degraded_after_successful_rollback() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let commit_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let outcome = commit_apply_and_report_profile_change(
            "profile activation",
            move || {
                commit_events.lock().unwrap().push("commit");
                Ok(())
            },
            move || async move {
                apply_events.lock().unwrap().push("apply");
                Err(super::IpcError::from(anyhow::anyhow!(
                    "injected core apply failure"
                )))
            },
            move || async move {
                rollback_events.lock().unwrap().push("rollback");
                Ok(())
            },
        )
        .await
        .expect("successful rollback must be reported as a degraded outcome");

        let super::RebuildOutcome::Degraded { error } = outcome else {
            panic!("failed apply with successful rollback must be degraded");
        };
        assert!(error.contains("profile activation"));
        assert!(error.contains("injected core apply failure"));
        assert!(error.contains("previous state was restored"));
        assert_eq!(*events.lock().unwrap(), vec!["commit", "apply", "rollback"]);
    }

    #[tokio::test]
    async fn reported_profile_change_does_not_apply_after_commit_failure() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let commit_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let error = commit_apply_and_report_profile_change(
            "profile edit",
            move || {
                commit_events.lock().unwrap().push("commit");
                Err(super::IpcError::from(anyhow::anyhow!(
                    "injected persistence failure"
                )))
            },
            move || async move {
                apply_events.lock().unwrap().push("apply");
                Ok(())
            },
            move || async move {
                rollback_events.lock().unwrap().push("rollback");
                Ok(())
            },
        )
        .await
        .expect_err("commit failure must stop apply and rollback");

        assert!(error.to_string().contains("injected persistence failure"));
        assert_eq!(*events.lock().unwrap(), vec!["commit"]);
    }

    #[tokio::test]
    async fn committed_profile_change_stops_when_persistence_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let commit_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let error = commit_then_apply_profile_change(
            move || {
                commit_events.lock().unwrap().push("commit");
                Err(super::IpcError::from(anyhow::anyhow!(
                    "injected commit failure"
                )))
            },
            move || async move {
                apply_events.lock().unwrap().push("apply");
                Ok(())
            },
            move || async move {
                rollback_events.lock().unwrap().push("rollback");
                Ok(())
            },
        )
        .await
        .expect_err("commit failure must stop the profile change");

        assert!(error.to_string().contains("injected commit failure"));
        assert_eq!(*events.lock().unwrap(), vec!["commit"]);
    }

    #[tokio::test]
    async fn failed_profile_change_application_runs_rollback_after_commit() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let commit_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let error = commit_then_apply_profile_change(
            move || {
                commit_events.lock().unwrap().push("commit");
                Ok(())
            },
            move || async move {
                apply_events.lock().unwrap().push("apply");
                Err(super::IpcError::from(anyhow::anyhow!(
                    "injected core apply failure"
                )))
            },
            move || async move {
                rollback_events.lock().unwrap().push("rollback");
                Ok(())
            },
        )
        .await
        .expect_err("core apply failure must be returned after rollback");

        let message = error.to_string();
        assert!(message.contains("injected core apply failure"));
        assert!(message.contains("previous state was restored"));
        assert_eq!(*events.lock().unwrap(), vec!["commit", "apply", "rollback"]);
    }

    #[tokio::test]
    async fn failed_profile_change_reports_apply_and_rollback_failures() {
        let error = commit_then_apply_profile_change(
            || Ok(()),
            || async {
                Err(super::IpcError::from(anyhow::anyhow!(
                    "injected core apply failure"
                )))
            },
            || async {
                Err(super::IpcError::from(anyhow::anyhow!(
                    "injected rollback failure"
                )))
            },
        )
        .await
        .expect_err("apply and rollback failures must both be returned");

        let message = error.to_string();
        assert!(message.contains("injected core apply failure"));
        assert!(message.contains("injected rollback failure"));
        assert!(message.contains("rollback also failed"));
    }

    #[test]
    fn failed_created_profile_persistence_restores_overwritten_preexisting_content() {
        let directory = tempdir().expect("failed to create profile snapshot directory");
        let materialized = directory.path().join("existing-profile.yaml");
        std::fs::write(&materialized, b"original: true\n")
            .expect("failed to write original profile materialization");
        let snapshot = ProfileMaterializationSnapshot::capture_path(materialized.clone())
            .expect("failed to snapshot original profile materialization");
        std::fs::write(&materialized, b"replacement: true\n")
            .expect("failed to overwrite profile materialization fixture");

        let error = restore_profile_materialization_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile persistence failure"
            ))),
            &snapshot,
        )
        .expect_err("profile persistence failure must be returned after rollback");

        assert!(error.to_string().contains("profile persistence failure"));
        assert_eq!(
            std::fs::read(&materialized)
                .expect("restored profile materialization must remain readable"),
            b"original: true\n"
        );
    }

    #[test]
    fn failed_created_profile_persistence_removes_new_materialization() {
        let directory = tempdir().expect("failed to create profile snapshot directory");
        let materialized = directory.path().join("new-profile.yaml");
        let snapshot = ProfileMaterializationSnapshot::capture_path(materialized.clone())
            .expect("failed to snapshot missing profile materialization");
        std::fs::write(&materialized, b"new: true\n")
            .expect("failed to write new profile materialization fixture");

        restore_profile_materialization_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile persistence failure"
            ))),
            &snapshot,
        )
        .expect_err("profile persistence failure must be returned after cleanup");

        assert!(
            !materialized.exists(),
            "new profile materialization must be removed when metadata persistence fails"
        );
    }

    #[test]
    fn failed_created_profile_rollback_refuses_a_directory_replacement() {
        let directory = tempdir().expect("failed to create profile snapshot directory");
        let materialized = directory.path().join("profile.yaml");
        let snapshot = ProfileMaterializationSnapshot::capture_path(materialized.clone())
            .expect("failed to snapshot missing profile materialization");
        std::fs::create_dir(&materialized)
            .expect("failed to create hostile profile directory replacement");
        let sentinel = materialized.join("sentinel.txt");
        std::fs::write(&sentinel, b"keep")
            .expect("failed to write profile directory replacement sentinel");

        let error = restore_profile_materialization_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile persistence failure"
            ))),
            &snapshot,
        )
        .expect_err("directory replacement must make rollback fail safely");

        let message = error.to_string();
        assert!(message.contains("materialization rollback also failed"));
        assert_eq!(
            std::fs::read(&sentinel).expect("directory replacement sentinel must remain readable"),
            b"keep"
        );
    }

    #[test]
    fn failed_profile_list_persistence_removes_a_new_materialized_file() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("new-profile.yaml");
        std::fs::write(&materialized, "proxies: []")
            .expect("failed to create materialized profile fixture");

        let error = cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("profile list persistence failure must be returned");

        assert!(
            format!("{error:?}").contains("injected profile list persistence failure"),
            "unexpected error: {error:?}"
        );
        assert!(
            !materialized.exists(),
            "new profile materialization must be removed after list persistence fails"
        );
    }

    #[test]
    fn failed_profile_list_persistence_preserves_a_preexisting_file() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("existing-profile.yaml");
        std::fs::write(&materialized, "existing: true")
            .expect("failed to create preexisting profile fixture");

        cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            true,
        )
        .expect_err("profile list persistence failure must be returned");

        assert_eq!(
            std::fs::read_to_string(&materialized)
                .expect("preexisting profile file must remain readable"),
            "existing: true"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_profile_list_persistence_removes_a_new_broken_symlink() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("new-profile.yaml");
        symlink_file(directory.path().join("missing-target.yaml"), &materialized)
            .expect("failed to create broken profile symlink fixture");

        cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("profile list persistence failure must be returned");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "new broken profile symlink must be removed after list persistence fails"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_profile_list_persistence_removes_directory_symlink_without_touching_target() {
        use std::os::windows::fs::symlink_dir;

        let directory = tempdir().expect("failed to create profile temp directory");
        let external = tempdir().expect("failed to create external target directory");
        let sentinel = external.path().join("sentinel.txt");
        std::fs::write(&sentinel, "keep: true")
            .expect("failed to create external directory sentinel");
        let materialized = directory.path().join("new-profile.yaml");
        symlink_dir(external.path(), &materialized)
            .expect("failed to create profile directory symlink fixture");

        cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("profile list persistence failure must be returned");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "orphan cleanup must remove only the directory symlink entry"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel)
                .expect("external directory sentinel must remain readable"),
            "keep: true"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_profile_list_persistence_removes_junction_without_touching_target() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let external = tempdir().expect("failed to create external target directory");
        let sentinel = external.path().join("sentinel.txt");
        std::fs::write(&sentinel, "keep: true")
            .expect("failed to create external junction sentinel");
        let materialized = directory.path().join("new-profile.yaml");
        create_directory_junction(external.path(), &materialized);

        cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("profile list persistence failure must be returned");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "orphan cleanup must remove only the junction entry"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel)
                .expect("external junction sentinel must remain readable"),
            "keep: true"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_profile_list_persistence_removes_a_broken_junction() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let external = tempdir().expect("failed to create external target parent");
        let missing_target = external.path().join("removed-target");
        std::fs::create_dir(&missing_target).expect("failed to create junction target fixture");
        let materialized = directory.path().join("new-profile.yaml");
        create_directory_junction(&missing_target, &materialized);
        std::fs::remove_dir(&missing_target).expect("failed to invalidate junction target");

        cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("profile list persistence failure must be returned");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "orphan cleanup must remove a broken junction entry"
        );
        assert!(
            !missing_target.exists(),
            "orphan cleanup must not recreate or touch the removed junction target"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_profile_list_persistence_removes_a_broken_directory_symlink() {
        use std::os::windows::fs::symlink_dir;

        let directory = tempdir().expect("failed to create profile temp directory");
        let missing_target = directory.path().join("missing-directory");
        let materialized = directory.path().join("new-profile.yaml");
        symlink_dir(&missing_target, &materialized)
            .expect("failed to create broken profile directory symlink fixture");

        cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("profile list persistence failure must be returned");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "orphan cleanup must remove a broken directory symlink entry"
        );
        assert!(
            !missing_target.exists(),
            "orphan cleanup must not create or touch the missing directory target"
        );
    }

    #[test]
    fn failed_profile_list_persistence_does_not_remove_a_replacement_directory() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("new-profile.yaml");
        std::fs::create_dir(&materialized).expect("failed to create replacement directory fixture");
        let sentinel = materialized.join("sentinel.txt");
        std::fs::write(&sentinel, "keep: true")
            .expect("failed to create replacement directory sentinel");

        let error = cleanup_profile_file_after_failed_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile list persistence failure"
            ))),
            &materialized,
            false,
        )
        .expect_err("directory replacement must make orphan cleanup fail safely");

        assert!(
            format!("{error:?}").contains("failed to remove orphaned profile file"),
            "unexpected cleanup error: {error:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel)
                .expect("replacement directory sentinel must remain readable"),
            "keep: true"
        );
    }

    #[test]
    fn successful_profile_list_persistence_keeps_the_materialized_file() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("committed-profile.yaml");
        std::fs::write(&materialized, "committed: true")
            .expect("failed to create committed profile fixture");

        cleanup_profile_file_after_failed_persistence(Ok(()), &materialized, false)
            .expect("successful profile persistence must keep the materialized file");

        assert_eq!(
            std::fs::read_to_string(&materialized)
                .expect("committed profile file must remain readable"),
            "committed: true"
        );
    }

    #[test]
    fn failed_profile_deletion_persistence_keeps_the_materialized_file() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("delete-rollback.yaml");
        std::fs::write(&materialized, "keep: true")
            .expect("failed to create profile deletion fixture");

        cleanup_deleted_profile_file_after_persistence(
            Err::<(), _>(super::IpcError::from(anyhow::anyhow!(
                "injected profile deletion persistence failure"
            ))),
            Some(&materialized),
        )
        .expect_err("profile deletion persistence failure must be returned");

        assert_eq!(
            std::fs::read_to_string(&materialized)
                .expect("failed deletion must preserve the materialized profile"),
            "keep: true"
        );
    }

    #[test]
    fn successful_profile_deletion_persistence_removes_the_materialized_file() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("deleted-profile.yaml");
        std::fs::write(&materialized, "remove: true")
            .expect("failed to create profile deletion fixture");

        cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect("committed profile deletion must remove its materialized file");

        assert!(
            !materialized.exists(),
            "committed profile deletion must clean its materialized file"
        );
    }

    #[test]
    fn successful_profile_deletion_does_not_remove_a_replacement_directory() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("deleted-profile.yaml");
        std::fs::create_dir(&materialized).expect("failed to create replacement directory fixture");
        let sentinel = materialized.join("sentinel.txt");
        std::fs::write(&sentinel, "keep: true")
            .expect("failed to create replacement directory sentinel");

        let error = cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect_err("directory replacement must make committed cleanup fail safely");

        assert!(
            format!("{error:?}")
                .contains("profile list was committed but failed to remove materialized file"),
            "unexpected cleanup error: {error:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel)
                .expect("replacement directory sentinel must remain readable"),
            "keep: true"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn successful_profile_deletion_removes_directory_symlink_without_touching_target() {
        use std::os::windows::fs::symlink_dir;

        let directory = tempdir().expect("failed to create profile temp directory");
        let external = tempdir().expect("failed to create external target directory");
        let sentinel = external.path().join("sentinel.txt");
        std::fs::write(&sentinel, "keep: true")
            .expect("failed to create external directory sentinel");
        let materialized = directory.path().join("deleted-profile.yaml");
        symlink_dir(external.path(), &materialized)
            .expect("failed to create profile directory symlink fixture");

        cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect("committed deletion must remove only the directory symlink entry");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "committed deletion must remove the directory symlink entry"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel)
                .expect("external directory sentinel must remain readable"),
            "keep: true"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn successful_profile_deletion_removes_junction_without_touching_target() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let external = tempdir().expect("failed to create external target directory");
        let sentinel = external.path().join("sentinel.txt");
        std::fs::write(&sentinel, "keep: true")
            .expect("failed to create external junction sentinel");
        let materialized = directory.path().join("deleted-profile.yaml");
        create_directory_junction(external.path(), &materialized);

        cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect("committed deletion must remove only the junction entry");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "committed deletion must remove the junction entry"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel)
                .expect("external junction sentinel must remain readable"),
            "keep: true"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn successful_profile_deletion_removes_a_broken_junction() {
        let directory = tempdir().expect("failed to create profile temp directory");
        let external = tempdir().expect("failed to create external target parent");
        let missing_target = external.path().join("removed-target");
        std::fs::create_dir(&missing_target).expect("failed to create junction target fixture");
        let materialized = directory.path().join("deleted-profile.yaml");
        create_directory_junction(&missing_target, &materialized);
        std::fs::remove_dir(&missing_target).expect("failed to invalidate junction target");

        cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect("committed deletion must remove a broken junction entry");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "committed deletion must not leave a broken junction entry"
        );
        assert!(
            !missing_target.exists(),
            "committed deletion must not recreate or touch the removed junction target"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn successful_profile_deletion_removes_a_broken_directory_symlink() {
        use std::os::windows::fs::symlink_dir;

        let directory = tempdir().expect("failed to create profile temp directory");
        let missing_target = directory.path().join("missing-directory");
        let materialized = directory.path().join("deleted-profile.yaml");
        symlink_dir(&missing_target, &materialized)
            .expect("failed to create broken profile directory symlink fixture");

        cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect("committed deletion must remove a broken directory symlink entry");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "committed deletion must not leave a broken directory symlink entry"
        );
        assert!(
            !missing_target.exists(),
            "committed deletion must not create or touch the missing directory target"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn successful_profile_deletion_removes_a_broken_symlink_entry() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("failed to create profile temp directory");
        let materialized = directory.path().join("deleted-profile.yaml");
        symlink_file(directory.path().join("missing-target.yaml"), &materialized)
            .expect("failed to create broken profile symlink fixture");

        cleanup_deleted_profile_file_after_persistence(Ok(()), Some(&materialized))
            .expect("committed profile deletion must remove a broken symlink entry");

        assert!(
            std::fs::symlink_metadata(&materialized).is_err(),
            "committed profile deletion must not leave a broken symlink entry"
        );
    }

    #[test]
    fn metadata_only_profile_deletion_never_touches_an_invalid_external_path() {
        let directory = tempdir().expect("failed to create external temp directory");
        let external = directory.path().join("outside.yaml");
        std::fs::write(&external, "outside: true")
            .expect("failed to create external profile fixture");

        cleanup_deleted_profile_file_after_persistence(Ok(()), None)
            .expect("metadata-only deletion must succeed without a cleanup target");

        assert_eq!(
            std::fs::read_to_string(&external)
                .expect("metadata-only deletion must preserve the external file"),
            "outside: true"
        );
    }

    #[cfg(feature = "e2e")]
    #[test]
    fn e2e_system_proxy_status_is_disabled_and_deterministic() {
        let response = isolated_sys_proxy_response();

        assert!(!response.enable);
        assert_eq!(response.host, "127.0.0.1");
        assert_eq!(response.port, 0);
        assert!(response.bypass.is_empty());
        assert_eq!(response.server, "127.0.0.1:0");
    }

    #[tokio::test]
    async fn frontend_storage_crud_uses_namespaced_keys_and_notifications() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let storage = Storage::try_new(&directory.path().join("storage.db"))
            .expect("failed to create storage");
        let mut receiver = storage.get_rx();

        set_frontend_storage_item(&storage, "theme", r#""dark""#)
            .expect("failed to write frontend storage value");

        assert_eq!(frontend_storage_key("theme"), "web:theme");
        assert_eq!(frontend_storage_key("web:theme"), "web:theme");
        assert_eq!(
            storage
                .get_item::<String>("web:theme")
                .expect("failed to read namespaced storage value"),
            Some(r#""dark""#.to_string())
        );
        assert_eq!(
            storage
                .get_item::<String>("theme")
                .expect("failed to inspect legacy storage key"),
            None
        );
        assert_eq!(
            get_frontend_storage_item(&storage, "theme")
                .expect("failed to read frontend storage value"),
            Some(r#""dark""#.to_string())
        );

        let written = receiver
            .recv()
            .await
            .expect("storage write notification channel closed");
        assert_eq!(written.0, "web:theme");
        assert_eq!(written.1, Some(br#""\"dark\"""#.to_vec()));

        remove_frontend_storage_item(&storage, "theme")
            .expect("failed to remove frontend storage value");
        assert_eq!(
            get_frontend_storage_item(&storage, "theme")
                .expect("failed to read removed frontend storage value"),
            None
        );

        let removed = receiver
            .recv()
            .await
            .expect("storage remove notification channel closed");
        assert_eq!(removed, ("web:theme".to_string(), None));
    }

    #[test]
    fn frontend_storage_read_migrates_known_legacy_unprefixed_values() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let storage = Storage::try_new(&directory.path().join("storage.db"))
            .expect("failed to create storage");
        storage
            .set_item("dashboard-widgets", &r#"{"layout":"compact"}"#)
            .expect("failed to seed legacy frontend storage value");

        assert_eq!(
            get_frontend_storage_item(&storage, "dashboard-widgets")
                .expect("failed to migrate legacy frontend storage value"),
            Some(r#"{"layout":"compact"}"#.to_string())
        );
        assert_eq!(
            storage
                .get_item::<String>("web:dashboard-widgets")
                .expect("failed to read migrated frontend storage value"),
            Some(r#"{"layout":"compact"}"#.to_string())
        );
        assert_eq!(
            storage
                .get_item::<String>("dashboard-widgets")
                .expect("failed to inspect migrated legacy key"),
            None
        );
    }

    #[test]
    fn frontend_storage_crud_never_reads_or_deletes_unknown_internal_string_keys() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let storage = Storage::try_new(&directory.path().join("storage.db"))
            .expect("failed to create storage");
        storage
            .set_item("task-history", &"internal-state")
            .expect("failed to seed internal storage value");

        assert_eq!(
            get_frontend_storage_item(&storage, "task-history")
                .expect("failed to read isolated frontend storage value"),
            None
        );

        set_frontend_storage_item(&storage, "task-history", r#""frontend-state""#)
            .expect("failed to write namespaced frontend storage value");
        assert_eq!(
            storage
                .get_item::<String>("task-history")
                .expect("failed to inspect internal storage value"),
            Some("internal-state".to_string())
        );
        assert_eq!(
            storage
                .get_item::<String>("web:task-history")
                .expect("failed to inspect namespaced frontend storage value"),
            Some(r#""frontend-state""#.to_string())
        );

        remove_frontend_storage_item(&storage, "task-history")
            .expect("failed to remove namespaced frontend storage value");
        assert_eq!(
            storage
                .get_item::<String>("task-history")
                .expect("failed to verify internal storage preservation"),
            Some("internal-state".to_string())
        );
        assert_eq!(
            storage
                .get_item::<String>("web:task-history")
                .expect("failed to verify frontend storage removal"),
            None
        );
    }

    #[test]
    fn frontend_storage_listing_strips_prefix_and_excludes_internal_entries() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let storage = Storage::try_new(&directory.path().join("storage.db"))
            .expect("failed to create storage");
        storage
            .set_item("web:theme", &"dark")
            .expect("failed to write frontend storage value");
        storage
            .set_item("internal:task-history", &42_u32)
            .expect("failed to write internal storage value");

        let entries =
            frontend_storage_entries(&storage).expect("failed to list frontend storage entries");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "theme");
        assert_eq!(entries[0].value, r#""dark""#);
    }

    #[tokio::test]
    async fn clearing_frontend_storage_preserves_internal_entries_and_notifies_in_order() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let storage = Storage::try_new(&directory.path().join("storage.db"))
            .expect("failed to create storage");
        storage
            .set_item("web:theme", &"dark")
            .expect("failed to write frontend theme");
        storage
            .set_item("web:route", &"/settings")
            .expect("failed to write frontend route");
        storage
            .set_item("dashboard-widgets", &"legacy-layout")
            .expect("failed to write legacy frontend storage value");
        storage
            .set_item("internal:task-history", &42_u32)
            .expect("failed to write internal storage value");
        storage
            .set_item("task-history", &"unknown-internal-value")
            .expect("failed to write unknown internal storage value");
        let mut receiver = storage.get_rx();

        clear_frontend_storage(&storage).expect("failed to clear frontend storage");

        assert!(
            frontend_storage_entries(&storage)
                .expect("failed to list cleared frontend storage")
                .is_empty()
        );
        assert_eq!(
            storage
                .get_item::<String>("dashboard-widgets")
                .expect("failed to inspect cleared legacy frontend storage value"),
            None
        );
        assert_eq!(
            storage
                .get_item::<u32>("internal:task-history")
                .expect("failed to read internal storage value"),
            Some(42)
        );
        assert_eq!(
            storage
                .get_item::<String>("task-history")
                .expect("failed to read unknown internal storage value"),
            Some("unknown-internal-value".to_string())
        );
        assert_eq!(
            receiver
                .recv()
                .await
                .expect("legacy removal notification channel closed"),
            ("dashboard-widgets".to_string(), None)
        );
        assert_eq!(
            receiver
                .recv()
                .await
                .expect("route removal notification channel closed"),
            ("web:route".to_string(), None)
        );
        assert_eq!(
            receiver
                .recv()
                .await
                .expect("theme removal notification channel closed"),
            ("web:theme".to_string(), None)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn cleared_frontend_storage_stays_removed_after_database_reopen() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let storage_path = directory.path().join("storage.db");

        {
            let storage = Storage::try_new(&storage_path).expect("failed to create storage");
            storage
                .set_item("web:theme", &"dark")
                .expect("failed to write frontend theme");
            storage
                .set_item("dashboard-widgets", &"legacy-layout")
                .expect("failed to write legacy frontend storage value");
            storage
                .set_item("internal:task-history", &42_u32)
                .expect("failed to write internal storage value");

            clear_frontend_storage(&storage).expect("failed to clear frontend storage");
        }

        let reopened = Storage::try_new(&storage_path).expect("failed to reopen storage");
        assert!(
            frontend_storage_entries(&reopened)
                .expect("failed to list reopened frontend storage")
                .is_empty()
        );
        assert_eq!(
            reopened
                .get_item::<String>("dashboard-widgets")
                .expect("failed to inspect reopened legacy frontend storage value"),
            None
        );
        assert_eq!(
            reopened
                .get_item::<u32>("internal:task-history")
                .expect("failed to read reopened internal storage value"),
            Some(42)
        );
    }
}

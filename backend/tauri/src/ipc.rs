use std::{collections::HashMap, path::PathBuf, result::Result as StdResult};

use anyhow::{Context, anyhow};
use specta_typescript::Any;

use chimera_ipc::api::status::CoreState;
use sysproxy::Sysproxy;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    config::{
        chimera::{self, IVerge},
        clash::ClashInfo,
        core::Config,
        profile::{
            builder::ProfileBuilder,
            item::{
                Profile,
                local::{LocalProfile, LocalProfileBuilder},
                merge::MergeProfile,
                remote::{
                    RemoteProfile, RemoteProfileBuilder, RemoteProfileOptions,
                    RemoteProfileOptionsBuilder, SubscriptionInfo,
                },
                script::{ScriptProfile, ScriptProfileBuilder},
                shared::ProfileSharedBuilder,
            },
            item_type::{ProfileItemType, ProfileUid, ScriptType},
        },
        runtime::{ClashConfigOverrides, PatchClashCoreConfig, PatchRuntimeConfig},
    },
    core::{
        clash::{
            self,
            client::{MutationOutcome, NyanpasuClient},
            core::RunType,
        },
        handle,
        storage::{Storage, StorageOperationError, WebStorage},
        updater::{self, ManifestVersionLatest},
    },
    feat,
    utils::{candy, collect::EnvInfo, dirs, help, resolve},
};

type Result<T = ()> = StdResult<T, IpcError>;

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
    Merge {
        #[serde(flatten)]
        profile: MergeProfile,
    },
    Script {
        #[serde(flatten)]
        profile: ScriptProfile,
    },
}

impl From<crate::config::profile::profiles::Profiles> for ProfilesResponse {
    fn from(profiles: crate::config::profile::profiles::Profiles) -> Self {
        let crate::config::profile::profiles::Profiles {
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
            Profile::Merge(profile) => Self::Merge { profile },
            Profile::Script(profile) => Self::Script { profile },
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
    Merge {
        name: Option<String>,
        desc: Option<String>,
    },
    Script {
        name: Option<String>,
        desc: Option<String>,
        #[serde(default)]
        script_type: ScriptType,
    },
}

impl From<ProfileBuilderRequest> for ProfileBuilder {
    fn from(request: ProfileBuilderRequest) -> Self {
        match request {
            ProfileBuilderRequest::Remote { profile } => Self::Remote(profile),
            ProfileBuilderRequest::Local { profile } => Self::Local(profile),
            ProfileBuilderRequest::Merge { name, desc } => {
                let mut shared = ProfileSharedBuilder::default();
                if let Some(name) = name {
                    shared.name(name);
                }
                if let Some(desc) = desc {
                    shared.desc(desc);
                }
                let mut builder =
                    crate::config::profile::item::merge::MergeProfileBuilder::default();
                builder.shared(shared);
                Self::Merge(builder)
            }
            ProfileBuilderRequest::Script {
                name,
                desc,
                script_type,
            } => {
                let mut shared = ProfileSharedBuilder::default();
                if let Some(name) = name {
                    shared.name(name);
                }
                if let Some(desc) = desc {
                    shared.desc(desc);
                }
                let mut builder = ScriptProfileBuilder::default();
                builder.shared(shared).script_type(script_type);
                Self::Script(builder)
            }
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
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
pub async fn get_profiles(client: State<'_, NyanpasuClient>) -> Result<ProfilesResponse> {
    Ok(client.get_profiles().await?.into())
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
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsBuilder>,
) -> Result<MutationOutcome<ProfileUid>> {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    let mut builder = RemoteProfileBuilder::default();
    let (uid, prepared_file) = client.reserve_managed_profile_identity(&ProfileItemType::Remote)?;
    builder.assign_managed_identity(uid);
    builder.url(url);
    if let Some(option) = option {
        builder.option(option.clone());
    }
    let prepared = builder
        .build_prepared()
        .await
        .context("failed to build a remote profile")?;
    let (profile, content) = prepared.into_parts();
    Ok(client
        .commit_new_profile(profile.into(), prepared_file, Some(content))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn view_profile(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
    uid: String,
) -> Result {
    let path = client.get_profile_materialized_path(uid).await?;
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
    feat::patch_verge(payload).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profile(
    client: State<'_, NyanpasuClient>,
    active_id: ProfileUid,
    over_id: ProfileUid,
) -> Result<MutationOutcome<()>> {
    Ok(client.reorder_profile(active_id, over_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileUid>,
) -> Result<MutationOutcome<()>> {
    Ok(client.reorder_profiles_by_list(list).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activate_profile(
    client: State<'_, NyanpasuClient>,
    uid: Option<ProfileUid>,
) -> Result<MutationOutcome<()>> {
    Ok(client.activate_profile(uid).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<MutationOutcome<()>> {
    Ok(client.set_profile_valid_fields(fields).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileUid,
    patch: ProfileMetadataPatch,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .patch_profile_metadata(uid, patch.name, patch.desc)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileUid,
    patch: RemoteProfileOptionsPatch,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .patch_remote_profile_options(
            uid,
            patch.user_agent,
            patch.with_proxy,
            patch.self_proxy,
            patch.update_interval_minutes,
        )
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn replace_profile_definition(
    client: State<'_, NyanpasuClient>,
    uid: ProfileUid,
    definition: ProfileDefinition,
) -> Result<MutationOutcome<()>> {
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
    Ok(client
        .replace_remote_profile_definition(uid, file, updated_at, url, option, subscription)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_clash_info() -> Result<ClashInfo> {
    Ok(Config::clash().latest().get_client_info())
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UpdateWrapper {
    rid: tauri::ResourceId,
    available: bool,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
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
                        match local.cmp_precedence(&remote.version) {
                            Ordering::Less => true,
                            Ordering::Equal => match remote.pub_date {
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
    crate::utils::open::that(path)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn cleanup_processes(app_handle: AppHandle) -> Result {
    crate::utils::help::cleanup_processes(&app_handle);
    Ok(())
}

const WEB_STORAGE_KEY_PREFIX: &str = "web:";
fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}

pub mod service {
    use super::{NyanpasuClient, Result, State};
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
    pub async fn start_service(client: State<'_, NyanpasuClient>) -> Result {
        let result = service::control::start_service().await;
        let enabled_service = *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false);
        if enabled_service && let Err(err) = client.rebuild_running_config().await {
            log::error!(target: "app", "{err}");
        }
        Ok(result?)
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn stop_service(client: State<'_, NyanpasuClient>) -> Result {
        let result = service::control::stop_service().await;
        let enabled_service = *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false);
        if enabled_service && let Err(err) = client.rebuild_running_config().await {
            log::error!(target: "app", "{err}");
        }
        Ok(result?)
    }
    #[tauri::command]
    #[specta::specta]
    pub async fn restart_service(client: State<'_, NyanpasuClient>) -> Result {
        let result = service::control::restart_service().await;
        let enabled_service = *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false);
        if enabled_service && let Err(err) = client.rebuild_running_config().await {
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

#[tauri::command]
#[specta::specta]
pub async fn patch_clash_config(
    client: State<'_, NyanpasuClient>,
    payload: PatchRuntimeConfig,
) -> Result {
    tracing::debug!(
        allow_lan = ?payload.allow_lan,
        ipv6 = ?payload.ipv6,
        log_level = ?payload.log_level,
        mode = ?payload.mode,
        "patch clash runtime config"
    );
    let overrides = ClashConfigOverrides::from(payload);
    let outcome = feat::patch_running_clash_overrides(&client, overrides).await;
    match &outcome {
        clash::transaction::TransactionOutcome::Committed => {}
        clash::transaction::TransactionOutcome::Rejected { primary_error } => {
            tracing::warn!(%primary_error, "runtime patch rejected before core mutation");
        }
        clash::transaction::TransactionOutcome::RolledBack { primary_error } => {
            tracing::warn!(%primary_error, "runtime patch failed and core state was restored");
        }
        clash::transaction::TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error,
        } => {
            tracing::error!(%primary_error, %rollback_error, "runtime patch failed and core restoration could not be verified");
        }
    }
    if let Err(error) = outcome.into_result() {
        return Err(IpcError::from(error));
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
        Ok(_) => Ok(ProxiesGuard::global().read().inner().clone()),
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn select_proxy(group: String, name: String) -> Result<()> {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};
    ProxiesGuard::global().select_proxy(&group, &name).await?;
    handle::Handle::mutate_proxies();
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
pub async fn get_core_status(
    client: State<'_, NyanpasuClient>,
) -> Result<(CoreState, i64, RunType)> {
    let status = client.core_status().await?;
    Ok((status.state, status.state_changed_at, status.run_type))
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
pub async fn change_clash_core(
    client: State<'_, NyanpasuClient>,
    clash_core: Option<chimera::ClashCore>,
) -> Result {
    log::debug!("change_clash_core: {clash_core:?}");
    let clash_core = clash_core.ok_or_else(|| anyhow!("clash core is null"))?;
    client.change_core(clash_core).await?;
    Ok(())
}

/// restart the sidecar
#[tauri::command]
#[specta::specta]
pub async fn restart_sidecar(client: State<'_, NyanpasuClient>) -> Result {
    client.rebuild_running_config().await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_latest_core_versions() -> Result<ManifestVersionLatest> {
    let mut updater = updater::UpdaterManager::global().write().await;
    updater.fetch_latest().await?;
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
pub async fn update_profile(
    uid: String,
    option: Option<RemoteProfileOptionsBuilder>,
) -> Result<MutationOutcome<()>> {
    Ok(feat::update_profile(uid, option).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn patch_profile(
    client: State<'_, NyanpasuClient>,
    uid: String,
    profile: ProfileBuilderRequest,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .patch_profile(uid, ProfileBuilder::from(profile))
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_profile(
    client: State<'_, NyanpasuClient>,
    uid: String,
) -> Result<MutationOutcome<()>> {
    Ok(client.delete_profile(uid).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn read_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileUid,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn save_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileUid,
    file_data: String,
) -> Result<MutationOutcome<()>> {
    Ok(client.save_profile_file(uid, file_data).await?)
}

/// create a new profile
#[tauri::command]
#[specta::specta]
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    item: ProfileBuilderRequest,
    file_data: Option<String>,
) -> Result<MutationOutcome<ProfileUid>> {
    let mut item = ProfileBuilder::from(item);
    let kind = item.kind();
    let (uid, prepared_file) = client.reserve_managed_profile_identity(&kind)?;
    item.assign_managed_identity(uid);
    let (profile, materialized_content): (Profile, Option<String>) = match item {
        ProfileBuilder::Remote(mut builder) => {
            let prepared = builder
                .build_prepared()
                .await
                .context("failed to build remote profile")?;
            let (profile, content) = prepared.into_parts();
            (profile.into(), Some(content))
        }
        ProfileBuilder::Local(builder) => (
            builder
                .build()
                .context("failed to build local profile")?
                .into(),
            file_data.filter(|data| !data.is_empty()),
        ),
        ProfileBuilder::Merge(builder) => {
            let content = file_data
                .filter(|data| !data.is_empty())
                .ok_or_else(|| anyhow!("merge profile content cannot be empty"))?;
            (
                builder
                    .build()
                    .context("failed to build merge profile")?
                    .into(),
                Some(content),
            )
        }
        ProfileBuilder::Script(builder) => {
            let content = file_data
                .filter(|data| !data.is_empty())
                .ok_or_else(|| anyhow!("script profile content cannot be empty"))?;
            (
                builder
                    .build()
                    .context("failed to build script profile")?
                    .into(),
                Some(content),
            )
        }
    };
    Ok(client
        .commit_new_profile(profile, prepared_file, materialized_content)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn get_storage_item(app_handle: AppHandle, key: String) -> Result<Option<String>> {
    let storage = app_handle.state::<Storage>();
    let namespaced_key = web_key(&key);
    if let Some(value) = storage.get_item(&namespaced_key)? {
        return Ok(Some(value));
    }
    Ok(storage.get_item(&key)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_storage_item(app_handle: AppHandle, key: String, value: String) -> Result {
    app_handle
        .state::<Storage>()
        .set_item(web_key(&key), &value)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn remove_storage_item(app_handle: AppHandle, key: String) -> Result {
    app_handle.state::<Storage>().remove_item(web_key(&key))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn save_window_size_state(app_handle: AppHandle, label: String) -> Result {
    match label.as_str() {
        crate::consts::LEGACY_WINDOW_LABEL => resolve::save_legacy_window_state(&app_handle, true)?,
        crate::consts::MAIN_WINDOW_LABEL => resolve::save_main_window_state(&app_handle, true)?,
        _ => return Err(IpcError::Custom(format!("unknown window label: {label}"))),
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn create_main_window(app_handle: AppHandle) -> Result<()> {
    std::thread::spawn(move || {
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
    Ok(config
        .ok_or(anyhow!("failed to parse config to yaml file"))
        .and_then(|config| {
            serde_yaml::to_string(config).context("failed to convert config to yaml")
        })?)
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[tauri::command]
#[specta::specta]
pub fn get_all_storage_items(app_handle: AppHandle) -> Result<Vec<StorageEntry>> {
    let storage = app_handle.state::<Storage>();
    let items = storage.get_all()?;
    Ok(items
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

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[tauri::command]
#[specta::specta]
pub fn clear_storage(app_handle: AppHandle) -> Result {
    let storage = app_handle.state::<Storage>();
    let web_keys: Vec<String> = storage
        .get_all()?
        .into_iter()
        .filter(|(k, _)| k.starts_with(WEB_STORAGE_KEY_PREFIX))
        .map(|(k, _)| k)
        .collect();
    for key in web_keys {
        storage.remove_item(&key)?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_connections_state(
    app_handle: AppHandle,
) -> Result<crate::core::clash::ws::ClashConnectionsConnectorState> {
    Ok(app_handle
        .state::<crate::core::clash::ws::ClashConnectionsConnector>()
        .state())
}

#[tauri::command]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    app_handle: AppHandle,
) -> Result<crate::core::clash::ws::ClashWsSnapshot> {
    Ok(app_handle
        .state::<crate::core::clash::ws::ClashConnectionsConnector>()
        .snapshot())
}

#[tauri::command]
#[specta::specta]
pub async fn set_clash_ws_recording(
    app_handle: AppHandle,
    kind: crate::core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<crate::core::clash::ws::ClashWsRecording> {
    Ok(app_handle
        .state::<crate::core::clash::ws::ClashConnectionsConnector>()
        .set_recording(kind, enabled))
}

#[tauri::command]
#[specta::specta]
pub async fn clear_clash_ws_history(
    app_handle: AppHandle,
    kind: crate::core::clash::ws::ClashWsKind,
) -> Result {
    app_handle
        .state::<crate::core::clash::ws::ClashConnectionsConnector>()
        .clear_history(kind);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn clash_api_get_configs() -> Result<clash::api::ClashRuntimeConfig> {
    Ok(clash::api::get_configs().await?)
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

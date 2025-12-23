use std::result::Result as StdResult;

use anyhow::{Context, anyhow};
use tauri::{AppHandle, Manager};

use crate::{
    config::{
        chimera::{self, IVerge},
        clash::ClashInfo,
        core::Config,
        profile::{
            item::{
                ProfileMetaGetter,
                remote::{RemoteProfileBuilder, RemoteProfileOptionsBuilder},
            },
            profiles::{Profiles, ProfilesBuilder},
        },
        runtime::PatchRuntimeConfig,
    },
    core::{clash::core::CoreManager, handle, updater::ManifestVersionLatest},
    feat,
    utils::{dirs, help, resolve},
};

type Result<T = ()> = StdResult<T, IpcError>;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    /// first used for open_that
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
    fn inline(
        type_map: &mut specta::TypeMap,
        generics: specta::Generics,
    ) -> specta::datatype::DataType {
        specta::datatype::DataType::Primitive(specta::datatype::PrimitiveType::String)
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_profiles() -> Result<Profiles> {
    Ok(Config::profiles().data().clone())
}

#[tauri::command]
#[specta::specta]
/// later: check in the frontend
pub async fn import_profile(url: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    let mut builder = RemoteProfileBuilder::default();
    builder.url(url);
    if let Some(option) = option {
        builder.option(option.clone());
    }
    let profile = builder
        .build_no_blocking()
        .await
        .context("failed to build a remote profile")?;
    log::debug!("import_profile 3");
    // 根据是否为 Some(uid) 来判断是否要激活配置
    let profile_id = {
        if Config::profiles().draft().current.is_empty() {
            Some(profile.uid().to_string())
        } else {
            None
        }
    };
    {
        let committer = Config::profiles().auto_commit();
        (committer.draft().append_item(profile.into()))?;
    }
    // TODO: 使用 activate_profile 来激活配置
    if let Some(profile_id) = profile_id {
        let mut builder = ProfilesBuilder::default();
        builder.current(vec![profile_id]);
        patch_profiles_config(builder).await?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn view_profile(app_handle: tauri::AppHandle, uid: String) -> Result {
    let file = {
        Config::profiles()
            .latest()
            .get_item(&uid)?
            .file()
            .to_string()
    };

    let path = (dirs::app_profiles_dir())?.join(file);
    if !path.exists() {
        return Err(anyhow!("file not exists: {:#?}", path).into());
    }

    help::open_file(app_handle, path)?;
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

/// 修改profiles的
#[tauri::command]
#[specta::specta]
pub async fn patch_profiles_config(profiles: ProfilesBuilder) -> Result {
    Config::profiles().draft().apply(profiles);

    match CoreManager::global().update_config().await {
        Ok(_) => {
            handle::Handle::refresh_clash();
            Config::profiles().apply();
            (Config::profiles().data().save_file())?;

            // Interrupt connections based on configuration
            let _ = crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change().await;

            Ok(())
        }
        Err(err) => {
            log::debug!(target: "app", "{err:?}");

            Config::profiles().discard();

            Err(IpcError::from(err))
        }
    }
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

// #[tracing_attributes::instrument]
// patch clash runtime config
#[tauri::command]
#[specta::specta]
pub async fn patch_clash_config(payload: PatchRuntimeConfig) -> Result {
    tracing::debug!("patch_clash_config: {payload:?}");
    todo!("implement the patch_clash_config ipc command");
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
pub async fn update_core(core_type: chimera::ClashCore) -> Result<usize> {
    log::warn!("update_core: {core_type:?}");
    todo!()
}

#[tauri::command]
#[specta::specta]
pub async fn change_clash_core(clash_core: Option<chimera::ClashCore>) -> Result {
    todo!()
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
    todo!()
}

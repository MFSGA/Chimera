use std::borrow::Borrow;

use anyhow::{Result, bail};
use chimera_ipc::api::status::CoreState;
use serde_yaml::Mapping;
use tauri::{AppHandle, Manager};
use tracing::debug;

use crate::{
    config::{
        chimera::IVerge, core::Config, profile::item::remote::RemoteProfileOptionsBuilder,
        runtime::ClashConfigOverrides,
    },
    core::{
        clash::{client::NyanpasuClient, transaction::TransactionOutcome},
        handle,
        service::ipc::get_ipc_state,
        sysopt,
    },
    log_err,
    utils::{self, help::get_clash_external_port},
};
use handle::Message;

struct ClashPatchPlan {
    mixed_port: Option<u16>,
    mixed_port_changed: bool,
    external_controller: Option<String>,
    external_controller_changed: bool,
    mode_changed: bool,
    requires_restart: bool,
}

fn get_non_null_patch_value<'a>(patch: &'a Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    patch.get(key).filter(|value| !value.is_null())
}

// Build a normalized view of the clash patch before validation and side effects.
fn plan_clash_patch(patch: &Mapping) -> Result<ClashPatchPlan> {
    let mixed_port = get_non_null_patch_value(patch, "mixed-port").and_then(|value| value.as_u64());
    let mixed_port = mixed_port
        .map(|port| u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port")))
        .transpose()?;
    let mixed_port_changed = mixed_port
        .map(|port| {
            port != Config::verge()
                .latest()
                .verge_mixed_port
                .unwrap_or(Config::clash().data().get_mixed_port())
        })
        .unwrap_or(false);

    let external_controller = get_non_null_patch_value(patch, "external-controller")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("external-controller must be a string"))
        })
        .transpose()?;
    let external_controller_changed = external_controller
        .as_ref()
        .map(|controller| controller != &Config::clash().data().get_client_info().server)
        .unwrap_or(false);

    Ok(ClashPatchPlan {
        mixed_port,
        mixed_port_changed,
        external_controller,
        external_controller_changed,
        mode_changed: get_non_null_patch_value(patch, "mode").is_some(),
        requires_restart: get_non_null_patch_value(patch, "mixed-port").is_some()
            || get_non_null_patch_value(patch, "secret").is_some()
            || get_non_null_patch_value(patch, "external-controller").is_some(),
    })
}

fn validate_mixed_port_change(plan: &ClashPatchPlan) -> Result<()> {
    let enable_random_port = Config::verge().latest().enable_random_port.unwrap_or(false);

    if plan.mixed_port_changed
        && !enable_random_port
        && let Some(port) = plan.mixed_port
        && !port_scanner::local_port_available(port)
    {
        bail!("port already in use");
    }

    Ok(())
}

async fn validate_external_controller_change(
    client: &NyanpasuClient,
    plan: &ClashPatchPlan,
) -> Result<()> {
    if !plan.external_controller_changed {
        return Ok(());
    }

    let external_controller = plan
        .external_controller
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing external-controller"))?;
    let (_, port) = external_controller
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("external-controller must be host:port"))?;
    let port = port.parse::<u16>()?;
    let strategy = Config::verge()
        .latest()
        .get_external_controller_port_strategy();
    let core_state = client.core_status().await?;

    if matches!(&core_state.state, CoreState::Running)
        && get_clash_external_port(&strategy, port).is_err()
    {
        bail!("can not select fixed: current port is not available.");
    }

    Ok(())
}

async fn apply_clash_runtime_change(client: &NyanpasuClient, plan: &ClashPatchPlan) -> Result<()> {
    if !plan.requires_restart {
        return Ok(());
    }

    client.rebuild_running_config().await?;
    Ok(())
}

fn run_clash_patch_side_effects(plan: &ClashPatchPlan) {
    if plan.mixed_port.is_some() {
        log_err!(sysopt::Sysopt::global().init_sysproxy());
    }

    if plan.mode_changed {
        crate::feat::update_proxies_buff(None);
        debug!("systray mode changed, update proxies buff");
        log_err!(handle::Handle::update_systray_part());
    }
}

struct VergePatchPlan {
    service_mode: Option<bool>,
    tun_mode: Option<bool>,
    auto_launch_changed: bool,
    system_proxy_changed: bool,
    proxy_bypass_changed: bool,
    enable_proxy_guard: bool,
    log_level_changed: bool,
    log_max_files_changed: bool,
    refresh_systray: bool,
}

// Build a normalized view of the verge patch before runtime changes and side effects.
fn plan_verge_patch(patch: &IVerge) -> Result<VergePatchPlan> {
    if let Some(ref theme_color) = patch.theme_color
        && !theme_color.is_empty()
        && !crate::config::chimera::is_hex_color(theme_color)
    {
        bail!("Invalid theme color: {}", theme_color);
    }

    Ok(VergePatchPlan {
        service_mode: patch.enable_service_mode,
        tun_mode: patch.enable_tun_mode,
        auto_launch_changed: patch.enable_auto_launch.is_some(),
        system_proxy_changed: patch.enable_system_proxy.is_some(),
        proxy_bypass_changed: patch.system_proxy_bypass.is_some(),
        enable_proxy_guard: patch.enable_proxy_guard == Some(true),
        log_level_changed: patch.app_log_level.is_some(),
        log_max_files_changed: patch.max_log_files.is_some(),
        refresh_systray: patch.enable_system_proxy.is_some() || patch.enable_tun_mode.is_some(),
    })
}

async fn apply_verge_runtime_change(client: &NyanpasuClient, plan: &VergePatchPlan) -> Result<()> {
    let ipc_state = get_ipc_state();

    if let Some(service_mode) = plan.service_mode
        && ipc_state.is_connected()
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
        client.rebuild_running_config().await?;
    }

    if plan.tun_mode.is_some() {
        log::debug!(target: "app", "toggle tun mode");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            use crate::utils::dirs::check_core_permission;
            let current_core = Config::verge().data().clash_core.unwrap_or_default();
            let current_core: chimera_utils::core::CoreType = (&current_core).into();
            let service_state = crate::core::service::ipc::get_ipc_state();
            if !service_state.is_connected() && check_core_permission(&current_core).inspect_err(|e| {
                log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
            }).is_ok_and(|v| !v) {
                log::debug!(target: "app", "clash core permission is missing, tun toggle will restart core and may still fail");
            };
        }
        update_core_config(client).await?;
    }

    Ok(())
}

fn run_verge_patch_side_effects(plan: &VergePatchPlan, patch: &IVerge) -> Result<()> {
    if plan.auto_launch_changed {
        sysopt::Sysopt::global().update_launch()?;
    }

    if plan.system_proxy_changed || plan.proxy_bypass_changed {
        sysopt::Sysopt::global().update_sysproxy()?;
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.enable_proxy_guard {
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.log_level_changed || plan.log_max_files_changed {
        utils::init::refresh_logger((patch.app_log_level.clone(), patch.max_log_files))?;
    }

    if plan.refresh_systray {
        handle::Handle::update_systray_part()?;
    }

    debug!("todo: handle other fields");

    Ok(())
}

/// Persists a typed set of runtime overrides without conflating it with a
/// running-core snapshot.
pub async fn patch_clash_overrides(
    client: &NyanpasuClient,
    overrides: ClashConfigOverrides,
) -> Result<()> {
    let patch = overrides.to_mapping();
    patch_clash_with_overrides(client, patch, overrides).await
}

/// Applies typed overrides to the running core and desired state through the
/// shared transaction coordinator used by IPC and non-window entry points.
pub async fn patch_running_clash_overrides(
    client: &NyanpasuClient,
    overrides: ClashConfigOverrides,
) -> TransactionOutcome {
    client.patch_running_clash_overrides(overrides).await
}

/// Applies a general Clash mapping while extracting only supported persistent
/// runtime overrides for the generated config.
pub async fn patch_clash(client: &NyanpasuClient, patch: Mapping) -> Result<()> {
    let overrides = ClashConfigOverrides::from_mapping(&patch)?;
    patch_clash_with_overrides(client, patch, overrides).await
}

async fn patch_clash_with_overrides(
    client: &NyanpasuClient,
    patch: Mapping,
    overrides: ClashConfigOverrides,
) -> Result<()> {
    Config::clash().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_clash_patch(&patch)?;
        validate_mixed_port_change(&plan)?;
        validate_external_controller_change(client, &plan).await?;
        apply_clash_runtime_change(client, &plan).await?;
        run_clash_patch_side_effects(&plan);
        Config::runtime().draft().patch_config(&overrides);
        Ok(plan)
    }
    .await;

    match result {
        Ok(plan) => {
            Config::clash().apply();
            Config::runtime().apply();
            Config::clash().data().save_config()?;
            if plan.mode_changed {
                log_err!(
                    crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change()
                        .await,
                    "failed to interrupt connections after mode change"
                );
            }
            Ok(())
        }
        Err(err) => {
            Config::clash().discard();
            Config::runtime().discard();
            Err(err)
        }
    }
}

fn managed_client() -> Result<NyanpasuClient> {
    let app_handle = handle::Handle::app_handle()
        .ok_or_else(|| anyhow::anyhow!("app handle is not initialized"))?;
    app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| anyhow::anyhow!("nyanpasu client is not managed"))
}

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    managed_client()?.patch_verge(patch).await
}

pub(crate) async fn patch_verge_uncoordinated(
    client: &NyanpasuClient,
    patch: IVerge,
) -> Result<()> {
    Config::verge().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_verge_patch(&patch)?;
        apply_verge_runtime_change(client, &plan).await?;
        run_verge_patch_side_effects(&plan, &patch)?;
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            Config::verge().apply();
            Config::verge().data().save_file()?;
            handle::Handle::refresh_verge();
            Ok(())
        }
        Err(err) => {
            Config::verge().discard();
            Err(err)
        }
    }
}

/// 更新配置
async fn update_core_config(client: &NyanpasuClient) -> Result<()> {
    match client.rebuild_running_config().await {
        Ok(_) => {
            handle::Handle::notice_message(&Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

/// 更新某个profile
/// 如果更新当前配置就激活配置
pub async fn update_profile<T: Borrow<String>>(
    uid: T,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<crate::core::clash::client::MutationOutcome<()>> {
    managed_client()?
        .refresh_profile(uid.borrow().clone(), opts)
        .await
}

pub fn update_proxies_buff(rx: Option<tokio::sync::oneshot::Receiver<()>>) {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};

    tauri::async_runtime::spawn(async move {
        if let Some(rx) = rx
            && let Err(e) = rx.await
        {
            log::error!(target: "app::clash::proxies", "update proxies buff by rx failed: {e}");
        }
        match ProxiesGuard::global().update().await {
            Ok(_) => {
                log::debug!(target: "app::clash::proxies", "update proxies buff success");
                handle::Handle::mutate_proxies();
            }
            Err(e) => {
                log::error!(target: "app::clash::proxies", "update proxies buff failed: {e}");
            }
        }
    });
}

pub fn change_clash_mode(app_handle: &AppHandle, mode: String) {
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let Some(client) = app_handle.try_state::<NyanpasuClient>() else {
            log::error!(target: "app", "nyanpasu client is not managed");
            return;
        };
        let overrides = ClashConfigOverrides {
            mode: Some(mode),
            ..ClashConfigOverrides::default()
        };

        if let Err(error) = patch_running_clash_overrides(&client, overrides)
            .await
            .into_result()
        {
            log::error!(target: "app", "failed to change clash mode transactionally: {error:#}");
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMode {
    Rule,
    Global,
    Direct,
}

impl RoutingMode {
    const fn as_core_value(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }
}

fn routing_mode_patch(mode: RoutingMode) -> Mapping {
    let mut patch = Mapping::new();
    patch.insert("mode".into(), mode.as_core_value().into());
    patch
}

/// Apply an explicit routing target through the shared runtime transaction path.
#[cfg(feature = "agent")]
pub async fn set_routing_mode(mode: RoutingMode) -> Result<()> {
    let client = managed_client()?;
    let overrides = ClashConfigOverrides::from_mapping(&routing_mode_patch(mode))?;
    patch_running_clash_overrides(&client, overrides)
        .await
        .into_result()
}

#[cfg(any(feature = "agent", test))]
fn service_mode_patch(enabled: bool) -> IVerge {
    IVerge {
        enable_service_mode: Some(enabled),
        ..IVerge::default()
    }
}

#[cfg(feature = "agent")]
pub async fn set_service_mode(enabled: bool) -> Result<()> {
    let current = Config::verge()
        .latest()
        .enable_service_mode
        .unwrap_or(false);
    if current == enabled {
        return Ok(());
    }
    patch_verge(service_mode_patch(enabled)).await
}

#[cfg(feature = "agent")]
pub(crate) async fn restore_service_mode(enabled: bool) -> Result<()> {
    patch_verge(service_mode_patch(enabled)).await
}

fn system_proxy_patch(enabled: bool) -> IVerge {
    IVerge {
        enable_system_proxy: Some(enabled),
        ..IVerge::default()
    }
}

#[cfg(feature = "agent")]
pub async fn set_system_proxy_enabled(enabled: bool) -> Result<()> {
    patch_verge(system_proxy_patch(enabled)).await
}

fn tun_mode_patch(enabled: bool) -> IVerge {
    IVerge {
        enable_tun_mode: Some(enabled),
        ..IVerge::default()
    }
}

fn tun_target_requires_apply(current: bool, target: bool) -> bool {
    current != target
}

#[cfg(feature = "agent")]
pub async fn set_tun_enabled(enabled: bool) -> Result<()> {
    let current = Config::verge().latest().enable_tun_mode.unwrap_or(false);
    if !tun_target_requires_apply(current, enabled) {
        return Ok(());
    }
    patch_verge(tun_mode_patch(enabled)).await
}

#[cfg(feature = "agent")]
pub fn enable_system_proxy() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_system_proxy_enabled(true).await {
            log::error!(target: "app", "failed to enable system proxy: {err:?}");
        }
    });
}

#[cfg(feature = "agent")]
pub fn disable_system_proxy() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_system_proxy_enabled(false).await {
            log::error!(target: "app", "failed to disable system proxy: {err:?}");
        }
    });
}

#[cfg(feature = "agent")]
pub fn enable_tun_mode() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_tun_enabled(true).await {
            log::error!(target: "app", "failed to enable tun mode: {err:?}");
        }
    });
}

#[cfg(feature = "agent")]
pub fn disable_tun_mode() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_tun_enabled(false).await {
            log::error!(target: "app", "failed to disable tun mode: {err:?}");
        }
    });
}

pub fn toggle_system_proxy() {
    let enabled = Config::verge()
        .latest()
        .enable_system_proxy
        .unwrap_or(false);
    tauri::async_runtime::spawn(async move {
        let patch = IVerge {
            enable_system_proxy: Some(!enabled),
            ..IVerge::default()
        };
        if let Err(err) = patch_verge(patch).await {
            log::error!(target: "app", "failed to toggle system proxy: {err:?}");
        }
    });
}

pub fn toggle_tun_mode() {
    let enabled = Config::verge().latest().enable_tun_mode.unwrap_or(false);
    tauri::async_runtime::spawn(async move {
        let patch = IVerge {
            enable_tun_mode: Some(!enabled),
            ..IVerge::default()
        };
        if let Err(err) = patch_verge(patch).await {
            log::error!(target: "app", "failed to toggle tun mode: {err:?}");
        }
    });
}

pub fn restart_clash_core() {
    let client = match managed_client() {
        Ok(client) => client,
        Err(err) => {
            log::error!(target: "app", "failed to resolve client for core restart: {err:?}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        if let Err(err) = client.rebuild_running_config().await {
            log::error!(target: "app", "failed to restart clash core: {err:?}");
            return;
        }
        log_err!(handle::Handle::update_systray_part());
    });
}

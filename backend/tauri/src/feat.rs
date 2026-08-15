use std::borrow::Borrow;

use anyhow::{Result, bail};
use chimera_ipc::api::status::CoreState;
use serde_yaml::Mapping;
use tauri::{AppHandle, Manager};
use tracing::debug;

use crate::{
    config::{
        chimera::IVerge,
        core::Config,
        profile::item::remote::{RemoteProfileOptionsBuilder, RemoteProfileSubscription},
        runtime::ClashConfigOverrides,
    },
    core::{
        clash::{
            self,
            core::CoreManager,
            transaction::{RuntimePatchCoordinator, TransactionOutcome},
        },
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

async fn validate_external_controller_change(plan: &ClashPatchPlan) -> Result<()> {
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
    let core_state = crate::core::CoreManager::global().status().await;

    if matches!(core_state.0.as_ref(), &CoreState::Running)
        && get_clash_external_port(&strategy, port).is_err()
    {
        bail!("can not select fixed: current port is not available.");
    }

    Ok(())
}

async fn apply_clash_runtime_change(plan: &ClashPatchPlan) -> Result<()> {
    if !plan.requires_restart {
        return Ok(());
    }

    CoreManager::global()
        .restart_core_with_generated_config()
        .await?;
    handle::Handle::refresh_clash();
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

async fn apply_verge_runtime_change(plan: &VergePatchPlan) -> Result<()> {
    let ipc_state = get_ipc_state();

    if let Some(service_mode) = plan.service_mode
        && ipc_state.is_connected()
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
        CoreManager::global()
            .restart_core_with_generated_config()
            .await?;
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
        update_core_config().await?;
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
pub async fn patch_clash_overrides(overrides: ClashConfigOverrides) -> Result<()> {
    let patch = overrides.to_mapping();
    patch_clash_with_overrides(patch, overrides).await
}

/// Applies typed overrides to the running core and desired state through the
/// shared transaction coordinator used by IPC and non-window entry points.
pub async fn patch_running_clash_overrides(
    coordinator: &RuntimePatchCoordinator,
    overrides: ClashConfigOverrides,
) -> TransactionOutcome {
    let mapping = overrides.to_mapping();
    let persist_overrides = overrides.clone();

    coordinator
        .apply(
            mapping,
            clash::api::get_configs,
            |patch| async move { clash::api::patch_configs(&patch).await },
            move |_patch| {
                let overrides = persist_overrides.clone();
                async move { patch_clash_overrides(overrides).await }
            },
        )
        .await
}

/// Applies a general Clash mapping while extracting only supported persistent
/// runtime overrides for the generated config.
pub async fn patch_clash(patch: Mapping) -> Result<()> {
    let overrides = ClashConfigOverrides::from_mapping(&patch)?;
    patch_clash_with_overrides(patch, overrides).await
}

async fn patch_clash_with_overrides(patch: Mapping, overrides: ClashConfigOverrides) -> Result<()> {
    Config::clash().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_clash_patch(&patch)?;
        validate_mixed_port_change(&plan)?;
        validate_external_controller_change(&plan).await?;
        apply_clash_runtime_change(&plan).await?;
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

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    Config::verge().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_verge_patch(&patch)?;
        apply_verge_runtime_change(&plan).await?;
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
async fn update_core_config() -> Result<()> {
    match CoreManager::global()
        .restart_core_with_generated_config()
        .await
    {
        Ok(_) => {
            handle::Handle::refresh_clash();
            handle::Handle::notice_message(&Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

pub(crate) async fn commit_profile_transaction<
    ApplyRuntime,
    ApplyRuntimeFuture,
    PersistState,
    PersistStateFuture,
    Discard,
    RestoreMaterialized,
    RestoreMaterializedFuture,
    RestoreRuntime,
    RestoreRuntimeFuture,
>(
    operation: &'static str,
    apply_runtime: ApplyRuntime,
    persist_state: PersistState,
    discard: Discard,
    restore_materialized: RestoreMaterialized,
    restore_runtime: RestoreRuntime,
) -> Result<()>
where
    ApplyRuntime: FnOnce() -> ApplyRuntimeFuture,
    ApplyRuntimeFuture: std::future::Future<Output = Result<()>>,
    PersistState: FnOnce() -> PersistStateFuture,
    PersistStateFuture: std::future::Future<Output = Result<()>>,
    Discard: FnOnce(),
    RestoreMaterialized: FnOnce() -> RestoreMaterializedFuture,
    RestoreMaterializedFuture: std::future::Future<Output = Result<()>>,
    RestoreRuntime: FnOnce() -> RestoreRuntimeFuture,
    RestoreRuntimeFuture: std::future::Future<Output = Result<()>>,
{
    if let Err(primary_error) = apply_runtime().await {
        discard();
        let materialized_restore = restore_materialized().await;
        return Err(profile_transaction_error(
            operation,
            primary_error,
            materialized_restore,
            Ok(()),
        ));
    }

    if let Err(primary_error) = persist_state().await {
        discard();
        let materialized_restore = restore_materialized().await;
        let runtime_restore = restore_runtime().await;
        return Err(profile_transaction_error(
            operation,
            primary_error,
            materialized_restore,
            runtime_restore,
        ));
    }

    Ok(())
}

fn profile_transaction_error(
    operation: &str,
    primary_error: anyhow::Error,
    materialized_restore: Result<()>,
    runtime_restore: Result<()>,
) -> anyhow::Error {
    if materialized_restore.is_ok() && runtime_restore.is_ok() {
        return primary_error;
    }

    anyhow::anyhow!(
        "{operation} failed: {primary_error:#}; materialized restore: {}; runtime restore: {}",
        format_restore_result(materialized_restore),
        format_restore_result(runtime_restore)
    )
}

fn format_restore_result(result: Result<()>) -> String {
    result
        .err()
        .map(|error| format!("{error:#}"))
        .unwrap_or_else(|| "ok".to_string())
}

fn compensate_profile_preparation<Restore>(
    operation: &str,
    primary_error: anyhow::Error,
    restore_materialized: Restore,
) -> anyhow::Error
where
    Restore: FnOnce() -> Result<()>,
{
    profile_transaction_error(operation, primary_error, restore_materialized(), Ok(()))
}

/// 更新某个profile
/// 如果更新当前配置就激活配置
pub async fn update_profile<T: Borrow<String>>(
    uid: T,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<()> {
    let uid = uid.borrow();
    let profile_item = Config::profiles().latest().get_item(uid)?.clone();
    let previous_file = profile_item.read_file()?;
    let mut item = profile_item
        .as_remote()
        .ok_or_else(|| anyhow::anyhow!("profile `{uid}` is not remote"))?
        .clone();
    item.subscribe(opts).await?;

    let should_update = {
        let mut profiles = Config::profiles().draft();
        match profiles.replace_item(uid, item.into()) {
            Ok(()) => profiles.get_current().iter().any(|current| current == uid),
            Err(primary_error) => {
                Config::profiles().discard();
                return Err(compensate_profile_preparation(
                    "remote profile update preparation",
                    primary_error,
                    || profile_item.save_file(&previous_file),
                ));
            }
        }
    };

    commit_profile_transaction(
        "remote profile update",
        || async {
            if should_update {
                update_core_config().await?;
            }
            Ok(())
        },
        || async { Config::profiles().latest().save_file() },
        || {
            Config::profiles().discard();
        },
        || async { profile_item.save_file(&previous_file) },
        || async {
            if should_update {
                update_core_config().await
            } else {
                Ok(())
            }
        },
    )
    .await?;

    Config::profiles().apply();
    handle::Handle::refresh_profiles();
    Ok(())
}

#[cfg(test)]
mod profile_transaction_tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn record(log: &Arc<Mutex<Vec<&'static str>>>, step: &'static str) {
        log.lock().expect("step log lock").push(step);
    }

    #[test]
    fn preparation_failure_restores_materialized_profile_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.yaml");
        std::fs::write(&path, "mode: global\n").unwrap();

        let error = compensate_profile_preparation(
            "remote profile update preparation",
            anyhow::anyhow!("profile disappeared"),
            {
                let path = path.clone();
                move || {
                    std::fs::write(path, "mode: rule\n")?;
                    Ok(())
                }
            },
        );

        assert_eq!(error.to_string(), "profile disappeared");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "mode: rule\n");
    }

    #[test]
    fn preparation_failure_reports_materialized_restore_failure() {
        let error = compensate_profile_preparation(
            "remote profile update preparation",
            anyhow::anyhow!("profile disappeared"),
            || anyhow::bail!("old file restore failed"),
        );

        let message = error.to_string();
        assert!(message.contains("remote profile update preparation failed"));
        assert!(message.contains("profile disappeared"));
        assert!(message.contains("old file restore failed"));
    }

    #[tokio::test]
    async fn profile_transaction_commits_runtime_then_state_without_compensation() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let result = commit_profile_transaction(
            "profile patch",
            step(&log, "runtime", Ok(())),
            step(&log, "persist", Ok(())),
            sync_step(&log, "discard"),
            step(&log, "restore-file", Ok(())),
            step(&log, "restore-runtime", Ok(())),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(*log.lock().unwrap(), vec!["runtime", "persist"]);
    }

    #[tokio::test]
    async fn runtime_failure_discards_and_restores_materialized_without_second_restart() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let result = commit_profile_transaction(
            "remote profile update",
            step(&log, "runtime", Err(anyhow::anyhow!("rebuild failed"))),
            step(&log, "persist", Ok(())),
            sync_step(&log, "discard"),
            step(&log, "restore-file", Ok(())),
            step(&log, "restore-runtime", Ok(())),
        )
        .await;

        assert_eq!(result.unwrap_err().to_string(), "rebuild failed");
        assert_eq!(
            *log.lock().unwrap(),
            vec!["runtime", "discard", "restore-file"]
        );
    }

    #[tokio::test]
    async fn state_failure_restores_materialized_and_previous_runtime_in_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let result = commit_profile_transaction(
            "remote profile update",
            step(&log, "runtime", Ok(())),
            step(
                &log,
                "persist",
                Err(anyhow::anyhow!("profiles save failed")),
            ),
            sync_step(&log, "discard"),
            step(&log, "restore-file", Ok(())),
            step(&log, "restore-runtime", Ok(())),
        )
        .await;

        assert_eq!(result.unwrap_err().to_string(), "profiles save failed");
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "runtime",
                "persist",
                "discard",
                "restore-file",
                "restore-runtime"
            ]
        );
    }

    #[tokio::test]
    async fn state_failure_reports_all_compensation_failures() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let result = commit_profile_transaction(
            "remote profile update",
            step(&log, "runtime", Ok(())),
            step(
                &log,
                "persist",
                Err(anyhow::anyhow!("profiles save failed")),
            ),
            sync_step(&log, "discard"),
            step(
                &log,
                "restore-file",
                Err(anyhow::anyhow!("old file restore failed")),
            ),
            step(
                &log,
                "restore-runtime",
                Err(anyhow::anyhow!("old runtime restore failed")),
            ),
        )
        .await;

        let message = result.unwrap_err().to_string();
        assert!(message.contains("remote profile update failed"));
        assert!(message.contains("profiles save failed"));
        assert!(message.contains("old file restore failed"));
        assert!(message.contains("old runtime restore failed"));
    }

    #[tokio::test]
    async fn state_failure_restores_deleted_profile_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.yaml");
        std::fs::write(&path, "mode: rule\n").unwrap();

        let result = commit_profile_transaction(
            "profile deletion",
            || std::future::ready(Ok(())),
            {
                let path = path.clone();
                move || {
                    std::fs::remove_file(&path).unwrap();
                    std::future::ready(Err(anyhow::anyhow!("profiles save failed")))
                }
            },
            || {},
            {
                let path = path.clone();
                move || {
                    std::fs::write(&path, "mode: rule\n").unwrap();
                    std::future::ready(Ok(()))
                }
            },
            || std::future::ready(Ok(())),
        )
        .await;

        assert_eq!(result.unwrap_err().to_string(), "profiles save failed");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "mode: rule\n");
    }

    fn step(
        log: &Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
        result: Result<()>,
    ) -> impl FnOnce() -> std::future::Ready<Result<()>> {
        let log = Arc::clone(log);
        move || {
            record(&log, name);
            std::future::ready(result)
        }
    }

    fn sync_step(log: &Arc<Mutex<Vec<&'static str>>>, name: &'static str) -> impl FnOnce() {
        let log = Arc::clone(log);
        move || record(&log, name)
    }
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
        let Some(coordinator) = app_handle.try_state::<RuntimePatchCoordinator>() else {
            log::error!(target: "app", "runtime patch coordinator is not managed");
            return;
        };
        let overrides = ClashConfigOverrides {
            mode: Some(mode),
            ..ClashConfigOverrides::default()
        };

        if let Err(error) = patch_running_clash_overrides(&coordinator, overrides)
            .await
            .into_result()
        {
            log::error!(target: "app", "failed to change clash mode transactionally: {error:#}");
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
    tauri::async_runtime::spawn(async {
        if let Err(err) = CoreManager::global().run_core().await {
            log::error!(target: "app", "failed to restart clash core: {err:?}");
            return;
        }
        log_err!(handle::Handle::update_systray_part());
    });
}

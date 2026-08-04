use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use chimera_ipc::api::status::CoreState;
use futures_util::{StreamExt, stream};
use serde_yaml::Mapping;
use tracing::debug;

use crate::{
    config::{
        chimera::{ClashCore, IVerge},
        clash::IClashTemp,
        core::Config,
        draft::Draft,
        profile::{
            item::{
                MAX_PROFILE_YAML_BYTES, Profile, profile_materialized_path,
                read_file_bytes_with_limit,
                remote::{
                    RemoteProfile, RemoteProfileOptionsBuilder,
                    is_valid_profile_update_interval_minutes,
                },
                shared::validate_profile_uid,
                write_profile_bytes_atomic,
            },
            profile_mutation_lock,
            profiles::Profiles,
        },
        runtime::IRuntime,
    },
    core::{clash::core::CoreManager, handle, service::ipc::get_ipc_state},
    log_err,
    transaction::{
        TransactionOutcome, apply_then_commit_with_rollback, persist_with_compensation,
        preserve_primary_failure,
    },
    utils::{self, help::get_clash_external_port},
};
use handle::Message;

#[cfg(not(feature = "e2e"))]
use crate::core::sysopt;

#[cfg_attr(feature = "e2e", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostSideEffectStatus {
    Applied,
    SkippedForE2e,
}

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

    CoreManager::global().run_core_with_current_config().await?;
    handle::Handle::refresh_clash();
    Ok(())
}

async fn compensate_clash_runtime_change(plan: &ClashPatchPlan) -> Result<()> {
    if !plan.requires_restart {
        return Ok(());
    }

    match CoreManager::global().run_core_with_current_config().await {
        Ok(()) => {
            Config::runtime().apply();
            handle::Handle::refresh_clash();
            Ok(())
        }
        Err(error) => {
            Config::runtime().discard();
            Err(error)
        }
    }
}

#[cfg(not(feature = "e2e"))]
fn run_host_clash_side_effects(plan: &ClashPatchPlan) -> HostSideEffectStatus {
    if plan.mixed_port.is_some() {
        log_err!(sysopt::Sysopt::global().init_sysproxy());
    }
    HostSideEffectStatus::Applied
}

#[cfg(feature = "e2e")]
fn run_host_clash_side_effects(_plan: &ClashPatchPlan) -> HostSideEffectStatus {
    HostSideEffectStatus::SkippedForE2e
}

fn run_clash_patch_side_effects(plan: &ClashPatchPlan) {
    let _ = run_host_clash_side_effects(plan);

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
    proxy_guard_changed: bool,
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
        proxy_guard_changed: patch.enable_proxy_guard.is_some(),
        log_level_changed: patch.app_log_level.is_some(),
        log_max_files_changed: patch.max_log_files.is_some(),
        refresh_systray: patch.enable_system_proxy.is_some() || patch.enable_tun_mode.is_some(),
    })
}

async fn apply_verge_runtime_change(plan: &VergePatchPlan, notify_core_result: bool) -> Result<()> {
    let ipc_state = get_ipc_state();
    let restart_for_service = plan.service_mode.is_some() && ipc_state.is_connected();

    if let Some(service_mode) = plan.service_mode
        && restart_for_service
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
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
        if notify_core_result {
            update_core_config().await?;
        } else {
            apply_generated_core_config().await?;
        }
    } else if restart_for_service {
        apply_generated_core_config().await?;
    }

    Ok(())
}

#[cfg(not(feature = "e2e"))]
fn run_host_verge_side_effects(plan: &VergePatchPlan) -> Result<HostSideEffectStatus> {
    if plan.auto_launch_changed {
        sysopt::Sysopt::global().update_launch()?;
    }

    if plan.system_proxy_changed || plan.proxy_bypass_changed {
        sysopt::Sysopt::global().update_sysproxy()?;
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.proxy_guard_changed && Config::verge().latest().enable_proxy_guard.unwrap_or(false) {
        sysopt::Sysopt::global().guard_proxy();
    }

    Ok(HostSideEffectStatus::Applied)
}

#[cfg(feature = "e2e")]
fn run_host_verge_side_effects(plan: &VergePatchPlan) -> Result<HostSideEffectStatus> {
    let _ = (
        plan.auto_launch_changed,
        plan.system_proxy_changed,
        plan.proxy_bypass_changed,
        plan.proxy_guard_changed,
    );
    Ok(HostSideEffectStatus::SkippedForE2e)
}

fn run_verge_patch_side_effects(plan: &VergePatchPlan, patch: &IVerge) -> Result<()> {
    let _ = run_host_verge_side_effects(plan)?;

    if plan.log_level_changed || plan.log_max_files_changed {
        utils::init::refresh_logger((patch.app_log_level.clone(), patch.max_log_files))?;
    }

    if plan.refresh_systray {
        handle::Handle::update_systray_part()?;
    }

    debug!("todo: handle other fields");

    Ok(())
}

async fn compensate_verge_patch(plan: &VergePatchPlan, previous: &IVerge) -> Result<()> {
    apply_verge_runtime_change(plan, false).await?;
    run_verge_patch_side_effects(plan, previous)
}

fn persist_clash_drafts_with<F>(
    clash: &Draft<IClashTemp>,
    runtime: &Draft<IRuntime>,
    persist: F,
) -> Result<()>
where
    F: FnOnce(&IClashTemp) -> Result<()>,
{
    let snapshot = clash.latest().clone();
    match persist(&snapshot) {
        Ok(()) => {
            clash.apply();
            runtime.apply();
            Ok(())
        }
        Err(error) => {
            clash.discard();
            runtime.discard();
            Err(error)
        }
    }
}

fn clash_runtime_rollback_patch(current: &IClashTemp, patch: &Mapping) -> Result<Mapping> {
    const RUNTIME_KEYS: [&str; 4] = ["allow-lan", "ipv6", "log-level", "mode"];

    for (key, value) in patch {
        if value.is_null() {
            continue;
        }
        let key = key
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("clash runtime patch keys must be strings"))?;
        if !RUNTIME_KEYS.contains(&key) {
            bail!("unsupported clash runtime patch key: {key}");
        }
    }

    let mut rollback = Mapping::new();
    for key in RUNTIME_KEYS {
        if get_non_null_patch_value(patch, key).is_none() {
            continue;
        }
        let value = current
            .0
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing committed clash runtime value: {key}"))?;
        rollback.insert(key.into(), value);
    }
    Ok(rollback)
}

fn clash_config_mutation_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn verge_config_mutation_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 修改clash的配置
async fn patch_clash_unlocked(patch: Mapping) -> Result<()> {
    let plan = plan_clash_patch(&patch)?;
    validate_mixed_port_change(&plan)?;
    validate_external_controller_change(&plan).await?;

    Config::clash().draft().patch_config(patch.clone());
    if let Err(primary) = apply_clash_runtime_change(&plan).await {
        Config::clash().discard();
        Config::runtime().discard();
        return Err(preserve_primary_failure(
            "configuration runtime apply failed",
            primary,
            compensate_clash_runtime_change(&plan).await,
        ));
    }
    Config::runtime().draft().patch_config(patch);

    persist_with_compensation(
        || {
            persist_clash_drafts_with(&Config::clash(), &Config::runtime(), |config| {
                config.save_config()
            })
        },
        || compensate_clash_runtime_change(&plan),
    )
    .await?;
    run_clash_patch_side_effects(&plan);
    if plan.mode_changed {
        log_err!(
            crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change()
                .await,
            "failed to interrupt connections after mode change"
        );
    }
    Ok(())
}

pub async fn patch_clash(patch: Mapping) -> Result<()> {
    let _guard = clash_config_mutation_lock().lock().await;
    patch_clash_unlocked(patch).await
}

pub async fn patch_clash_runtime(patch: Mapping) -> Result<()> {
    let _guard = clash_config_mutation_lock().lock().await;
    let rollback = {
        let clash = Config::clash();
        let current = clash.data();
        clash_runtime_rollback_patch(&current, &patch)?
    };
    let apply_patch = patch.clone();

    match apply_then_commit_with_rollback(
        move || async move { crate::core::clash::api::patch_configs(&apply_patch).await },
        move || async move { patch_clash_unlocked(patch).await },
        move || async move { crate::core::clash::api::patch_configs(&rollback).await },
    )
    .await?
    {
        TransactionOutcome::Committed => Ok(()),
        TransactionOutcome::RolledBack { primary_error } => Err(primary_error),
        TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error,
        } => Err(anyhow::anyhow!(
            "configuration commit failed: {primary_error:#}; runtime compensation failed: {rollback_error:#}"
        )),
    }
}

fn persist_verge_draft_with<F>(draft: &Draft<IVerge>, persist: F) -> Result<()>
where
    F: FnOnce(&IVerge) -> Result<()>,
{
    let snapshot = draft.latest().clone();
    match persist(&snapshot) {
        Ok(()) => {
            draft.apply();
            Ok(())
        }
        Err(error) => {
            draft.discard();
            Err(error)
        }
    }
}

/// 修改verge的配置
/// 一般都是一个个的修改
async fn patch_verge_unlocked(patch: IVerge) -> Result<()> {
    let plan = plan_verge_patch(&patch)?;
    let previous = Config::verge().data().clone();
    Config::verge().draft().patch_config(patch.clone());

    let apply_result = async {
        apply_verge_runtime_change(&plan, true).await?;
        run_verge_patch_side_effects(&plan, &patch)
    }
    .await;
    if let Err(primary) = apply_result {
        Config::verge().discard();
        return Err(preserve_primary_failure(
            "verge runtime or host apply failed",
            primary,
            compensate_verge_patch(&plan, &previous).await,
        ));
    }

    persist_with_compensation(
        || persist_verge_draft_with(&Config::verge(), |config| config.save_file()),
        || compensate_verge_patch(&plan, &previous),
    )
    .await?;
    handle::Handle::refresh_verge();
    Ok(())
}

pub async fn patch_verge(patch: IVerge) -> Result<()> {
    let _guard = verge_config_mutation_lock().lock().await;
    patch_verge_unlocked(patch).await
}

#[cfg(not(feature = "e2e"))]
pub async fn change_clash_core(clash_core: Option<ClashCore>) -> Result<()> {
    let _clash_guard = clash_config_mutation_lock().lock().await;
    let _verge_guard = verge_config_mutation_lock().lock().await;
    CoreManager::global().change_core(clash_core).await
}

#[cfg(feature = "e2e")]
pub async fn change_clash_core(clash_core: Option<ClashCore>) -> Result<()> {
    let _ = clash_core;
    bail!("changing the Clash core is disabled in E2E mode")
}

async fn apply_generated_core_config() -> Result<()> {
    CoreManager::global()
        .restart_core_with_generated_config()
        .await?;
    handle::Handle::refresh_clash();
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfileUpdateOrigin {
    Manual,
    Automatic,
}

fn profile_update_refreshes_immediately(origin: ProfileUpdateOrigin) -> bool {
    origin == ProfileUpdateOrigin::Manual
}

fn automatic_profile_batch_needs_refresh(updated_profiles: usize) -> bool {
    updated_profiles > 0
}

fn profile_core_notice_result(
    origin: ProfileUpdateOrigin,
    should_update: bool,
    result: &Result<()>,
) -> Option<std::result::Result<(), String>> {
    if origin == ProfileUpdateOrigin::Automatic || !should_update {
        return None;
    }
    Some(match result {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("{error:?}")),
    })
}

fn notice_profile_core_result(
    origin: ProfileUpdateOrigin,
    should_update: bool,
    result: &Result<()>,
) {
    if let Some(result) = profile_core_notice_result(origin, should_update, result) {
        handle::Handle::notice_message(&Message::SetConfig(result));
    }
}

/// 更新配置
async fn update_core_config() -> Result<()> {
    let result = apply_generated_core_config().await;
    notice_profile_core_result(ProfileUpdateOrigin::Manual, true, &result);
    result
}

async fn run_serialized_profile_update<T, Operation, OperationFuture>(
    lock: &tokio::sync::Mutex<()>,
    operation: Operation,
) -> T
where
    Operation: FnOnce() -> OperationFuture,
    OperationFuture: Future<Output = T>,
{
    let _guard = lock.lock().await;
    operation().await
}

#[derive(Debug)]
struct ProfileFileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

impl ProfileFileSnapshot {
    fn capture(file: &str) -> Result<Self> {
        Self::capture_path(profile_materialized_path(file)?)
    }

    fn capture_path(path: PathBuf) -> Result<Self> {
        let content = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => Some(
                read_file_bytes_with_limit(&path, MAX_PROFILE_YAML_BYTES).with_context(|| {
                    format!("failed to snapshot profile file {}", path.display())
                })?,
            ),
            Ok(_) => bail!(
                "profile materialized path is not a regular file: {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect profile file {}", path.display()));
            }
        };
        Ok(Self { path, content })
    }

    fn restore(&self) -> Result<()> {
        match &self.content {
            Some(content) => write_profile_bytes_atomic(&self.path, content)
                .with_context(|| format!("failed to restore profile file {}", self.path.display())),
            None => match std::fs::symlink_metadata(&self.path) {
                Ok(metadata) if metadata.file_type().is_file() => std::fs::remove_file(&self.path)
                    .with_context(|| {
                        format!(
                            "failed to remove newly created profile file {}",
                            self.path.display()
                        )
                    }),
                Ok(_) => bail!(
                    "refusing to remove non-file profile replacement: {}",
                    self.path.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error).with_context(|| {
                    format!(
                        "failed to inspect profile rollback path {}",
                        self.path.display()
                    )
                }),
            },
        }
    }
}

fn combine_profile_update_failure(
    primary: anyhow::Error,
    restore: Result<()>,
    rollback_apply: Option<Result<()>>,
) -> anyhow::Error {
    let mut message = format!("profile update failed: {primary:#}");
    if let Err(error) = restore {
        message.push_str(&format!("; profile file rollback failed: {error:#}"));
    }
    if let Some(Err(error)) = rollback_apply {
        message.push_str(&format!("; core rollback failed: {error:#}"));
    }
    anyhow::anyhow!(message)
}

async fn commit_materialized_profile_change<
    Stage,
    Apply,
    ApplyFuture,
    Persist,
    Discard,
    Rollback,
    RollbackFuture,
>(
    snapshot: &ProfileFileSnapshot,
    should_update: bool,
    stage: Stage,
    apply: Apply,
    persist: Persist,
    discard: Discard,
    rollback_apply: Rollback,
) -> Result<()>
where
    Stage: FnOnce() -> Result<()>,
    Apply: FnOnce() -> ApplyFuture,
    ApplyFuture: std::future::Future<Output = Result<()>>,
    Persist: FnOnce() -> Result<()>,
    Discard: FnOnce(),
    Rollback: FnOnce() -> RollbackFuture,
    RollbackFuture: std::future::Future<Output = Result<()>>,
{
    let mut apply_attempted = false;
    let result = async {
        stage()?;
        if should_update {
            apply_attempted = true;
            apply().await?;
        }
        persist()?;
        Ok(())
    }
    .await;

    if let Err(primary) = result {
        discard();
        let restore = snapshot.restore();
        let rollback = if apply_attempted {
            Some(rollback_apply().await)
        } else {
            None
        };
        return Err(combine_profile_update_failure(primary, restore, rollback));
    }
    Ok(())
}

pub async fn save_local_profile_file(uid: String, file_data: String) -> Result<()> {
    validate_profile_uid(&uid)?;
    run_serialized_profile_update(profile_mutation_lock(), move || async move {
        let (profile_item, should_update) = {
            let profiles = Config::profiles().latest();
            (
                profiles.get_item(&uid)?.clone(),
                profiles.materialization_affects_current(&uid),
            )
        };
        if profile_item.as_remote().is_some() {
            bail!("remote profiles are updater-owned");
        }
        let snapshot = ProfileFileSnapshot::capture(profile_item.file())?;

        let result = commit_materialized_profile_change(
            &snapshot,
            should_update,
            move || profile_item.save_file(file_data),
            apply_generated_core_config,
            || Ok(()),
            || {},
            apply_generated_core_config,
        )
        .await;
        notice_profile_core_result(ProfileUpdateOrigin::Manual, should_update, &result);
        result
    })
    .await
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
const PROFILE_AUTO_UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(60);
#[cfg_attr(feature = "e2e", allow(dead_code))]
const PROFILE_AUTO_UPDATE_FAILURE_RETRY_BASE: Duration = Duration::from_secs(5 * 60);
#[cfg_attr(feature = "e2e", allow(dead_code))]
const PROFILE_AUTO_UPDATE_FAILURE_RETRY_MAX: Duration = Duration::from_secs(60 * 60);
#[cfg_attr(feature = "e2e", allow(dead_code))]
const PROFILE_AUTO_UPDATE_MAX_CONCURRENCY: usize = 3;

#[derive(Default)]
struct ProfileUpdateRegistry {
    active: parking_lot::Mutex<HashSet<String>>,
}

struct ProfileUpdateLease<'a> {
    registry: &'a ProfileUpdateRegistry,
    uid: String,
}

impl ProfileUpdateRegistry {
    fn try_begin(&self, uid: String) -> Option<ProfileUpdateLease<'_>> {
        let mut active = self.active.lock();
        if !active.insert(uid.clone()) {
            return None;
        }
        Some(ProfileUpdateLease {
            registry: self,
            uid,
        })
    }
}

impl Drop for ProfileUpdateLease<'_> {
    fn drop(&mut self) {
        self.registry.active.lock().remove(&self.uid);
    }
}

fn profile_update_registry() -> &'static ProfileUpdateRegistry {
    static REGISTRY: OnceLock<ProfileUpdateRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ProfileUpdateRegistry::default)
}

fn try_begin_profile_update(uid: String) -> Option<ProfileUpdateLease<'static>> {
    profile_update_registry().try_begin(uid)
}

#[derive(Clone, Debug)]
struct ProfileAutoUpdateFailure {
    failed_at: Instant,
    attempts: u32,
    profile: RemoteProfile,
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
fn profile_auto_update_retry_delay(attempts: u32) -> Duration {
    let exponent = attempts.saturating_sub(1).min(4);
    let seconds = PROFILE_AUTO_UPDATE_FAILURE_RETRY_BASE
        .as_secs()
        .saturating_mul(1_u64 << exponent)
        .min(PROFILE_AUTO_UPDATE_FAILURE_RETRY_MAX.as_secs());
    Duration::from_secs(seconds)
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
fn profile_auto_update_retry_is_pending(
    failure: Option<&ProfileAutoUpdateFailure>,
    now: Instant,
) -> bool {
    failure.is_some_and(|failure| {
        now.saturating_duration_since(failure.failed_at)
            < profile_auto_update_retry_delay(failure.attempts)
    })
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
fn profile_auto_update_failure_is_relevant(
    failure: &ProfileAutoUpdateFailure,
    current: Option<&RemoteProfile>,
    now: u64,
) -> bool {
    current
        .is_some_and(|current| current == &failure.profile && remote_profile_is_due(current, now))
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
fn remote_profile_is_due(profile: &RemoteProfile, now: u64) -> bool {
    if !is_valid_profile_update_interval_minutes(profile.option.update_interval_minutes) {
        return false;
    }

    let Ok(updated) = u64::try_from(profile.shared.updated) else {
        return false;
    };
    if updated == 0 {
        return true;
    }

    let Some(interval_seconds) = profile.option.update_interval_minutes.checked_mul(60) else {
        return false;
    };
    updated
        .checked_add(interval_seconds)
        .is_some_and(|next_update| now >= next_update)
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
fn due_remote_profile_uids(profiles: &Profiles, now: u64) -> Vec<String> {
    profiles
        .items
        .iter()
        .filter_map(|profile| {
            let remote = profile.as_remote()?;
            remote_profile_is_due(remote, now).then(|| remote.shared.uid.clone())
        })
        .collect()
}

struct PreparedRemoteProfileUpdate {
    source: RemoteProfile,
    updated: RemoteProfile,
    content: String,
}

fn remote_profile_snapshot(uid: &str) -> Result<RemoteProfile> {
    let profiles = Config::profiles().data();
    profiles
        .get_item(uid)?
        .as_remote()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("profile \"uid:{uid}\" is not remote"))
}

fn optional_remote_profile_snapshot(uid: &str) -> Option<RemoteProfile> {
    let profiles = Config::profiles().data();
    profiles
        .get_item(uid)
        .ok()
        .and_then(|profile| profile.as_remote())
        .cloned()
}

async fn prepare_remote_profile_update(
    source: &RemoteProfile,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<PreparedRemoteProfileUpdate> {
    let (updated, content) = source.prepare_subscription(opts).await?;
    Ok(PreparedRemoteProfileUpdate {
        source: source.clone(),
        updated,
        content,
    })
}

fn prepared_remote_profile_can_commit(
    source: &RemoteProfile,
    current: Option<&RemoteProfile>,
    due_at: Option<u64>,
) -> Result<bool> {
    let unchanged = current.is_some_and(|current| current == source);
    if !unchanged {
        if due_at.is_some() {
            return Ok(false);
        }
        bail!(
            "profile \"uid:{}\" changed while the remote update was downloading; retry the update",
            source.shared.uid
        );
    }

    if let Some(now) = due_at
        && !current.is_some_and(|current| remote_profile_is_due(current, now))
    {
        return Ok(false);
    }
    Ok(true)
}

async fn commit_prepared_remote_profile_update(
    prepared: PreparedRemoteProfileUpdate,
    due_at: Option<u64>,
    origin: ProfileUpdateOrigin,
) -> Result<bool> {
    run_serialized_profile_update(profile_mutation_lock(), move || async move {
        let PreparedRemoteProfileUpdate {
            source,
            updated,
            content,
        } = prepared;
        let uid = source.shared.uid.clone();
        let (file, should_update) = {
            let profiles = Config::profiles().latest();
            let current = profiles
                .get_item(&uid)
                .ok()
                .and_then(|profile| profile.as_remote());
            if !prepared_remote_profile_can_commit(&source, current, due_at)? {
                return Ok(false);
            }
            let current = profiles.get_item(&uid)?;
            (
                current.file().to_string(),
                profiles.materialization_affects_current(&uid),
            )
        };
        let snapshot = ProfileFileSnapshot::capture(&file)?;
        let updated_profile = Profile::Remote(updated);
        let staged_profile = updated_profile.clone();
        let stage_uid = uid.clone();
        let result = commit_materialized_profile_change(
            &snapshot,
            should_update,
            move || {
                staged_profile.save_file(content)?;
                Config::profiles()
                    .draft()
                    .replace_item(&stage_uid, updated_profile)
            },
            apply_generated_core_config,
            || Config::profiles().persist_draft_with(|profiles| profiles.save_file()),
            || {
                Config::profiles().discard();
            },
            apply_generated_core_config,
        )
        .await;
        notice_profile_core_result(origin, should_update, &result);
        result?;

        if profile_update_refreshes_immediately(origin) {
            handle::Handle::refresh_profiles();
        }
        Ok(true)
    })
    .await
}

/// 更新某个profile
/// 如果更新当前配置就激活配置
pub async fn update_profile<T: Borrow<String>>(
    uid: T,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<()> {
    let uid = uid.borrow().clone();
    validate_profile_uid(&uid)?;
    let _lease = try_begin_profile_update(uid.clone())
        .ok_or_else(|| anyhow::anyhow!("profile \"uid:{uid}\" is already being updated"))?;
    let source = remote_profile_snapshot(&uid)?;
    let prepared = prepare_remote_profile_update(&source, opts).await?;
    commit_prepared_remote_profile_update(prepared, None, ProfileUpdateOrigin::Manual).await?;
    Ok(())
}

#[derive(Debug)]
struct AutomaticProfileUpdateError {
    source: RemoteProfile,
    error: anyhow::Error,
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
async fn update_profile_if_due(
    uid: String,
    now: u64,
) -> std::result::Result<bool, AutomaticProfileUpdateError> {
    let Some(_lease) = try_begin_profile_update(uid.clone()) else {
        return Ok(false);
    };
    let Some(source) = optional_remote_profile_snapshot(&uid) else {
        return Ok(false);
    };
    if !remote_profile_is_due(&source, now) {
        return Ok(false);
    }

    let prepared = prepare_remote_profile_update(&source, None)
        .await
        .map_err(|error| AutomaticProfileUpdateError {
            source: source.clone(),
            error,
        })?;
    commit_prepared_remote_profile_update(prepared, Some(now), ProfileUpdateOrigin::Automatic)
        .await
        .map_err(|error| AutomaticProfileUpdateError { source, error })
}

async fn run_bounded_profile_update_batch<T, F, Fut>(
    uids: Vec<String>,
    max_concurrency: usize,
    worker: F,
) -> Vec<(String, T)>
where
    T: Send,
    F: Fn(String) -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
{
    let worker = &worker;
    stream::iter(uids.into_iter().map(|uid| async move {
        let result = worker(uid.clone()).await;
        (uid, result)
    }))
    .buffer_unordered(max_concurrency.max(1))
    .collect()
    .await
}

#[cfg_attr(feature = "e2e", allow(dead_code))]
async fn run_due_profile_updates_once(
    now: u64,
    retry_now: Instant,
    failures: &mut HashMap<String, ProfileAutoUpdateFailure>,
) {
    let due = {
        let profiles = Config::profiles().data();
        failures.retain(|uid, failure| {
            let current = profiles
                .get_item(uid)
                .ok()
                .and_then(|profile| profile.as_remote());
            profile_auto_update_failure_is_relevant(failure, current, now)
        });
        due_remote_profile_uids(&profiles, now)
    };

    let eligible = due
        .into_iter()
        .filter(|uid| !profile_auto_update_retry_is_pending(failures.get(uid), retry_now))
        .collect();
    let results = run_bounded_profile_update_batch(
        eligible,
        PROFILE_AUTO_UPDATE_MAX_CONCURRENCY,
        move |uid| async move { update_profile_if_due(uid, now).await },
    )
    .await;

    let mut updated_profiles = 0;
    for (uid, result) in results {
        match result {
            Ok(true) => {
                updated_profiles += 1;
                failures.remove(&uid);
                log::info!(target: "app::profiles", "automatically updated remote profile {uid}");
            }
            Ok(false) => {
                failures.remove(&uid);
            }
            Err(failure) => {
                let attempts = failures
                    .get(&uid)
                    .filter(|previous| previous.profile == failure.source)
                    .map_or(1, |previous| previous.attempts.saturating_add(1));
                failures.insert(
                    uid.clone(),
                    ProfileAutoUpdateFailure {
                        failed_at: retry_now,
                        attempts,
                        profile: failure.source,
                    },
                );
                let retry_delay = profile_auto_update_retry_delay(attempts);
                log::error!(target: "app::profiles", "automatic remote profile update failed for {uid}; retrying after {} seconds: {:#}", retry_delay.as_secs(), failure.error);
            }
        }
    }

    if automatic_profile_batch_needs_refresh(updated_profiles) {
        handle::Handle::refresh_profiles();
    }
}

#[cfg(not(feature = "e2e"))]
pub fn setup_profile_auto_update() {
    tauri::async_runtime::spawn(async {
        let start = tokio::time::Instant::now() + PROFILE_AUTO_UPDATE_POLL_INTERVAL;
        let mut interval = tokio::time::interval_at(start, PROFILE_AUTO_UPDATE_POLL_INTERVAL);
        let mut failures = HashMap::new();
        loop {
            interval.tick().await;
            let timestamp = chrono::Utc::now().timestamp();
            let Ok(now) = u64::try_from(timestamp) else {
                log::error!(target: "app::profiles", "system time is before the Unix epoch; skipping automatic profile updates");
                continue;
            };
            run_due_profile_updates_once(now, Instant::now(), &mut failures).await;
        }
    });
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

pub fn change_clash_mode(mode: String) {
    tauri::async_runtime::spawn(async move {
        let mut patch = Mapping::new();
        patch.insert("mode".into(), mode.into());

        if let Err(err) = patch_clash_runtime(patch).await {
            log::error!(target: "app", "failed to patch clash mode state: {err:?}");
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

#[cfg(all(test, feature = "e2e"))]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use tempfile::tempdir;
    use tokio::sync::{Mutex as AsyncMutex, Semaphore, mpsc, oneshot};

    use super::{
        ClashPatchPlan, HostSideEffectStatus, ProfileAutoUpdateFailure, ProfileFileSnapshot,
        ProfileUpdateOrigin, ProfileUpdateRegistry, TransactionOutcome,
        apply_then_commit_with_rollback, automatic_profile_batch_needs_refresh, change_clash_core,
        clash_config_mutation_lock, clash_runtime_rollback_patch,
        commit_materialized_profile_change, due_remote_profile_uids, persist_clash_drafts_with,
        persist_verge_draft_with, persist_with_compensation, plan_verge_patch,
        prepared_remote_profile_can_commit, profile_auto_update_failure_is_relevant,
        profile_auto_update_retry_delay, profile_auto_update_retry_is_pending,
        profile_core_notice_result, profile_update_refreshes_immediately, remote_profile_is_due,
        run_bounded_profile_update_batch, run_host_clash_side_effects, run_host_verge_side_effects,
        run_serialized_profile_update, verge_config_mutation_lock,
    };
    use crate::config::{
        chimera::{ClashCore, IVerge},
        clash::IClashTemp,
        draft::Draft,
        profile::{
            item::{
                Profile,
                local::LocalProfile,
                remote::{RemoteProfile, RemoteProfileOptions, SubscriptionInfo},
                shared::ProfileShared,
            },
            profiles::Profiles,
        },
        runtime::IRuntime,
    };

    fn mode_mapping(mode: &str) -> serde_yaml::Mapping {
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert("mode".into(), mode.into());
        mapping
    }

    fn push_event(events: &Arc<Mutex<Vec<&'static str>>>, event: &'static str) {
        events
            .lock()
            .expect("profile update event lock")
            .push(event);
    }

    fn remote_profile_fixture(uid: &str, updated: usize, interval_minutes: u64) -> RemoteProfile {
        RemoteProfile {
            url: url::Url::parse("https://example.com/profile.yaml")
                .expect("valid scheduled profile fixture URL"),
            option: RemoteProfileOptions {
                update_interval_minutes: interval_minutes,
                ..RemoteProfileOptions::default()
            },
            shared: ProfileShared {
                uid: uid.to_string(),
                name: uid.to_string(),
                file: format!("{uid}.yaml"),
                desc: None,
                updated,
            },
            chain: Vec::new(),
            extra: SubscriptionInfo::default(),
        }
    }

    #[test]
    fn prepared_remote_profile_commit_rejects_stale_downloads() {
        let source = remote_profile_fixture("remote", 100, 2);
        assert!(prepared_remote_profile_can_commit(&source, Some(&source), None).unwrap());
        assert!(prepared_remote_profile_can_commit(&source, Some(&source), Some(220)).unwrap());
        assert!(!prepared_remote_profile_can_commit(&source, Some(&source), Some(219)).unwrap());

        let mut changed = source.clone();
        changed.option.with_proxy = true;
        let error = prepared_remote_profile_can_commit(&source, Some(&changed), None)
            .expect_err("manual stale downloads must report a conflict");
        assert!(
            error
                .to_string()
                .contains("changed while the remote update was downloading")
        );
        assert!(!prepared_remote_profile_can_commit(&source, Some(&changed), Some(220)).unwrap());
        assert!(!prepared_remote_profile_can_commit(&source, None, Some(220)).unwrap());
    }

    #[test]
    fn automatic_profile_update_due_boundary_is_exact_and_overflow_safe() {
        let profile = remote_profile_fixture("remote", 100, 2);
        assert!(!remote_profile_is_due(&profile, 219));
        assert!(remote_profile_is_due(&profile, 220));

        let never_updated = remote_profile_fixture("new", 0, 2);
        assert!(remote_profile_is_due(&never_updated, 1));

        let future = remote_profile_fixture("future", 1_000, 2);
        assert!(!remote_profile_is_due(&future, 999));

        let invalid = remote_profile_fixture("invalid", 100, 0);
        assert!(!remote_profile_is_due(&invalid, u64::MAX));

        let overflowing = remote_profile_fixture("overflow", usize::MAX, 1);
        assert!(!remote_profile_is_due(&overflowing, u64::MAX));
    }

    #[test]
    fn profile_update_registry_deduplicates_by_uid_and_releases_on_drop() {
        let registry = ProfileUpdateRegistry::default();
        let first = registry
            .try_begin("profile-a".to_string())
            .expect("first update lease must be acquired");
        assert!(registry.try_begin("profile-a".to_string()).is_none());

        let other = registry
            .try_begin("profile-b".to_string())
            .expect("different profile update must proceed independently");
        drop(other);
        assert!(registry.try_begin("profile-b".to_string()).is_some());

        drop(first);
        assert!(registry.try_begin("profile-a".to_string()).is_some());
    }

    #[tokio::test]
    async fn automatic_profile_update_batch_enforces_the_concurrency_limit() {
        let gate = Arc::new(Semaphore::new(0));
        let release = gate.clone();
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let uids = (0..7).map(|index| format!("profile-{index}")).collect();

        let task = tokio::spawn(run_bounded_profile_update_batch(uids, 3, move |uid| {
            let gate = gate.clone();
            let started_tx = started_tx.clone();
            async move {
                started_tx
                    .send(uid.clone())
                    .expect("bounded update start receiver");
                let permit = gate
                    .acquire_owned()
                    .await
                    .expect("bounded update release permit");
                permit.forget();
                uid
            }
        }));

        for _ in 0..3 {
            tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
                .await
                .expect("first three updates must start")
                .expect("bounded update start channel");
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), started_rx.recv())
                .await
                .is_err(),
            "a fourth update started before a concurrency slot was released"
        );

        release.add_permits(7);
        let mut results = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("bounded update batch must complete")
            .expect("bounded update task must not panic");
        results.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(results.len(), 7);
        assert!(results.iter().all(|(uid, result)| uid == result));
    }

    #[test]
    fn automatic_profile_update_retry_delay_uses_exponential_monotonic_boundaries() {
        assert_eq!(profile_auto_update_retry_delay(0), Duration::from_secs(300));
        assert_eq!(profile_auto_update_retry_delay(1), Duration::from_secs(300));
        assert_eq!(profile_auto_update_retry_delay(2), Duration::from_secs(600));
        assert_eq!(
            profile_auto_update_retry_delay(3),
            Duration::from_secs(1_200)
        );
        assert_eq!(
            profile_auto_update_retry_delay(4),
            Duration::from_secs(2_400)
        );
        assert_eq!(
            profile_auto_update_retry_delay(5),
            Duration::from_secs(3_600)
        );
        assert_eq!(
            profile_auto_update_retry_delay(u32::MAX),
            Duration::from_secs(3_600)
        );

        let now = Instant::now();
        let profile = remote_profile_fixture("retry", 100, 2);
        let failure = ProfileAutoUpdateFailure {
            failed_at: now,
            attempts: 2,
            profile: profile.clone(),
        };
        assert!(profile_auto_update_retry_is_pending(Some(&failure), now));
        let before_boundary = ProfileAutoUpdateFailure {
            failed_at: now.checked_sub(Duration::from_secs(599)).unwrap_or(now),
            attempts: 2,
            profile: profile.clone(),
        };
        assert!(profile_auto_update_retry_is_pending(
            Some(&before_boundary),
            now
        ));
        let at_boundary = ProfileAutoUpdateFailure {
            failed_at: now.checked_sub(Duration::from_secs(600)).unwrap_or(now),
            attempts: 2,
            profile,
        };
        assert!(!profile_auto_update_retry_is_pending(
            Some(&at_boundary),
            now
        ));
        assert!(!profile_auto_update_retry_is_pending(None, now));

        assert!(profile_auto_update_failure_is_relevant(
            &failure,
            Some(&failure.profile),
            220
        ));
        let mut changed = failure.profile.clone();
        changed.url = url::Url::parse("https://example.com/changed.yaml")
            .expect("valid changed retry profile URL");
        assert!(!profile_auto_update_failure_is_relevant(
            &failure,
            Some(&changed),
            220
        ));
        assert!(!profile_auto_update_failure_is_relevant(
            &failure,
            Some(&failure.profile),
            219
        ));
        assert!(!profile_auto_update_failure_is_relevant(
            &failure, None, 220
        ));
    }

    #[test]
    fn automatic_profile_update_selects_only_due_remote_profiles_in_stable_order() {
        let local = LocalProfile::builder()
            .build()
            .expect("build local scheduled profile fixture");
        let profiles = Profiles {
            items: vec![
                Profile::Remote(remote_profile_fixture("due-a", 100, 1)),
                Profile::Local(local),
                Profile::Remote(remote_profile_fixture("not-due", 200, 2)),
                Profile::Remote(remote_profile_fixture("due-b", 0, 60)),
            ],
            ..Profiles::default()
        };

        assert_eq!(
            due_remote_profile_uids(&profiles, 200),
            vec!["due-a".to_string(), "due-b".to_string()]
        );
    }

    #[test]
    fn profile_core_notice_uses_only_manual_final_transaction_results() {
        let success: anyhow::Result<()> = Ok(());
        assert_eq!(
            profile_core_notice_result(ProfileUpdateOrigin::Manual, false, &success),
            None
        );
        assert_eq!(
            profile_core_notice_result(ProfileUpdateOrigin::Manual, true, &success),
            Some(Ok(()))
        );
        assert_eq!(
            profile_core_notice_result(ProfileUpdateOrigin::Automatic, true, &success),
            None
        );

        let failure: anyhow::Result<()> = Err(anyhow::anyhow!(
            "profile update failed after successful rollback"
        ));
        let notice = profile_core_notice_result(ProfileUpdateOrigin::Manual, true, &failure)
            .expect("active manual profile failure must emit one final notice")
            .expect_err("failed transaction must never emit a success notice");
        assert!(notice.contains("successful rollback"));
        assert_eq!(
            profile_core_notice_result(ProfileUpdateOrigin::Automatic, true, &failure),
            None
        );

        assert!(profile_update_refreshes_immediately(
            ProfileUpdateOrigin::Manual
        ));
        assert!(!profile_update_refreshes_immediately(
            ProfileUpdateOrigin::Automatic
        ));
        assert!(!automatic_profile_batch_needs_refresh(0));
        assert!(automatic_profile_batch_needs_refresh(1));
        assert!(automatic_profile_batch_needs_refresh(usize::MAX));
    }

    #[tokio::test]
    async fn profile_updates_are_serialized_across_the_complete_async_operation() {
        let lock = Arc::new(AsyncMutex::new(()));
        let (first_entered_tx, first_entered_rx) = oneshot::channel();
        let (release_first_tx, release_first_rx) = oneshot::channel();
        let first_lock = Arc::clone(&lock);
        let first = tokio::spawn(async move {
            run_serialized_profile_update(&first_lock, move || async move {
                first_entered_tx
                    .send(())
                    .expect("signal first profile update entry");
                release_first_rx
                    .await
                    .expect("release first profile update");
                "first"
            })
            .await
        });
        first_entered_rx
            .await
            .expect("first profile update must enter the transaction");

        let (second_started_tx, second_started_rx) = oneshot::channel();
        let (second_entered_tx, mut second_entered_rx) = oneshot::channel();
        let second_lock = Arc::clone(&lock);
        let second = tokio::spawn(async move {
            second_started_tx
                .send(())
                .expect("signal second profile update start");
            run_serialized_profile_update(&second_lock, move || async move {
                second_entered_tx
                    .send(())
                    .expect("signal second profile update entry");
                "second"
            })
            .await
        });
        second_started_rx
            .await
            .expect("second profile update must start waiting");
        tokio::task::yield_now().await;
        assert!(
            matches!(
                second_entered_rx.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "second profile update must not enter before the first transaction finishes"
        );

        release_first_tx
            .send(())
            .expect("release serialized first profile update");
        assert_eq!(first.await.expect("join first profile update"), "first");
        second_entered_rx
            .await
            .expect("second profile update must enter after release");
        assert_eq!(second.await.expect("join second profile update"), "second");
    }

    #[tokio::test]
    async fn staged_local_profile_write_is_restored_when_core_apply_fails() {
        let directory = tempdir().expect("profile save fixture directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, b"old").expect("write old local profile fixture");
        let snapshot =
            ProfileFileSnapshot::capture_path(path.clone()).expect("capture local profile fixture");
        let staged_path = path.clone();
        let rollback_events = Arc::new(Mutex::new(Vec::new()));
        let rollback_events_clone = Arc::clone(&rollback_events);

        let error = commit_materialized_profile_change(
            &snapshot,
            true,
            move || {
                std::fs::write(&staged_path, b"new")
                    .map_err(anyhow::Error::from)
                    .map(|_| ())
            },
            || async { Err(anyhow::anyhow!("injected local core apply failure")) },
            || Ok(()),
            || {},
            move || async move {
                push_event(&rollback_events_clone, "rollback");
                Ok(())
            },
        )
        .await
        .expect_err("failed local core apply must roll back the staged file");

        assert!(error.to_string().contains("local core apply failure"));
        assert_eq!(
            std::fs::read(&path).expect("read restored local profile fixture"),
            b"old"
        );
        assert_eq!(
            *rollback_events
                .lock()
                .expect("local profile rollback event lock"),
            vec!["rollback"]
        );
    }

    #[tokio::test]
    async fn successful_materialized_profile_change_keeps_new_file_and_commits_in_order() {
        let directory = tempdir().expect("profile update fixture directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, b"old").expect("write old profile fixture");
        let snapshot =
            ProfileFileSnapshot::capture_path(path.clone()).expect("capture old profile fixture");
        std::fs::write(&path, b"new").expect("write new profile fixture");
        let events = Arc::new(Mutex::new(Vec::new()));

        let stage_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let persist_events = Arc::clone(&events);
        let discard_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);
        commit_materialized_profile_change(
            &snapshot,
            true,
            move || {
                push_event(&stage_events, "stage");
                Ok(())
            },
            move || async move {
                push_event(&apply_events, "apply");
                Ok(())
            },
            move || {
                push_event(&persist_events, "persist");
                Ok(())
            },
            move || push_event(&discard_events, "discard"),
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect("successful profile update transaction must commit");

        assert_eq!(
            std::fs::read(&path).expect("read committed profile"),
            b"new"
        );
        assert_eq!(
            *events.lock().expect("profile update event lock"),
            vec!["stage", "apply", "persist"]
        );
    }

    #[tokio::test]
    async fn failed_profile_stage_restores_old_file_without_core_rollback() {
        let directory = tempdir().expect("profile update fixture directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, b"old").expect("write old profile fixture");
        let snapshot =
            ProfileFileSnapshot::capture_path(path.clone()).expect("capture old profile fixture");
        std::fs::write(&path, b"new").expect("write new profile fixture");
        let events = Arc::new(Mutex::new(Vec::new()));

        let stage_events = Arc::clone(&events);
        let discard_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);
        let error = commit_materialized_profile_change(
            &snapshot,
            true,
            move || {
                push_event(&stage_events, "stage");
                Err(anyhow::anyhow!("injected stage failure"))
            },
            || async { Ok(()) },
            || Ok(()),
            move || push_event(&discard_events, "discard"),
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect_err("profile stage failure must roll back the file");

        assert!(error.to_string().contains("injected stage failure"));
        assert_eq!(std::fs::read(&path).expect("read restored profile"), b"old");
        assert_eq!(
            *events.lock().expect("profile update event lock"),
            vec!["stage", "discard"]
        );
    }

    #[tokio::test]
    async fn failed_profile_core_apply_restores_file_and_reapplies_previous_config() {
        let directory = tempdir().expect("profile update fixture directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, b"old").expect("write old profile fixture");
        let snapshot =
            ProfileFileSnapshot::capture_path(path.clone()).expect("capture old profile fixture");
        std::fs::write(&path, b"new").expect("write new profile fixture");
        let events = Arc::new(Mutex::new(Vec::new()));

        let stage_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let discard_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);
        let error = commit_materialized_profile_change(
            &snapshot,
            true,
            move || {
                push_event(&stage_events, "stage");
                Ok(())
            },
            move || async move {
                push_event(&apply_events, "apply");
                Err(anyhow::anyhow!("injected core apply failure"))
            },
            || Ok(()),
            move || push_event(&discard_events, "discard"),
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect_err("core apply failure must roll back profile update");

        assert!(error.to_string().contains("injected core apply failure"));
        assert_eq!(std::fs::read(&path).expect("read restored profile"), b"old");
        assert_eq!(
            *events.lock().expect("profile update event lock"),
            vec!["stage", "apply", "discard", "rollback"]
        );
    }

    #[tokio::test]
    async fn failed_profile_metadata_persistence_restores_file_and_core() {
        let directory = tempdir().expect("profile update fixture directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, b"old").expect("write old profile fixture");
        let snapshot =
            ProfileFileSnapshot::capture_path(path.clone()).expect("capture old profile fixture");
        std::fs::write(&path, b"new").expect("write new profile fixture");
        let events = Arc::new(Mutex::new(Vec::new()));

        let stage_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let persist_events = Arc::clone(&events);
        let discard_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);
        let error = commit_materialized_profile_change(
            &snapshot,
            true,
            move || {
                push_event(&stage_events, "stage");
                Ok(())
            },
            move || async move {
                push_event(&apply_events, "apply");
                Ok(())
            },
            move || {
                push_event(&persist_events, "persist");
                Err(anyhow::anyhow!("injected metadata persistence failure"))
            },
            move || push_event(&discard_events, "discard"),
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect_err("metadata persistence failure must roll back profile update");

        assert!(
            error
                .to_string()
                .contains("injected metadata persistence failure")
        );
        assert_eq!(std::fs::read(&path).expect("read restored profile"), b"old");
        assert_eq!(
            *events.lock().expect("profile update event lock"),
            vec!["stage", "apply", "persist", "discard", "rollback"]
        );
    }

    #[tokio::test]
    async fn rollback_removes_new_file_when_profile_had_no_previous_materialization() {
        let directory = tempdir().expect("profile update fixture directory");
        let path = directory.path().join("profile.yaml");
        let snapshot = ProfileFileSnapshot::capture_path(path.clone())
            .expect("capture missing profile fixture");
        std::fs::write(&path, b"new").expect("write new profile fixture");

        commit_materialized_profile_change(
            &snapshot,
            false,
            || Err(anyhow::anyhow!("injected stage failure")),
            || async { Ok(()) },
            || Ok(()),
            || {},
            || async { Ok(()) },
        )
        .await
        .expect_err("failed update must remove newly materialized profile");

        assert!(!path.exists());
    }

    #[tokio::test]
    async fn profile_update_error_reports_primary_and_file_rollback_failures() {
        let directory = tempdir().expect("profile update fixture directory");
        let path = directory.path().join("profile.yaml");
        let snapshot = ProfileFileSnapshot::capture_path(path.clone())
            .expect("capture missing profile fixture");
        std::fs::create_dir(&path).expect("create hostile rollback directory fixture");

        let error = commit_materialized_profile_change(
            &snapshot,
            false,
            || Err(anyhow::anyhow!("injected stage failure")),
            || async { Ok(()) },
            || Ok(()),
            || {},
            || async { Ok(()) },
        )
        .await
        .expect_err("profile update must report rollback path replacement");

        let message = error.to_string();
        assert!(message.contains("injected stage failure"));
        assert!(message.contains("profile file rollback failed"));
        assert!(message.contains("refusing to remove non-file"));
        assert!(path.is_dir());
    }

    #[test]
    fn e2e_verge_patch_skips_all_host_integrations() {
        let patch = IVerge {
            enable_auto_launch: Some(true),
            enable_system_proxy: Some(true),
            system_proxy_bypass: Some("localhost".to_string()),
            enable_proxy_guard: Some(true),
            ..IVerge::default()
        };
        let plan = plan_verge_patch(&patch).expect("failed to plan E2E verge patch");

        assert!(plan.auto_launch_changed);
        assert!(plan.system_proxy_changed);
        assert!(plan.proxy_bypass_changed);
        assert!(plan.proxy_guard_changed);
        assert_eq!(
            run_host_verge_side_effects(&plan)
                .expect("E2E host verge side effects should be skipped safely"),
            HostSideEffectStatus::SkippedForE2e
        );
    }

    #[tokio::test]
    async fn e2e_core_change_is_rejected_before_host_access() {
        let error = change_clash_core(Some(ClashCore::Mihomo))
            .await
            .expect_err("E2E core changes must be rejected before process or file access");

        assert!(error.to_string().contains("disabled in E2E mode"));
    }

    #[tokio::test]
    async fn config_transaction_locks_serialize_per_domain_without_cross_blocking() {
        let clash_guard = clash_config_mutation_lock().lock().await;
        assert!(
            clash_config_mutation_lock().try_lock().is_err(),
            "a second Clash transaction must not enter while the first is active"
        );
        let verge_guard = verge_config_mutation_lock()
            .try_lock()
            .expect("a Clash transaction must not block an unrelated Verge transaction");
        drop(verge_guard);

        let (clash_entered_tx, mut clash_entered_rx) = mpsc::channel(1);
        let clash_waiter = tokio::spawn(async move {
            let _guard = clash_config_mutation_lock().lock().await;
            clash_entered_tx
                .send(())
                .await
                .expect("signal queued Clash transaction entry");
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), clash_entered_rx.recv())
                .await
                .is_err(),
            "the queued Clash transaction must remain blocked"
        );
        drop(clash_guard);
        tokio::time::timeout(Duration::from_secs(1), clash_entered_rx.recv())
            .await
            .expect("queued Clash transaction must enter after release")
            .expect("queued Clash transaction signal");
        clash_waiter.await.expect("queued Clash transaction task");

        let verge_guard = verge_config_mutation_lock().lock().await;
        assert!(
            verge_config_mutation_lock().try_lock().is_err(),
            "a second Verge transaction must not enter while the first is active"
        );
        let clash_guard = clash_config_mutation_lock()
            .try_lock()
            .expect("a Verge transaction must not block an unrelated Clash transaction");
        drop(clash_guard);
        drop(verge_guard);
    }

    #[test]
    fn clash_runtime_rollback_snapshot_covers_only_supported_changed_fields() {
        let mut current = mode_mapping("rule");
        current.insert("allow-lan".into(), false.into());
        current.insert("ipv6".into(), false.into());
        current.insert("log-level".into(), "info".into());
        let current = IClashTemp(current);

        let mut patch = serde_yaml::Mapping::new();
        patch.insert("mode".into(), "global".into());
        patch.insert("ipv6".into(), true.into());
        let rollback = clash_runtime_rollback_patch(&current, &patch)
            .expect("supported runtime fields must have a complete rollback snapshot");

        assert_eq!(rollback.len(), 2);
        assert_eq!(
            rollback.get("mode").and_then(serde_yaml::Value::as_str),
            Some("rule")
        );
        assert_eq!(
            rollback.get("ipv6").and_then(serde_yaml::Value::as_bool),
            Some(false)
        );

        patch.insert("external-controller".into(), "127.0.0.1:9999".into());
        let error = clash_runtime_rollback_patch(&current, &patch)
            .expect_err("unsupported runtime keys must be rejected before API mutation");
        assert!(
            error
                .to_string()
                .contains("unsupported clash runtime patch key")
        );
    }

    #[tokio::test]
    async fn runtime_transaction_applies_then_commits_without_rollback_on_success() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let apply_events = Arc::clone(&events);
        let commit_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let outcome = apply_then_commit_with_rollback(
            move || async move {
                push_event(&apply_events, "apply");
                Ok::<(), anyhow::Error>(())
            },
            move || async move {
                push_event(&commit_events, "commit");
                Ok(())
            },
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect("successful runtime transaction must not roll back");

        assert!(matches!(outcome, TransactionOutcome::Committed));

        assert_eq!(
            *events.lock().expect("runtime transaction event lock"),
            vec!["apply", "commit"]
        );
    }

    #[tokio::test]
    async fn runtime_transaction_stops_before_commit_when_api_apply_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let apply_events = Arc::clone(&events);
        let commit_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let error = apply_then_commit_with_rollback(
            move || async move {
                push_event(&apply_events, "apply");
                Err(anyhow::anyhow!("injected API apply failure"))
            },
            move || async move {
                push_event(&commit_events, "commit");
                Ok(())
            },
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect_err("failed API apply must abort before commit");

        assert_eq!(error.to_string(), "injected API apply failure");
        assert_eq!(
            *events.lock().expect("runtime transaction event lock"),
            vec!["apply"]
        );
    }

    #[tokio::test]
    async fn runtime_transaction_rolls_back_after_commit_failure() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let apply_events = Arc::clone(&events);
        let commit_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let outcome = apply_then_commit_with_rollback(
            move || async move {
                push_event(&apply_events, "apply");
                Ok::<(), anyhow::Error>(())
            },
            move || async move {
                push_event(&commit_events, "commit");
                Err(anyhow::anyhow!("injected commit failure"))
            },
            move || async move {
                push_event(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect("runtime apply succeeds");

        let TransactionOutcome::RolledBack { primary_error } = outcome else {
            panic!("failed commit with successful rollback must be rolled back");
        };
        assert_eq!(primary_error.to_string(), "injected commit failure");
        assert_eq!(
            *events.lock().expect("runtime transaction event lock"),
            vec!["apply", "commit", "rollback"]
        );
    }

    #[tokio::test]
    async fn runtime_transaction_reports_commit_and_rollback_failures() {
        let outcome = apply_then_commit_with_rollback(
            || async { Ok::<(), anyhow::Error>(()) },
            || async { Err(anyhow::anyhow!("injected commit failure")) },
            || async { Err(anyhow::anyhow!("injected API rollback failure")) },
        )
        .await
        .expect("runtime apply succeeds");

        let TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error,
        } = outcome
        else {
            panic!("commit and rollback failures must be reported together");
        };
        assert!(
            primary_error
                .to_string()
                .contains("injected commit failure")
        );
        assert!(
            rollback_error
                .to_string()
                .contains("injected API rollback failure")
        );
    }

    #[tokio::test]
    async fn successful_persistence_skips_runtime_compensation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let persist_events = Arc::clone(&events);
        let compensation_events = Arc::clone(&events);

        persist_with_compensation(
            move || {
                push_event(&persist_events, "persist");
                Ok(())
            },
            move || async move {
                push_event(&compensation_events, "compensate");
                Ok(())
            },
        )
        .await
        .expect("successful persistence must not run compensation");

        assert_eq!(
            *events.lock().expect("persistence event lock"),
            vec!["persist"]
        );
    }

    #[tokio::test]
    async fn failed_persistence_runs_compensation_and_preserves_primary_error() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let persist_events = Arc::clone(&events);
        let compensation_events = Arc::clone(&events);

        let error = persist_with_compensation(
            move || {
                push_event(&persist_events, "persist");
                Err(anyhow::anyhow!("injected persistence failure"))
            },
            move || async move {
                push_event(&compensation_events, "compensate");
                Ok(())
            },
        )
        .await
        .expect_err("failed persistence must remain visible after compensation");

        assert_eq!(error.to_string(), "injected persistence failure");
        assert_eq!(
            *events.lock().expect("persistence event lock"),
            vec!["persist", "compensate"]
        );
    }

    #[tokio::test]
    async fn failed_persistence_reports_compensation_failure_without_hiding_primary_error() {
        let error = persist_with_compensation(
            || Err(anyhow::anyhow!("injected persistence failure")),
            || async { Err(anyhow::anyhow!("injected compensation failure")) },
        )
        .await
        .expect_err("both persistence and compensation failures must be returned");
        let message = error.to_string();

        assert!(message.contains("injected persistence failure"));
        assert!(message.contains("injected compensation failure"));
    }

    #[test]
    fn failed_verge_persistence_discards_the_draft_before_next_read() {
        let draft = Draft::from(IVerge {
            lighten_animation_effects: Some(false),
            ..IVerge::default()
        });
        draft.draft().lighten_animation_effects = Some(true);

        let error = persist_verge_draft_with(&draft, |_| {
            Err(anyhow::anyhow!("injected config persistence failure"))
        })
        .expect_err("persistence failure must be returned");

        assert!(
            error
                .to_string()
                .contains("injected config persistence failure")
        );
        assert_eq!(draft.data().lighten_animation_effects, Some(false));
        assert!(draft.apply().is_none(), "failed draft must be discarded");
    }

    #[test]
    fn successful_verge_persistence_commits_the_saved_snapshot() {
        let draft = Draft::from(IVerge {
            lighten_animation_effects: Some(false),
            ..IVerge::default()
        });
        draft.draft().lighten_animation_effects = Some(true);
        let mut persisted_value = None;

        persist_verge_draft_with(&draft, |config| {
            persisted_value = config.lighten_animation_effects;
            Ok(())
        })
        .expect("successful persistence must commit the draft");

        assert_eq!(persisted_value, Some(true));
        assert_eq!(draft.data().lighten_animation_effects, Some(true));
        assert!(draft.apply().is_none(), "committed draft must be consumed");
    }

    #[test]
    fn failed_clash_persistence_discards_clash_and_runtime_drafts() {
        let clash = Draft::from(IClashTemp(mode_mapping("rule")));
        let runtime = Draft::from(IRuntime {
            config: Some(mode_mapping("rule")),
        });
        clash.draft().patch_config(mode_mapping("global"));
        runtime.draft().patch_config(mode_mapping("global"));

        let error = persist_clash_drafts_with(&clash, &runtime, |_| {
            Err(anyhow::anyhow!("injected clash persistence failure"))
        })
        .expect_err("clash persistence failure must be returned");

        assert!(
            error
                .to_string()
                .contains("injected clash persistence failure")
        );
        assert_eq!(
            clash
                .data()
                .0
                .get("mode")
                .and_then(serde_yaml::Value::as_str),
            Some("rule")
        );
        assert_eq!(
            runtime
                .data()
                .config
                .as_ref()
                .and_then(|config| config.get("mode"))
                .and_then(serde_yaml::Value::as_str),
            Some("rule")
        );
        assert!(clash.apply().is_none());
        assert!(runtime.apply().is_none());
    }

    #[test]
    fn successful_clash_persistence_commits_both_saved_drafts() {
        let clash = Draft::from(IClashTemp(mode_mapping("rule")));
        let runtime = Draft::from(IRuntime {
            config: Some(mode_mapping("rule")),
        });
        clash.draft().patch_config(mode_mapping("global"));
        runtime.draft().patch_config(mode_mapping("global"));
        let mut persisted_mode = None;

        persist_clash_drafts_with(&clash, &runtime, |config| {
            persisted_mode = config
                .0
                .get("mode")
                .and_then(serde_yaml::Value::as_str)
                .map(str::to_owned);
            Ok(())
        })
        .expect("successful clash persistence must commit both drafts");

        assert_eq!(persisted_mode.as_deref(), Some("global"));
        assert_eq!(
            clash
                .data()
                .0
                .get("mode")
                .and_then(serde_yaml::Value::as_str),
            Some("global")
        );
        assert_eq!(
            runtime
                .data()
                .config
                .as_ref()
                .and_then(|config| config.get("mode"))
                .and_then(serde_yaml::Value::as_str),
            Some("global")
        );
        assert!(clash.apply().is_none());
        assert!(runtime.apply().is_none());
    }

    #[test]
    fn e2e_mixed_port_patch_skips_system_proxy_reconfiguration() {
        let plan = ClashPatchPlan {
            mixed_port: Some(7890),
            mixed_port_changed: true,
            external_controller: None,
            external_controller_changed: false,
            mode_changed: false,
            requires_restart: true,
        };

        assert_eq!(
            run_host_clash_side_effects(&plan),
            HostSideEffectStatus::SkippedForE2e
        );
    }
}

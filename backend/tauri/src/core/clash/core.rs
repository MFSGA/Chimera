use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use chimera_config::clash::config::ClashConfig;
use chimera_ipc::{api::status::CoreState, utils::get_current_ts};
use chimera_utils::{
    core::{
        CommandEvent,
        instance::{CoreInstance, CoreInstanceBuilder},
    },
    runtime::spawn,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    client::{
        SessionPortResolver,
        runtime::{
            CheckedPromotionError, RuntimeDocument, RuntimeLifecycle, RuntimePaths,
            RuntimeRebuildGate, RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransactionSnapshot,
            RuntimeTransformFailure, capture_runtime_transaction, check_and_promote_candidate,
            restore_failed_apply,
        },
    },
    config::{chimera::ClashCore, clash::ClashInfo, core::Config},
    core::{clash::api, logger::Logger},
    enhance::{PostProcessingOutput, TransformFailureError},
    log_err,
    utils::dirs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    /// Run as child process directly
    Normal,
    /// Run by Nyanpasu Service via a ipc call
    Service,
    // TODO: Not implemented yet
    /// Run as elevated process, if profile advice to run as elevated
    Elevated,
}

impl RunType {
    /// Classify the core launch mode from explicit inputs.
    ///
    /// Typed client paths should use this instead of `Default`, which remains
    /// only for legacy callers that still read the combined verge state.
    pub(crate) fn classify(
        enable_service: bool,
        ipc_state: crate::core::service::ipc::IpcState,
    ) -> Self {
        if enable_service && ipc_state.is_connected() {
            Self::Service
        } else {
            Self::Normal
        }
    }

    fn from_service_mode(enable_service: bool) -> Self {
        let run_type = Self::classify(enable_service, crate::core::service::ipc::get_ipc_state());
        if run_type == Self::Service {
            tracing::info!("run core as service");
        } else {
            tracing::info!("run core as child process");
        }
        run_type
    }
}

impl Default for RunType {
    fn default() -> Self {
        let enable_service = Config::verge()
            .latest()
            .enable_service_mode
            .unwrap_or(false);
        Self::from_service_mode(enable_service)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RuntimeRestartError {
    #[error("failed to prepare runtime candidate: {0}")]
    Prepare(#[source] anyhow::Error),
    #[error("runtime candidate check failed: {0}")]
    Check(#[source] anyhow::Error),
    #[error("failed to promote runtime candidate: {0}")]
    Promote(#[source] anyhow::Error),
    #[error("failed to start core with promoted runtime product: {0}")]
    Start(#[source] anyhow::Error),
    #[error("runtime restart failed: {primary}; recovery also failed: {recovery}")]
    Recovery { primary: String, recovery: String },
}

impl RuntimeRestartError {
    /// Recovery is only required after the product may have changed or core apply began.
    fn requires_recovery(&self) -> bool {
        matches!(
            self,
            Self::Promote(_) | Self::Start(_) | Self::Recovery { .. }
        )
    }
}

struct RuntimeApplyTransaction {
    paths: RuntimePaths,
    rollback: RuntimeTransactionSnapshot,
    previous_clash: crate::config::clash::IClashTemp,
    recovery_target: ClashCore,
    target_core: ClashCore,
}

#[derive(Debug)]
struct Instance {
    child: Mutex<Arc<CoreInstance>>,
    stated_changed_at: Arc<AtomicI64>,
    kill_flag: Arc<AtomicBool>,
    recovery_notify: Arc<tokio::sync::Notify>,
}

impl Instance {
    /// get core state with state changed timestamp
    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64) {
        let this = self.child.lock();
        (
            Cow::Borrowed(match this.state() {
                chimera_utils::core::instance::CoreInstanceState::Running => &CoreState::Running,
                chimera_utils::core::instance::CoreInstanceState::Stopped => {
                    &CoreState::Stopped(None)
                }
            }),
            self.stated_changed_at.load(Ordering::Relaxed),
        )
    }

    pub fn run_type(&self) -> RunType {
        RunType::Normal
    }

    pub async fn state<'a>(&self) -> Cow<'a, CoreState> {
        let this = self.child.lock();
        Cow::Borrowed(match this.state() {
            chimera_utils::core::instance::CoreInstanceState::Running => &CoreState::Running,
            chimera_utils::core::instance::CoreInstanceState::Stopped => &CoreState::Stopped(None),
        })
    }

    pub async fn stop(&self) -> Result<()> {
        let state = self.state().await;
        if matches!(state.as_ref(), CoreState::Stopped(_)) {
            anyhow::bail!("core is already stopped");
        }
        self.kill_flag.store(true, Ordering::Release);
        let child = self.child.lock().clone();
        child.kill().await?;
        self.stated_changed_at
            .store(get_current_ts(), Ordering::Relaxed);
        Ok(())
    }

    pub fn try_new(
        run_type: RunType,
        clash_core: ClashCore,
        config_path: PathBuf,
        recovery_notify: Arc<tokio::sync::Notify>,
    ) -> Result<Self> {
        let config_path = camino::Utf8PathBuf::from_path_buf(config_path)
            .map_err(|e| anyhow::anyhow!("failed to convert config path to utf8 path: {:?}", e))?;

        match run_type {
            RunType::Normal => {
                let core_type: chimera_utils::core::CoreType = (&clash_core).into();
                let data_dir =
                    camino::Utf8PathBuf::from_path_buf(dirs::app_data_dir()?).map_err(|e| {
                        anyhow::anyhow!("failed to convert data dir to utf8 path: {:?}", e)
                    })?;
                let binary = camino::Utf8PathBuf::from_path_buf(find_binary_path(&core_type)?)
                    .map_err(|e| {
                        anyhow::anyhow!("failed to convert binary path to utf8 path: {:?}", e)
                    })?;
                let pid_path = camino::Utf8PathBuf::from_path_buf(dirs::clash_pid_path()?)
                    .map_err(|e| {
                        anyhow::anyhow!("failed to convert pid path to utf8 path: {:?}", e)
                    })?;
                let instance = Arc::new(
                    CoreInstanceBuilder::default()
                        .core_type(core_type)
                        .app_dir(data_dir)
                        .binary_path(binary)
                        .config_path(config_path.clone())
                        .pid_path(pid_path)
                        .build()?,
                );
                Ok(Instance {
                    child: Mutex::new(instance),
                    kill_flag: Arc::new(AtomicBool::new(false)),
                    stated_changed_at: Arc::new(AtomicI64::new(get_current_ts())),
                    recovery_notify,
                })
            }
            RunType::Service => {
                anyhow::bail!("local CoreManager cannot construct a Service-hosted instance")
            }
            RunType::Elevated => todo!(),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let instance = self.child.lock().clone();
        let (is_premium, core_type) = {
            let child = self.child.lock();
            (
                matches!(
                    child.core_type,
                    chimera_utils::core::CoreType::Clash(
                        chimera_utils::core::ClashCoreType::ClashPremium
                    )
                ),
                child.core_type.clone(),
            )
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel::<anyhow::Result<()>>(1);
        let stated_changed_at = self.stated_changed_at.clone();
        let kill_flag = self.kill_flag.clone();
        let recovery_notify = self.recovery_notify.clone();
        tracing::trace!("todo: instance start and may use admin performs better.");
        tokio::spawn(async move {
            match instance.run().await {
                Ok((_, mut rx)) => {
                    kill_flag.store(false, Ordering::Release);
                    let mut err_buf: Vec<String> = Vec::with_capacity(6);
                    loop {
                        if let Some(event) = rx.recv().await {
                            match event {
                                CommandEvent::Stdout(line) => {
                                    if is_premium {
                                        let log = api::parse_log(line.clone());
                                        log::info!(target: "app", "[{core_type}]: {log}");
                                    } else {
                                        log::info!(target: "app", "[{core_type}]: {line}");
                                    }
                                    Logger::global().set_log(line);
                                }
                                CommandEvent::Stderr(line) => {
                                    log::error!(target: "app", "[{core_type}]: {line}");
                                    err_buf.push(line.clone());
                                    Logger::global().set_log(line);
                                }
                                CommandEvent::Error(e) => {
                                    log::error!(target: "app", "[{core_type}]: {e}");
                                    let err =
                                        anyhow::anyhow!(format!("{}\n{}", e, err_buf.join("\n")));
                                    Logger::global().set_log(e);
                                    let _ = tx.send(Err(err)).await;
                                    stated_changed_at.store(get_current_ts(), Ordering::Relaxed);
                                    break;
                                }
                                CommandEvent::Terminated(status) => {
                                    log::error!(
                                        target: "app",
                                        "core terminated with status: {status:?}"
                                    );
                                    stated_changed_at.store(get_current_ts(), Ordering::Relaxed);
                                    if status.code != Some(0)
                                        || !matches!(status.signal, Some(9) | Some(15))
                                    {
                                        let err = anyhow::anyhow!(format!(
                                            "core terminated with status: {:?}\n{}",
                                            status,
                                            err_buf.join("\n")
                                        ));
                                        tracing::error!("{}\n{}", err, err_buf.join("\n"));
                                        if tx.send(Err(err)).await.is_err()
                                            && !kill_flag.load(Ordering::Acquire)
                                        {
                                            recovery_notify.notify_one();
                                        }
                                    }
                                    break;
                                }
                                CommandEvent::DelayCheckpointPass => {
                                    tracing::debug!("delay checkpoint pass");
                                    stated_changed_at.store(get_current_ts(), Ordering::Relaxed);
                                    tx.send(Ok(())).await.unwrap();
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    spawn(async move {
                        tx.send(Err(err.into())).await.unwrap();
                    });
                }
            }
        });
        rx.recv().await.unwrap()?;
        Ok(())
    }
}

/// Exclusive guard for core lifecycle mutations.
#[must_use = "the lifecycle lease releases the mutex when dropped"]
pub(crate) struct CoreLifecycleLease<'a> {
    manager: &'a CoreManager,
    _guard: tokio::sync::MutexGuard<'a, ()>,
}

impl CoreLifecycleLease<'_> {
    pub(crate) async fn apply_runtime_snapshot(
        &self,
        snapshot: Arc<RuntimeSnapshot>,
        run_type: RunType,
    ) -> Result<()> {
        self.manager
            .apply_runtime_snapshot_locked(snapshot, run_type)
            .await
    }

    pub(crate) async fn stop_core(&self) -> Result<()> {
        self.manager.stop_core_with_lease(self).await
    }
}

#[derive(Debug)]
struct CoreLifecycleState {
    /// Single mutex domain for run/restart, stop, check, recover, and core changes.
    run_lock: RuntimeRebuildGate,
    runtime_lifecycle: Arc<RuntimeLifecycle>,
    port_resolver: Arc<SessionPortResolver>,
    recovery_notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimePreparation {
    runtime_lifecycle: Arc<RuntimeLifecycle>,
    port_resolver: Arc<SessionPortResolver>,
}

impl RuntimePreparation {
    pub(crate) fn prepare_ports(
        &self,
        clash: &ClashConfig,
    ) -> std::result::Result<crate::client::ports::PreparedPortBindings, RuntimeRestartError> {
        self.port_resolver
            .prepare(clash)
            .map_err(RuntimeRestartError::Prepare)
    }

    pub(crate) fn commit_ports(&self, prepared: crate::client::ports::PreparedPortBindings) {
        self.port_resolver.commit(prepared);
    }

    pub(crate) fn record_prepare_failure(
        &self,
        error: &anyhow::Error,
    ) -> std::result::Result<(), RuntimeRestartError> {
        if let Some(transform) = error.downcast_ref::<TransformFailureError>() {
            let revision = self
                .runtime_lifecycle
                .allocate_transform_attempt_revision()
                .map_err(RuntimeRestartError::Prepare)?;
            self.runtime_lifecycle
                .publish_transform_failure(RuntimeTransformFailure {
                    attempt_revision: revision,
                    transform_uid: transform.transform_uid.clone(),
                    scope_uid: transform.scope_uid.clone(),
                    script_type: transform.script_type,
                    message: transform.message(),
                });
        }
        Ok(())
    }

    pub(crate) fn clear_prepare_failure(&self) {
        self.runtime_lifecycle.clear_transform_failure();
    }

    pub(crate) async fn build_runtime_document(
        &self,
        target_core: ClashCore,
        clash: &ClashConfig,
        profiles: &crate::config::profile::profiles::Profiles,
        resolved_ports: chimera_config::runtime::executor::ResolvedPortBindings,
        enable_builtin_enhanced: bool,
    ) -> anyhow::Result<RuntimeDocument> {
        let output = Config::generate_runtime_output_with_ports(
            clash,
            profiles,
            target_core,
            resolved_ports,
            enable_builtin_enhanced,
        )
        .await?;
        let bytes = Config::render_runtime_bytes(&output.config)?;
        Ok(RuntimeDocument::from_data(
            target_core,
            bytes.into(),
            RuntimeSnapshotData {
                config: output.config,
                exists_keys: output.exists_keys,
                postprocessing_output: output.postprocessing_output,
                inspection: Arc::new(output.inspection),
            },
        ))
    }
}

#[derive(Debug)]
pub struct CoreManager {
    instance: Mutex<Option<Arc<Instance>>>,
    lifecycle: CoreLifecycleState,
}

impl CoreManager {
    pub(crate) fn new() -> Self {
        Self {
            instance: Mutex::new(None),
            lifecycle: CoreLifecycleState {
                run_lock: RuntimeRebuildGate::default(),
                runtime_lifecycle: Arc::new(RuntimeLifecycle::default()),
                port_resolver: Arc::new(SessionPortResolver::default()),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            },
        }
    }

    pub(crate) async fn begin_lifecycle(&self) -> CoreLifecycleLease<'_> {
        CoreLifecycleLease {
            manager: self,
            _guard: self.lifecycle.run_lock.lock().await,
        }
    }

    pub(crate) fn runtime_preparation(&self) -> RuntimePreparation {
        RuntimePreparation {
            runtime_lifecycle: self.lifecycle.runtime_lifecycle.clone(),
            port_resolver: self.lifecycle.port_resolver.clone(),
        }
    }

    pub(crate) fn allocate_runtime_revision(
        &self,
    ) -> anyhow::Result<crate::client::runtime::RuntimeRevision> {
        self.lifecycle.runtime_lifecycle.allocate_revision()
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        self.lifecycle
            .runtime_lifecycle
            .snapshot()
            .applied
            .map(|snapshot| {
                (
                    snapshot.revision.get(),
                    snapshot.postprocessing_output.clone(),
                )
            })
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.lifecycle.runtime_lifecycle.snapshot().promoted
    }

    pub(crate) fn applied_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.lifecycle.runtime_lifecycle.snapshot().applied
    }

    pub(crate) fn discard_runtime_draft(&self) {
        Config::runtime().discard();
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.lifecycle
            .runtime_lifecycle
            .snapshot()
            .last_transform_failure
    }

    pub(crate) fn applied_clash_info(&self) -> Option<ClashInfo> {
        self.lifecycle
            .runtime_lifecycle
            .snapshot()
            .applied
            .map(|snapshot| snapshot.clash_info())
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        if self.lifecycle.run_lock.is_locked() {
            return Config::clash().latest().get_client_info();
        }

        self.applied_clash_info()
            .unwrap_or_else(|| Config::clash().latest().get_client_info())
    }

    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64, RunType) {
        let instance = {
            let instance = self.instance.lock();
            instance.as_ref().cloned()
        };
        if let Some(instance) = instance {
            let (state, ts) = instance.status().await;
            (state, ts, instance.run_type())
        } else {
            (Cow::Owned(CoreState::Stopped(None)), 0_i64, RunType::Normal)
        }
    }

    fn committed_core() -> ClashCore {
        Config::verge()
            .data()
            .clash_core
            .unwrap_or(ClashCore::Mihomo)
    }

    async fn stop_running_instance(&self) -> Result<()> {
        let instance = {
            let instance = self.instance.lock();
            instance.as_ref().cloned()
        };
        if let Some(instance) = instance
            && matches!(instance.state().await.as_ref(), CoreState::Running)
        {
            log::debug!(target: "app", "core is already running, stop it first...");
            instance.stop().await?;
        }
        Ok(())
    }

    async fn run_core_from_product_inner(
        &self,
        product: &Path,
        target_core: ClashCore,
        run_type: RunType,
    ) -> Result<()> {
        self.stop_running_instance().await?;
        let instance = Arc::new(Instance::try_new(
            run_type,
            target_core,
            product.to_path_buf(),
            self.lifecycle.recovery_notify.clone(),
        )?);

        #[cfg(target_os = "macos")]
        {
            let enable_tun = Config::verge().latest().enable_tun_mode.unwrap_or(false);
            let _ = self
                .change_default_network_dns(enable_tun)
                .await
                .inspect_err(|e| log::error!(target: "app", "failed to set system dns: {:?}", e));
        }

        {
            let mut this = self.instance.lock();
            *this = Some(instance.clone());
        }
        instance.start().await?;
        let app_handle = crate::core::handle::Handle::global()
            .app_handle
            .lock()
            .clone();
        if let Some(app_handle) = app_handle {
            log_err!(
                crate::core::clash::restart_ws_connector(&app_handle).await,
                "failed to restart clash websocket connector"
            );
        }
        crate::core::handle::Handle::refresh_clash();
        Ok(())
    }

    async fn check_candidate_path(&self, path: &Path, target_core: ClashCore) -> Result<()> {
        use chimera_utils::core::instance::CoreInstance;

        let config_path = Utf8PathBuf::from_path_buf(path.to_path_buf())
            .map_err(|_| anyhow::anyhow!("failed to convert candidate path to utf8"))?;
        let core_type: chimera_utils::core::CoreType = (&target_core).into();
        let app_dir = Utf8PathBuf::from_path_buf(dirs::app_data_dir()?)
            .map_err(|_| anyhow::anyhow!("failed to convert app dir to utf8 path"))?;
        let binary_path = Utf8PathBuf::from_path_buf(find_binary_path(&core_type)?)
            .map_err(|_| anyhow::anyhow!("failed to convert binary path to utf8 path"))?;

        log::debug!(target: "app", "check candidate config in `{core_type}`");
        CoreInstance::check_config_(&core_type, &config_path, &binary_path, &app_dir)
            .await
            .context("failed to check runtime candidate")
    }

    async fn promote_runtime_snapshot_locked(
        &self,
        paths: &RuntimePaths,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> std::result::Result<Arc<RuntimeSnapshot>, RuntimeRestartError> {
        let candidate = paths
            .create_candidate(snapshot.product_bytes())
            .await
            .map_err(RuntimeRestartError::Prepare)?;
        let target_core = snapshot.target_core;
        let checked =
            check_and_promote_candidate(&candidate, paths.product(), |candidate_path| async move {
                self.check_candidate_path(&candidate_path, target_core)
                    .await
            })
            .await;
        if let Err(error) = candidate.cleanup().await {
            log::warn!(target: "app", "failed to clean runtime candidate: {error:?}");
        }
        let promoted_bytes = checked.map_err(|error| match error {
            CheckedPromotionError::Check(error) | CheckedPromotionError::Verify(error) => {
                RuntimeRestartError::Check(error)
            }
            CheckedPromotionError::Promote(error) => RuntimeRestartError::Promote(error),
        })?;
        if promoted_bytes.as_slice() != snapshot.product_bytes() {
            return Err(RuntimeRestartError::Promote(anyhow::anyhow!(
                "promoted runtime product differs from the prepared runtime snapshot"
            )));
        }
        self.lifecycle
            .runtime_lifecycle
            .publish_promoted(snapshot.clone());
        Ok(snapshot)
    }

    async fn begin_runtime_apply_transaction(
        &self,
        target_core: ClashCore,
    ) -> std::result::Result<RuntimeApplyTransaction, RuntimeRestartError> {
        let had_active_runtime = {
            let instance = self.instance.lock().as_ref().cloned();
            match instance {
                Some(instance) => {
                    matches!(instance.state().await.as_ref(), CoreState::Running)
                }
                None => false,
            }
        };
        let paths = RuntimePaths::from_app_config_dir().map_err(RuntimeRestartError::Prepare)?;
        if let Err(error) = paths
            .cleanup_stale_candidates(Duration::from_secs(24 * 60 * 60))
            .await
        {
            log::warn!(target: "app", "failed to clean stale runtime candidates: {error:?}");
        }
        let mut rollback = capture_runtime_transaction(&paths, &self.lifecycle.runtime_lifecycle)
            .await
            .map_err(RuntimeRestartError::Prepare)?;
        if !had_active_runtime {
            rollback.lifecycle.applied = None;
        }
        let recovery_target = rollback
            .lifecycle
            .applied
            .as_ref()
            .map(|snapshot| snapshot.target_core)
            .unwrap_or_else(Self::committed_core);
        let previous_clash = Config::clash().data().clone();
        Ok(RuntimeApplyTransaction {
            paths,
            rollback,
            previous_clash,
            recovery_target,
            target_core,
        })
    }

    async fn apply_prepared_runtime_locked(
        &self,
        transaction: &RuntimeApplyTransaction,
        snapshot: Arc<RuntimeSnapshot>,
        run_type: RunType,
    ) -> std::result::Result<(), RuntimeRestartError> {
        self.run_core_from_product_inner(
            transaction.paths.product(),
            transaction.target_core,
            run_type,
        )
        .await
        .map_err(RuntimeRestartError::Start)?;
        self.lifecycle
            .runtime_lifecycle
            .publish_applied(snapshot.clone())
            .map_err(RuntimeRestartError::Promote)?;
        *Config::runtime().draft() = crate::config::runtime::IRuntime {
            config: Some(snapshot.config.clone()),
        };
        Config::runtime().apply();
        Ok(())
    }

    async fn restore_after_restart_failure(
        &self,
        paths: &RuntimePaths,
        transaction: RuntimeTransactionSnapshot,
        previous_clash: crate::config::clash::IClashTemp,
        recovery_target: ClashCore,
    ) -> Result<()> {
        *Config::clash().data() = previous_clash;
        restore_failed_apply(
            paths,
            &self.lifecycle.runtime_lifecycle,
            transaction,
            |had_product| async move {
                if had_product {
                    self.run_core_from_product_inner(
                        paths.product(),
                        recovery_target,
                        RunType::Normal,
                    )
                    .await
                } else {
                    self.stop_running_instance().await?;
                    self.instance.lock().take();
                    Ok(())
                }
            },
        )
        .await
    }

    async fn apply_runtime_snapshot_locked(
        &self,
        snapshot: Arc<RuntimeSnapshot>,
        run_type: RunType,
    ) -> Result<()> {
        let transaction = self
            .begin_runtime_apply_transaction(snapshot.target_core)
            .await?;
        let primary = match self
            .promote_runtime_snapshot_locked(&transaction.paths, snapshot)
            .await
        {
            Ok(snapshot) => {
                self.apply_prepared_runtime_locked(&transaction, snapshot, run_type)
                    .await
            }
            Err(error) => Err(error),
        };

        match primary {
            Ok(()) => Ok(()),
            Err(primary) => {
                Config::runtime().discard();
                if !primary.requires_recovery() {
                    *Config::clash().data() = transaction.previous_clash;
                    return Err(primary.into());
                }
                match self
                    .restore_after_restart_failure(
                        &transaction.paths,
                        transaction.rollback,
                        transaction.previous_clash,
                        transaction.recovery_target,
                    )
                    .await
                {
                    Ok(()) => Err(primary.into()),
                    Err(recovery) => Err(RuntimeRestartError::Recovery {
                        primary: primary.to_string(),
                        recovery: recovery.to_string(),
                    }
                    .into()),
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub async fn change_default_network_dns(&self, enabled: bool) -> Result<()> {
        todo!()
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.lifecycle.recovery_notify.clone()
    }

    async fn stop_core_with_lease(&self, _lease: &CoreLifecycleLease<'_>) -> Result<()> {
        #[cfg(target_os = "macos")]
        let _ = self
            .change_default_network_dns(false)
            .await
            .inspect_err(|e| log::error!(target: "app", "failed to set system dns: {:?}", e));
        let instance = self.instance.lock().as_ref().cloned();
        if let Some(instance) = instance.as_ref()
            && matches!(instance.state().await.as_ref(), CoreState::Running)
        {
            instance.stop().await?;
        }
        self.instance.lock().take();
        self.lifecycle.runtime_lifecycle.clear_applied();
        Ok(())
    }
}

// TODO: support system path search via a config or flag
// FIXME: move this fn to nyanpasu-utils
/// Search the binary path of the core: Data Dir -> Sidecar Dir
pub fn find_binary_path(
    core_type: &chimera_utils::core::CoreType,
) -> std::io::Result<std::path::PathBuf> {
    let data_dir = dirs::app_data_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = data_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    let app_dir = dirs::app_install_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = app_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} not found", core_type.get_executable_name()),
    ))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::{CoreManager, Instance, RunType, RuntimeRestartError};
    use crate::{config::chimera::ClashCore, core::service::ipc::IpcState};

    #[test]
    fn run_type_classification_uses_only_explicit_inputs() {
        assert_eq!(
            RunType::classify(false, IpcState::Disconnected),
            RunType::Normal
        );
        assert_eq!(
            RunType::classify(false, IpcState::Connected),
            RunType::Normal
        );
        assert_eq!(
            RunType::classify(true, IpcState::Disconnected),
            RunType::Normal
        );
        assert_eq!(
            RunType::classify(true, IpcState::Connected),
            RunType::Service
        );
    }

    #[test]
    fn transform_failure_attempt_does_not_consume_applied_runtime_revision() {
        let manager = CoreManager::new();
        let preparation = manager.runtime_preparation();
        let error = anyhow::Error::new(crate::enhance::TransformFailureError::merge(
            "transform-failed".into(),
            Some("source-test".into()),
            anyhow::anyhow!("transform exploded"),
        ));

        preparation.record_prepare_failure(&error).unwrap();
        let failure = manager
            .runtime_transform_failure()
            .expect("transform failure must be published");
        assert_eq!(failure.attempt_revision.get(), 1);

        let applied_revision = manager.allocate_runtime_revision().unwrap();
        assert_eq!(
            applied_revision.get(),
            1,
            "prepare failure diagnostics must not advance the applied revision allocator"
        );
    }

    #[test]
    fn recovery_only_runs_after_product_or_core_may_have_changed() {
        assert!(!RuntimeRestartError::Prepare(anyhow::anyhow!("prepare")).requires_recovery());
        assert!(!RuntimeRestartError::Check(anyhow::anyhow!("check")).requires_recovery());
        assert!(RuntimeRestartError::Promote(anyhow::anyhow!("promote")).requires_recovery());
        assert!(RuntimeRestartError::Start(anyhow::anyhow!("start")).requires_recovery());
        assert!(
            RuntimeRestartError::Recovery {
                primary: "start".to_string(),
                recovery: "restart".to_string(),
            }
            .requires_recovery()
        );
    }

    #[test]
    fn local_instance_rejects_service_host() {
        let error = Instance::try_new(
            RunType::Service,
            ClashCore::Mihomo,
            PathBuf::from("runtime.yaml"),
            Arc::new(tokio::sync::Notify::new()),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cannot construct a Service-hosted instance")
        );
    }
}

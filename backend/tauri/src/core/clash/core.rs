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
use tracing::instrument;

use crate::{
    client::{
        SessionPortResolver,
        runtime::{
            CheckedPromotionError, RuntimeLifecycle, RuntimePaths, RuntimeRebuildGate,
            RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransactionSnapshot,
            RuntimeTransformFailure, capture_runtime_transaction, check_and_promote_candidate,
            restore_failed_apply,
        },
    },
    config::{chimera::ClashCore, clash::ClashInfo, core::Config},
    core::{
        clash::api,
        logger::Logger,
        service::core_host::{IpcServiceCoreHost, ServiceCoreHost, is_outcome_uncertain},
    },
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
enum RuntimeRestartError {
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
    fn outcome_uncertain(&self) -> bool {
        matches!(self, Self::Start(error) if is_outcome_uncertain(error))
    }

    /// Recovery is only required after the product may have changed or core apply began.
    fn requires_recovery(&self) -> bool {
        matches!(
            self,
            Self::Promote(_) | Self::Start(_) | Self::Recovery { .. }
        )
    }
}

#[derive(Debug)]
enum Instance {
    Child {
        child: Mutex<Arc<CoreInstance>>,
        stated_changed_at: Arc<AtomicI64>,
        kill_flag: Arc<AtomicBool>,
        recovery_notify: Arc<tokio::sync::Notify>,
    },
    Service {
        config_path: PathBuf,
        core_type: chimera_utils::core::CoreType,
        host: Arc<dyn ServiceCoreHost>,
    },
}

impl Instance {
    /// get core state with state changed timestamp
    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64) {
        match self {
            Instance::Child {
                child,
                stated_changed_at,
                ..
            } => {
                let this = child.lock();
                (
                    Cow::Borrowed(match this.state() {
                        chimera_utils::core::instance::CoreInstanceState::Running => {
                            &CoreState::Running
                        }
                        chimera_utils::core::instance::CoreInstanceState::Stopped => {
                            &CoreState::Stopped(None)
                        }
                    }),
                    stated_changed_at.load(Ordering::Relaxed),
                )
            }
            Instance::Service { host, .. } => match host.status().await {
                Ok((state, changed_at)) => (
                    Cow::Owned(match state {
                        CoreState::Running => CoreState::Running,
                        CoreState::Stopped(_) => CoreState::Stopped(None),
                    }),
                    changed_at,
                ),
                Err(_) => (Cow::Owned(CoreState::Stopped(None)), 0),
            },
        }
    }

    pub fn run_type(&self) -> RunType {
        match self {
            Instance::Child { .. } => RunType::Normal,
            Instance::Service { .. } => RunType::Service,
        }
    }

    pub async fn state<'a>(&self) -> Cow<'a, CoreState> {
        match self {
            Instance::Child { child, .. } => {
                let this = child.lock();
                Cow::Borrowed(match this.state() {
                    chimera_utils::core::instance::CoreInstanceState::Running => {
                        &CoreState::Running
                    }
                    chimera_utils::core::instance::CoreInstanceState::Stopped => {
                        &CoreState::Stopped(None)
                    }
                })
            }
            Instance::Service { host, .. } => {
                let status = host
                    .status()
                    .await
                    .map(|(state, _)| match state {
                        CoreState::Running => CoreState::Running,
                        CoreState::Stopped(_) => CoreState::Stopped(None),
                    })
                    .unwrap_or(CoreState::Stopped(None));
                Cow::Owned(status)
            }
        }
    }

    pub async fn recover(&self) -> Result<()> {
        match self {
            Instance::Child { .. } => {
                anyhow::bail!("local core recovery must use typed reconcile")
            }
            Instance::Service { host, .. } => host.recover().await,
        }
    }

    pub async fn stop(&self) -> Result<()> {
        let state = self.state().await;
        match self {
            Instance::Child {
                child,
                stated_changed_at,
                kill_flag,
                ..
            } => {
                if matches!(state.as_ref(), CoreState::Stopped(_)) {
                    anyhow::bail!("core is already stopped");
                }
                kill_flag.store(true, Ordering::Release);
                let child = {
                    let child = child.lock();
                    child.clone()
                };
                child.kill().await?;
                stated_changed_at.store(get_current_ts(), Ordering::Relaxed);
                Ok(())
            }
            Instance::Service { host, .. } => host.stop().await,
        }
    }

    pub fn try_new(
        run_type: RunType,
        clash_core: ClashCore,
        config_path: PathBuf,
        recovery_notify: Arc<tokio::sync::Notify>,
        service_host: Arc<dyn ServiceCoreHost>,
    ) -> Result<Self> {
        let core_type: chimera_utils::core::CoreType = (&clash_core).into();
        let service_core_type: chimera_utils::core::CoreType = (&clash_core).into();

        let data_dir = camino::Utf8PathBuf::from_path_buf(dirs::app_data_dir()?)
            .map_err(|e| anyhow::anyhow!("failed to convert data dir to utf8 path: {:?}", e))?;
        let binary = camino::Utf8PathBuf::from_path_buf(find_binary_path(&core_type)?)
            .map_err(|e| anyhow::anyhow!("failed to convert binary path to utf8 path: {:?}", e))?;
        let config_path = camino::Utf8PathBuf::from_path_buf(config_path)
            .map_err(|e| anyhow::anyhow!("failed to convert config path to utf8 path: {:?}", e))?;

        let pid_path = camino::Utf8PathBuf::from_path_buf(dirs::clash_pid_path()?)
            .map_err(|e| anyhow::anyhow!("failed to convert pid path to utf8 path: {:?}", e))?;
        match run_type {
            RunType::Normal => {
                let instance = Arc::new(
                    CoreInstanceBuilder::default()
                        .core_type(core_type)
                        .app_dir(data_dir)
                        .binary_path(binary)
                        .config_path(config_path.clone())
                        .pid_path(pid_path)
                        .build()?,
                );
                Ok(Instance::Child {
                    child: Mutex::new(instance),
                    kill_flag: Arc::new(AtomicBool::new(false)),
                    stated_changed_at: Arc::new(AtomicI64::new(get_current_ts())),
                    recovery_notify,
                })
            }
            RunType::Service => Ok(Instance::Service {
                config_path: config_path.into(),
                core_type: service_core_type,
                host: service_host,
            }),
            RunType::Elevated => {
                todo!()
            }
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            Instance::Child {
                child,
                kill_flag,
                stated_changed_at,
                recovery_notify,
            } => {
                let instance = {
                    let child = child.lock();
                    child.clone()
                };
                let (is_premium, core_type) = {
                    let child = child.lock();
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
                let (tx, mut rx) = tokio::sync::mpsc::channel::<anyhow::Result<()>>(1); // use mpsc channel just to avoid type moved error, though it never fails
                let stated_changed_at = stated_changed_at.clone();
                let kill_flag = kill_flag.clone();
                let recovery_notify = recovery_notify.clone();
                tracing::trace!("todo: instance start and may use admin performs better.");
                // This block below is to handle the stdio from the core process
                tokio::spawn(async move {
                    match instance.run().await {
                        Ok((_, mut rx)) => {
                            kill_flag.store(false, Ordering::Release); // reset kill flag
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
                                            let err = anyhow::anyhow!(format!(
                                                "{}\n{}",
                                                e,
                                                err_buf.join("\n")
                                            ));
                                            Logger::global().set_log(e);
                                            let _ = tx.send(Err(err)).await;
                                            stated_changed_at
                                                .store(get_current_ts(), Ordering::Relaxed);
                                            break;
                                        }
                                        CommandEvent::Terminated(status) => {
                                            log::error!(
                                                target: "app",
                                                "core terminated with status: {status:?}"
                                            );
                                            stated_changed_at
                                                .store(get_current_ts(), Ordering::Relaxed);
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
                                            stated_changed_at
                                                .store(get_current_ts(), Ordering::Relaxed);
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
            Instance::Service {
                config_path,
                core_type,
                host,
            } => host.start(config_path, core_type).await,
        }
    }
}

/// Exclusive guard for core lifecycle mutations.
#[must_use = "the lifecycle lease releases the mutex when dropped"]
pub(crate) struct CoreLifecycleLease<'a> {
    manager: &'a CoreManager,
    _guard: tokio::sync::MutexGuard<'a, ()>,
}

impl CoreLifecycleLease<'_> {
    pub(crate) async fn rebuild_running_config_with(
        &self,
        clash: ClashConfig,
        profiles: crate::config::profile::profiles::Profiles,
        target_core: ClashCore,
        run_type: RunType,
    ) -> Result<()> {
        self.manager
            .rebuild_and_run_locked_with(target_core, &clash, &profiles, run_type)
            .await
    }

    pub(crate) async fn stop_core(&self) -> Result<()> {
        self.manager.stop_core_with_lease(self).await
    }

    pub(crate) async fn recover_service_core(&self) -> Result<()> {
        let instance = {
            let instance = self.manager.instance.lock();
            instance.as_ref().cloned()
        };
        let Some(instance) = instance else {
            return Ok(());
        };
        anyhow::ensure!(
            instance.run_type() == RunType::Service,
            "service recover requires a Service-hosted core"
        );
        instance.recover().await
    }

    pub(crate) async fn change_core(
        &self,
        profiles: crate::config::profile::profiles::Profiles,
        clash_core: ClashCore,
    ) -> Result<()> {
        self.manager
            .change_core_with_lease(self, profiles, clash_core)
            .await
    }
}

#[derive(Debug)]
struct CoreLifecycleState {
    /// Single mutex domain for run/restart, stop, check, recover, and core changes.
    run_lock: RuntimeRebuildGate,
    runtime_lifecycle: RuntimeLifecycle,
    port_resolver: SessionPortResolver,
    recovery_notify: Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
pub struct CoreManager {
    instance: Mutex<Option<Arc<Instance>>>,
    lifecycle: CoreLifecycleState,
    service_host: Arc<dyn ServiceCoreHost>,
}

impl CoreManager {
    pub(crate) fn new() -> Self {
        Self::with_service_host(Arc::new(IpcServiceCoreHost))
    }

    fn with_service_host(service_host: Arc<dyn ServiceCoreHost>) -> Self {
        Self {
            instance: Mutex::new(None),
            lifecycle: CoreLifecycleState {
                run_lock: RuntimeRebuildGate::default(),
                runtime_lifecycle: RuntimeLifecycle::default(),
                port_resolver: SessionPortResolver::default(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            },
            service_host,
        }
    }

    pub(crate) async fn begin_lifecycle(&self) -> CoreLifecycleLease<'_> {
        CoreLifecycleLease {
            manager: self,
            _guard: self.lifecycle.run_lock.lock().await,
        }
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

    pub(crate) fn applied_runtime_identity(&self) -> Option<(u64, ClashCore)> {
        self.lifecycle
            .runtime_lifecycle
            .snapshot()
            .applied
            .map(|snapshot| (snapshot.revision.get(), snapshot.target_core))
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
            (
                Cow::Owned(CoreState::Stopped(None)),
                0_i64,
                RunType::default(),
            )
        }
    }

    fn committed_core() -> ClashCore {
        Config::verge()
            .data()
            .clash_core
            .unwrap_or(ClashCore::Mihomo)
    }

    fn committed_run_type() -> RunType {
        let enable_service = Config::verge().data().enable_service_mode.unwrap_or(false);
        RunType::from_service_mode(enable_service)
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
            self.service_host.clone(),
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

    async fn prepare_promoted_runtime_locked(
        &self,
        paths: &RuntimePaths,
        target_core: ClashCore,
        clash: &ClashConfig,
        profiles: &crate::config::profile::profiles::Profiles,
    ) -> std::result::Result<Arc<RuntimeSnapshot>, RuntimeRestartError> {
        Config::clash().reload();
        log::debug!(target: "app", "reloaded clash config from file");
        Config::clash()
            .latest()
            .prepare_external_controller_port()
            .map_err(RuntimeRestartError::Prepare)?;

        let resolved_ports = self
            .lifecycle
            .port_resolver
            .resolve(clash)
            .map_err(RuntimeRestartError::Prepare)?;

        let revision = self
            .lifecycle
            .runtime_lifecycle
            .allocate_revision()
            .map_err(RuntimeRestartError::Prepare)?;
        let (config, exists_keys, transform_output, inspection) =
            match Config::generate_runtime_output_with_ports(
                clash,
                profiles,
                target_core,
                resolved_ports,
            )
            .await
            {
                Ok(output) => (
                    output.config,
                    output.exists_keys,
                    output.postprocessing_output,
                    output.inspection,
                ),
                Err(error) => {
                    if let Some(transform) = error.downcast_ref::<TransformFailureError>() {
                        self.lifecycle.runtime_lifecycle.publish_transform_failure(
                            RuntimeTransformFailure {
                                attempt_revision: revision,
                                transform_uid: transform.transform_uid.clone(),
                                scope_uid: transform.scope_uid.clone(),
                                script_type: transform.script_type,
                                message: transform.message(),
                            },
                        );
                    }
                    return Err(RuntimeRestartError::Prepare(error));
                }
            };
        self.lifecycle.runtime_lifecycle.clear_transform_failure();
        let bytes = Config::render_runtime_bytes(&config).map_err(RuntimeRestartError::Prepare)?;
        let candidate = paths
            .create_candidate(&bytes)
            .await
            .map_err(RuntimeRestartError::Prepare)?;

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
        let snapshot = Arc::new(RuntimeSnapshot::from_data(
            revision,
            target_core,
            promoted_bytes.into(),
            RuntimeSnapshotData {
                config,
                exists_keys,
                postprocessing_output: transform_output,
                inspection: Arc::new(inspection),
            },
        ));
        self.lifecycle
            .runtime_lifecycle
            .publish_promoted(snapshot.clone());
        Ok(snapshot)
    }

    async fn promote_and_start_locked(
        &self,
        paths: &RuntimePaths,
        target_core: ClashCore,
        clash: &ClashConfig,
        profiles: &crate::config::profile::profiles::Profiles,
        run_type: RunType,
    ) -> std::result::Result<(), RuntimeRestartError> {
        let snapshot = self
            .prepare_promoted_runtime_locked(paths, target_core, clash, profiles)
            .await?;
        self.run_core_from_product_inner(paths.product(), target_core, run_type)
            .await
            .map_err(RuntimeRestartError::Start)?;
        self.lifecycle
            .runtime_lifecycle
            .publish_applied(snapshot)
            .map_err(RuntimeRestartError::Promote)?;
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
                        Self::committed_run_type(),
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

    async fn rebuild_and_run_locked_with(
        &self,
        target_core: ClashCore,
        clash: &ClashConfig,
        profiles: &crate::config::profile::profiles::Profiles,
        run_type: RunType,
    ) -> Result<()> {
        let paths = RuntimePaths::from_app_config_dir().map_err(RuntimeRestartError::Prepare)?;
        if let Err(error) = paths
            .cleanup_stale_candidates(Duration::from_secs(24 * 60 * 60))
            .await
        {
            log::warn!(target: "app", "failed to clean stale runtime candidates: {error:?}");
        }
        let transaction = capture_runtime_transaction(&paths, &self.lifecycle.runtime_lifecycle)
            .await
            .map_err(RuntimeRestartError::Prepare)?;
        let recovery_target = transaction
            .lifecycle
            .applied
            .as_ref()
            .map(|snapshot| snapshot.target_core)
            .unwrap_or_else(Self::committed_core);
        let previous_clash = Config::clash().data().clone();

        match self
            .promote_and_start_locked(&paths, target_core, clash, profiles, run_type)
            .await
        {
            Ok(()) => Ok(()),
            Err(primary) => {
                Config::runtime().discard();
                if primary.outcome_uncertain() {
                    return Err(primary.into());
                }
                if !primary.requires_recovery() {
                    *Config::clash().data() = previous_clash;
                    return Err(primary.into());
                }
                match self
                    .restore_after_restart_failure(
                        &paths,
                        transaction,
                        previous_clash,
                        recovery_target,
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
        let instance = {
            let instance = self.instance.lock();
            instance.as_ref().cloned()
        };
        if let Some(instance) = instance.as_ref() {
            instance.stop().await?;
        }
        Ok(())
    }

    /// 切换核心
    #[instrument(skip(self, _lease, profiles))]
    async fn change_core_with_lease(
        &self,
        _lease: &CoreLifecycleLease<'_>,
        profiles: crate::config::profile::profiles::Profiles,
        clash_core: ClashCore,
    ) -> Result<()> {
        log::debug!(target: "app", "change core to `{clash_core}`");
        Config::verge().draft().clash_core = Some(clash_core);

        // 清掉旧日志
        Logger::global().clear_log();

        let clash = crate::bridge::clash::clash_config_from_legacy(
            &Config::verge().latest(),
            &Config::clash().latest().0,
        )?;
        match self
            .rebuild_and_run_locked_with(clash_core, &clash, &profiles, RunType::default())
            .await
        {
            Ok(_) => {
                tracing::info!("change core success");
                Config::verge().apply();
                log_err!(Config::verge().latest().save_file());
                Ok(())
            }
            Err(err) => {
                tracing::error!("failed to change core: {err:?}");
                Config::verge().discard();
                Config::runtime().discard();
                Err(err)
            }
        }
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

    use async_trait::async_trait;
    use chimera_ipc::api::status::CoreState;

    use super::{Instance, RunType, RuntimeRestartError};
    use crate::{
        config::chimera::ClashCore,
        core::service::{core_host::ServiceCoreHost, ipc::IpcState},
    };

    #[derive(Debug, Default)]
    struct RecordingServiceCoreHost {
        calls: parking_lot::Mutex<Vec<&'static str>>,
    }

    #[async_trait]
    impl ServiceCoreHost for RecordingServiceCoreHost {
        async fn api_connection(
            &self,
        ) -> anyhow::Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>> {
            self.calls.lock().push("api");
            Ok(None)
        }

        async fn status(&self) -> anyhow::Result<(CoreState, i64)> {
            self.calls.lock().push("status");
            Ok((CoreState::Running, 42))
        }

        async fn start(
            &self,
            _config_path: &std::path::Path,
            _core_type: &chimera_utils::core::CoreType,
        ) -> anyhow::Result<()> {
            self.calls.lock().push("start");
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.calls.lock().push("stop");
            Ok(())
        }

        async fn recover(&self) -> anyhow::Result<()> {
            self.calls.lock().push("recover");
            Ok(())
        }
    }

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

    #[tokio::test]
    async fn service_instance_delegates_to_injected_core_host() {
        let host = Arc::new(RecordingServiceCoreHost::default());
        let instance = Instance::Service {
            config_path: PathBuf::from("runtime.yaml"),
            core_type: (&ClashCore::Mihomo).into(),
            host: host.clone(),
        };

        let (state, changed_at) = instance.status().await;
        assert!(matches!(state.as_ref(), CoreState::Running));
        assert_eq!(changed_at, 42);

        instance.start().await.unwrap();
        instance.recover().await.unwrap();
        instance.stop().await.unwrap();

        assert_eq!(
            host.calls.lock().as_slice(),
            ["status", "start", "recover", "status", "stop"]
        );
    }
}

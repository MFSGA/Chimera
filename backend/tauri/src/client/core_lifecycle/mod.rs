//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Chimera currently routes execution to the legacy CoreManager adapter while
//! lifecycle mutation admission is serialized through a client-owned mailbox.

pub(crate) mod adapters;
pub(crate) mod ports;
mod workflow;

use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::{
    ChimeraClient, application::ApplicationClient, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};
use workflow::{Command, CoreLifecycleWorkflow};

const CALL_WAIT: Duration = Duration::from_secs(180);
const DIRTY_WINDOW: Duration = Duration::from_millis(500);
const MAX_PENDING: usize = 32;
const COMPLETED_LIMIT: usize = 32;

type OperationId = u64;

#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub(crate) struct CoreLifecycleStatus {
    pub(crate) active: Option<OperationId>,
    pub(crate) queued: Vec<OperationId>,
    pub(crate) uncertain: bool,
    pub(crate) shutting_down: bool,
    pub(crate) completed: Vec<CoreLifecycleOperationResult>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct CoreLifecycleOperationResult {
    pub(crate) id: OperationId,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServicePhase {
    Probing,
    NotInstalled,
    DaemonStopped,
    Installing,
    StartingDaemon,
    Ready,
    Incompatible,
    Restarting,
    Uninstalling,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct ServiceHostStatus {
    pub(crate) name: std::borrow::Cow<'static, str>,
    pub(crate) version: std::borrow::Cow<'static, str>,
    pub(crate) status: chimera_ipc::types::ServiceStatus,
    pub(crate) server: Option<chimera_ipc::api::status::StatusResBody<'static>>,
    pub(crate) phase: ServicePhase,
    pub(crate) compat: crate::core::service::compat::ServiceCompat,
    pub(crate) runtime_owned: bool,
    pub(crate) restart_attempts: u8,
}

impl ServiceHostStatus {
    fn probing() -> Self {
        Self {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed(""),
            status: chimera_ipc::types::ServiceStatus::NotInstalled,
            server: None,
            phase: ServicePhase::Probing,
            compat: crate::core::service::compat::ServiceCompat::Unknown,
            runtime_owned: false,
            restart_attempts: 0,
        }
    }

    fn from_probe(info: chimera_ipc::types::StatusInfo<'static>) -> Self {
        use chimera_ipc::types::ServiceStatus;

        let compat = crate::core::service::compat::ServiceCompat::classify(&info);
        let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
        let phase = match info.status {
            ServiceStatus::Running if compat.allows_service_backend() => ServicePhase::Ready,
            ServiceStatus::Running => ServicePhase::Incompatible,
            ServiceStatus::Stopped => ServicePhase::DaemonStopped,
            ServiceStatus::NotInstalled => ServicePhase::NotInstalled,
        };
        Self {
            name: info.name,
            version: info.version,
            status: info.status,
            server: info.server,
            phase,
            compat,
            runtime_owned,
            restart_attempts: 0,
        }
    }

    fn probe_failed(previous: &Self) -> Self {
        Self {
            name: previous.name.clone(),
            version: previous.version.clone(),
            status: previous.status,
            server: previous.server.clone(),
            phase: ServicePhase::Unknown,
            compat: crate::core::service::compat::ServiceCompat::Unknown,
            runtime_owned: false,
            restart_attempts: previous.restart_attempts,
        }
    }
}

#[allow(unused_imports)]
pub(crate) use adapters::{
    FsBinaryInstaller, LegacyCoreBridge, LegacyRunningConfigBridge, LegacyServiceBridge,
};
#[allow(unused_imports)]
pub(crate) use ports::{
    BinaryInstallProgress, CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot,
    PreparedCoreBinary, RunningConfigPort, RuntimeTransformDiagnostics, ServiceLifecyclePort,
    ServiceTransitionLease,
};

enum Message {
    Execute {
        id: OperationId,
        command: Command,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    #[cfg(test)]
    ProbeService {
        reply: RpcReplyPort<anyhow::Result<ServiceHostStatus>>,
    },
    ObserveService {
        info: chimera_ipc::types::StatusInfo<'static>,
    },
    ServiceProbeFailed,
    #[cfg(not(test))]
    RefreshServiceStatus,
    RecoverCore,
    StartupReconcile,
    RuntimeDirty,
    DirtyTick,
}

struct CoreLifecycleActor;

struct CoreLifecycleActorState {
    workflow: CoreLifecycleWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: tokio::sync::watch::Sender<ServiceHostStatus>,
    uncertain: bool,
    shutting_down: bool,
    shutdown_result: Option<Option<String>>,
    dirty: bool,
    dirty_tick_scheduled: bool,
}

struct CoreLifecycleActorArgs {
    workflow: CoreLifecycleWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: tokio::sync::watch::Sender<ServiceHostStatus>,
}

impl CoreLifecycleActorState {
    fn allocate_operation_id(&self) -> OperationId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn publish_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        self.service_status
            .send_replace(ServiceHostStatus::from_probe(info));
    }

    fn publish_service_phase(&self, phase: ServicePhase) {
        let current = self.service_status.borrow().clone();
        self.service_status.send_replace(ServiceHostStatus {
            name: current.name,
            version: current.version,
            status: current.status,
            server: current.server,
            phase,
            compat: crate::core::service::compat::ServiceCompat::Unknown,
            runtime_owned: current.runtime_owned,
            restart_attempts: current.restart_attempts,
        });
    }

    fn publish_service_probe_failure(&self) {
        let previous = self.service_status.borrow().clone();
        self.service_status
            .send_replace(ServiceHostStatus::probe_failed(&previous));
    }

    async fn refresh_service_status(&self) -> anyhow::Result<ServiceHostStatus> {
        match self.workflow.probe_service().await {
            Ok(info) => {
                let status = ServiceHostStatus::from_probe(info);
                self.service_status.send_replace(status.clone());
                Ok(status)
            }
            Err(error) => {
                self.publish_service_probe_failure();
                Err(error)
            }
        }
    }

    fn mark_runtime_dirty(&mut self, myself: &ActorRef<Message>) {
        if self.uncertain || self.shutting_down {
            return;
        }
        self.dirty = true;
        if !self.dirty_tick_scheduled {
            self.dirty_tick_scheduled = true;
            let actor = myself.clone();
            tokio::spawn(async move {
                tokio::time::sleep(DIRTY_WINDOW).await;
                let _ = actor.cast(Message::DirtyTick);
            });
        }
    }

    async fn execute_operation(&mut self, id: OperationId, command: Command) -> anyhow::Result<()> {
        let shutdown = matches!(&command, Command::Shutdown);
        if shutdown {
            if let Some(error) = self.shutdown_result.clone() {
                let result = match error {
                    Some(error) => Err(anyhow::anyhow!(error)),
                    None => Ok(()),
                };
                let mut status = self.status.lock();
                status.queued.retain(|queued| *queued != id);
                if status.completed.len() == COMPLETED_LIMIT {
                    status.completed.remove(0);
                }
                status.completed.push(CoreLifecycleOperationResult {
                    id,
                    error: result.as_ref().err().map(ToString::to_string),
                });
                return result;
            }
            self.shutting_down = true;
            self.dirty = false;
            self.status.lock().shutting_down = true;
        } else if self.shutting_down {
            let error = anyhow::anyhow!("core lifecycle is shutting down");
            let mut status = self.status.lock();
            status.queued.retain(|queued| *queued != id);
            if status.completed.len() == COMPLETED_LIMIT {
                status.completed.remove(0);
            }
            status.completed.push(CoreLifecycleOperationResult {
                id,
                error: Some(error.to_string()),
            });
            return Err(error);
        } else if self.uncertain {
            let error = anyhow::anyhow!(
                "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
            );
            let mut status = self.status.lock();
            status.queued.retain(|queued| *queued != id);
            if status.completed.len() == COMPLETED_LIMIT {
                status.completed.remove(0);
            }
            status.completed.push(CoreLifecycleOperationResult {
                id,
                error: Some(error.to_string()),
            });
            return Err(error);
        }

        {
            let mut status = self.status.lock();
            status.queued.retain(|queued| *queued != id);
            status.active = Some(id);
        }
        let progress = match &command {
            Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
            _ => None,
        };
        let result = match AssertUnwindSafe(self.workflow.execute(command))
            .catch_unwind()
            .await
        {
            Ok(result) => result,
            Err(_) => {
                self.uncertain = true;
                Err(anyhow::anyhow!(
                    "core lifecycle workflow panicked; execution state is uncertain"
                ))
            }
        };
        if let Some(progress) = progress {
            let error = result.as_ref().err().map(ToString::to_string);
            if std::panic::catch_unwind(AssertUnwindSafe(|| progress.finished(error.as_deref())))
                .is_err()
            {
                tracing::error!("binary installation progress observer panicked");
            }
        }
        if shutdown {
            self.shutdown_result = Some(result.as_ref().err().map(ToString::to_string));
        }
        {
            let mut status = self.status.lock();
            status.active = None;
            status.uncertain = self.uncertain;
            status.shutting_down = self.shutting_down;
            if status.completed.len() == COMPLETED_LIMIT {
                status.completed.remove(0);
            }
            status.completed.push(CoreLifecycleOperationResult {
                id,
                error: result.as_ref().err().map(ToString::to_string),
            });
        }
        result
    }
}

impl Actor for CoreLifecycleActor {
    type Msg = Message;
    type State = CoreLifecycleActorState;
    type Arguments = CoreLifecycleActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(CoreLifecycleActorState {
            workflow: args.workflow,
            next_id: args.next_id,
            status: args.status,
            service_status: args.service_status,
            uncertain: false,
            shutting_down: false,
            shutdown_result: None,
            dirty: false,
            dirty_tick_scheduled: false,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            #[cfg(test)]
            Message::ProbeService { reply } => {
                let _ = reply.send(state.refresh_service_status().await);
            }
            Message::ObserveService { info } => {
                state.publish_service_status(info);
            }
            Message::ServiceProbeFailed => {
                state.publish_service_probe_failure();
            }
            #[cfg(not(test))]
            Message::RefreshServiceStatus => {
                if let Err(error) = state.refresh_service_status().await {
                    tracing::debug!(%error, "failed to refresh cached service status");
                }
            }
            Message::Execute { id, command, reply } => {
                let service_phase = match &command {
                    Command::InstallService => Some(ServicePhase::Installing),
                    Command::StartService => Some(ServicePhase::StartingDaemon),
                    Command::RestartService => Some(ServicePhase::Restarting),
                    Command::UninstallService => Some(ServicePhase::Uninstalling),
                    _ => None,
                };
                let service_mutation = matches!(
                    &command,
                    Command::InstallService
                        | Command::UninstallService
                        | Command::UpdateService
                        | Command::StartService
                        | Command::RestartService
                        | Command::StopService
                );
                if let Some(phase) = service_phase {
                    state.publish_service_phase(phase);
                }
                let retry_reconcile_on_failure = matches!(
                    &command,
                    Command::StartService
                        | Command::RestartService
                        | Command::StopService
                        | Command::UninstallService
                        | Command::UpdateService
                );
                let result = state.execute_operation(id, command).await;
                if service_mutation && let Err(error) = state.refresh_service_status().await {
                    tracing::debug!(%error, "failed to refresh service status after mutation");
                }
                if retry_reconcile_on_failure && result.is_err() && !state.uncertain {
                    state.mark_runtime_dirty(&myself);
                }
                let _ = reply.send(result);
            }
            Message::StartupReconcile => {
                if state.uncertain || state.shutting_down {
                    tracing::warn!(
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
                        "ignoring startup reconcile because lifecycle is unavailable"
                    );
                    return Ok(());
                }
                let id = state.allocate_operation_id();
                if let Err(error) = state.execute_operation(id, Command::Reconcile).await {
                    tracing::error!(%error, id, "startup core reconcile failed");
                }
            }
            Message::RecoverCore => {
                if state.uncertain || state.shutting_down {
                    tracing::warn!(
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
                        "ignoring crash recovery because lifecycle is unavailable"
                    );
                    return Ok(());
                }
                tracing::info!("trying to recover core through lifecycle actor");
                let id = state.allocate_operation_id();
                if let Err(error) = state.execute_operation(id, Command::RecoverCore).await {
                    if !state.uncertain {
                        tracing::error!(%error, "failed to recover core; scheduling retry");
                        let actor = myself.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            let _ = actor.cast(Message::RecoverCore);
                        });
                    }
                }
            }
            Message::RuntimeDirty => {
                if state.uncertain || state.shutting_down {
                    tracing::debug!(
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
                        "ignoring runtime dirty signal because lifecycle is unavailable"
                    );
                    return Ok(());
                }
                state.mark_runtime_dirty(&myself);
            }
            Message::DirtyTick => {
                state.dirty_tick_scheduled = false;
                if state.uncertain || state.shutting_down {
                    state.dirty = false;
                    return Ok(());
                }
                if state.dirty {
                    state.dirty = false;
                    let id = state.allocate_operation_id();
                    if let Err(error) = state.execute_operation(id, Command::Reconcile).await {
                        tracing::warn!(%error, id, "coalesced background runtime rebuild failed");
                    }
                }
            }
        }
        Ok(())
    }
}

enum CoreLifecycleClientInner {
    Actor {
        actor_ref: ActorRef<Message>,
        next_id: Arc<AtomicU64>,
        status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
        service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
    },
    #[cfg(test)]
    Direct {
        workflow: tokio::sync::Mutex<CoreLifecycleWorkflow>,
        service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
    },
}

#[derive(Clone)]
pub(super) struct CoreLifecycleClient(Arc<CoreLifecycleClientInner>);

impl CoreLifecycleClient {
    pub(super) async fn spawn(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> anyhow::Result<Self> {
        let client = Self::spawn_with_installer(
            core,
            application,
            clash,
            runtime_paths,
            Arc::new(FsBinaryInstaller),
            Arc::new(LegacyServiceBridge),
        )
        .await?;
        #[cfg(not(test))]
        client.request_service_status_refresh();
        Ok(client)
    }

    async fn spawn_with_installer(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> anyhow::Result<Self> {
        let recovery_notify = core.recovery_notify();
        let workflow =
            CoreLifecycleWorkflow::new(application, clash, core, installer, runtime_paths, service);
        let status = Arc::new(parking_lot::Mutex::new(CoreLifecycleStatus::default()));
        let next_id = Arc::new(AtomicU64::new(1));
        let (service_status_tx, service_status_rx) =
            tokio::sync::watch::channel(ServiceHostStatus::probing());
        let (actor_ref, _handle) = Actor::spawn(
            None,
            CoreLifecycleActor,
            CoreLifecycleActorArgs {
                workflow,
                next_id: next_id.clone(),
                status: status.clone(),
                service_status: service_status_tx,
            },
        )
        .await?;
        if let Some(recovery_notify) = recovery_notify {
            let recovery_actor = actor_ref.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    recovery_notify.notified().await;
                    if recovery_actor.cast(Message::RecoverCore).is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Self(Arc::new(CoreLifecycleClientInner::Actor {
            actor_ref,
            next_id,
            status,
            service_status: service_status_rx,
        })))
    }

    #[cfg(test)]
    pub(super) fn direct(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> Self {
        let (_service_status_tx, service_status_rx) =
            tokio::sync::watch::channel(ServiceHostStatus::probing());
        Self(Arc::new(CoreLifecycleClientInner::Direct {
            workflow: tokio::sync::Mutex::new(CoreLifecycleWorkflow::new(
                application,
                clash,
                core,
                Arc::new(FsBinaryInstaller),
                runtime_paths,
                Arc::new(LegacyServiceBridge),
            )),
            service_status: service_status_rx,
        }))
    }

    async fn execute(&self, command: Command) -> anyhow::Result<()> {
        self.execute_with_timeout(command, CALL_WAIT).await
    }

    async fn execute_with_timeout(
        &self,
        command: Command,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref,
                next_id,
                status,
                ..
            } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                let shutdown = matches!(&command, Command::Shutdown);
                let rejection = {
                    let mut lifecycle = status.lock();
                    let error = if lifecycle.shutting_down && !shutdown {
                        Some("core lifecycle is shutting down".to_string())
                    } else if lifecycle.uncertain && !shutdown {
                        Some(
                            "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                                .to_string(),
                        )
                    } else if lifecycle.queued.len() >= MAX_PENDING {
                        Some("core lifecycle queue is full".to_string())
                    } else {
                        lifecycle.queued.push(id);
                        None
                    };
                    if let Some(error) = error.as_ref() {
                        if lifecycle.completed.len() == COMPLETED_LIMIT {
                            lifecycle.completed.remove(0);
                        }
                        lifecycle.completed.push(CoreLifecycleOperationResult {
                            id,
                            error: Some(error.clone()),
                        });
                    }
                    error
                };
                if let Some(error) = rejection {
                    anyhow::bail!(error);
                }
                match actor_ref
                    .call(
                        |reply| Message::Execute { id, command, reply },
                        Some(timeout),
                    )
                    .await
                {
                    Ok(CallResult::Success(result)) => result,
                    Ok(CallResult::Timeout) => anyhow::bail!(
                        "core lifecycle operation {id} timed out; it may still be queued or running"
                    ),
                    Ok(CallResult::SenderError) => {
                        status.lock().queued.retain(|queued| *queued != id);
                        anyhow::bail!(
                            "core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                        )
                    }
                    Err(error) => {
                        status.lock().queued.retain(|queued| *queued != id);
                        Err(error.into())
                    }
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow, .. } => {
                workflow.lock().await.execute(command).await
            }
        }
    }

    pub(super) fn status(&self) -> CoreLifecycleStatus {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { status, .. } => status.lock().clone(),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => CoreLifecycleStatus::default(),
        }
    }

    pub(super) fn request_startup_reconcile(&self) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => actor_ref
                .cast(Message::StartupReconcile)
                .map_err(|error| anyhow::anyhow!("failed to enqueue startup reconcile: {error}")),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => {
                anyhow::bail!("startup reconcile requires the lifecycle actor")
            }
        }
    }

    #[cfg(not(test))]
    pub(super) fn request_service_status_refresh(&self) {
        if let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref()
            && actor_ref.cast(Message::RefreshServiceStatus).is_err()
        {
            tracing::debug!("failed to enqueue service status refresh");
        }
    }

    pub(super) fn request_runtime_rebuild(&self) {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => {
                if actor_ref.cast(Message::RuntimeDirty).is_err() {
                    tracing::warn!("failed to enqueue background runtime rebuild");
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => {}
        }
    }

    #[cfg(test)]
    pub(super) async fn probe_service(&self) -> anyhow::Result<ServiceHostStatus> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => match actor_ref
                .call(|reply| Message::ProbeService { reply }, None)
                .await
            {
                Ok(CallResult::Success(result)) => result,
                Ok(CallResult::Timeout) => anyhow::bail!("service probe timed out"),
                Ok(CallResult::SenderError) => anyhow::bail!("service probe reply dropped"),
                Err(error) => Err(error.into()),
            },
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow, .. } => workflow
                .lock()
                .await
                .probe_service()
                .await
                .map(ServiceHostStatus::from_probe),
        }
    }

    pub(super) fn service_status(&self) -> ServiceHostStatus {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { service_status, .. } => {
                service_status.borrow().clone()
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { service_status, .. } => {
                service_status.borrow().clone()
            }
        }
    }

    pub(super) fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        if let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref()
            && actor_ref.cast(Message::ObserveService { info }).is_err()
        {
            tracing::debug!("failed to publish service status observation");
        }
    }

    pub(super) fn observe_service_probe_failure(&self) {
        if let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref()
            && actor_ref.cast(Message::ServiceProbeFailed).is_err()
        {
            tracing::debug!("failed to publish service probe failure");
        }
    }

    pub(super) async fn shutdown(&self) -> anyhow::Result<()> {
        self.execute(Command::Shutdown).await
    }

    pub(super) async fn stop_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StopCore).await
    }

    pub(super) async fn select_core(
        &self,
        core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.execute(Command::SelectCore(core)).await
    }

    pub(super) async fn reconcile(&self) -> anyhow::Result<()> {
        self.execute(Command::Reconcile).await
    }

    pub(super) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReplaceCoreBinary(artifact)).await
    }

    pub(super) async fn install_service(&self) -> anyhow::Result<()> {
        self.execute(Command::InstallService).await
    }

    pub(super) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.execute(Command::UninstallService).await
    }

    pub(super) async fn update_service(&self) -> anyhow::Result<()> {
        self.execute(Command::UpdateService).await
    }

    pub(super) async fn start_service(&self) -> anyhow::Result<()> {
        self.execute(Command::StartService).await
    }

    pub(super) async fn restart_service(&self) -> anyhow::Result<()> {
        self.execute(Command::RestartService).await
    }

    pub(super) async fn stop_service(&self) -> anyhow::Result<()> {
        self.execute(Command::StopService).await
    }
}

impl ChimeraClient {
    pub(crate) fn init_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.request_startup_reconcile()
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.core.status().await
    }

    pub(crate) fn service_status(&self) -> ServiceHostStatus {
        self.inner.core_lifecycle.service_status()
    }

    pub(crate) fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        self.inner.core_lifecycle.observe_service_status(info);
    }

    pub(crate) fn observe_service_probe_failure(&self) {
        self.inner.core_lifecycle.observe_service_probe_failure();
    }

    pub(crate) fn core_lifecycle_status(&self) -> CoreLifecycleStatus {
        self.inner.core_lifecycle.status()
    }

    pub(crate) fn request_runtime_rebuild(&self) {
        self.inner.core_lifecycle.request_runtime_rebuild();
    }

    pub(crate) fn runtime_transform_diagnostics(
        &self,
    ) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        self.inner.core.runtime_transform_diagnostics()
    }

    pub(crate) fn promoted_runtime_snapshot(
        &self,
    ) -> Option<Arc<crate::client::runtime::RuntimeSnapshot>> {
        self.inner.core.promoted_runtime_snapshot()
    }

    pub(crate) async fn change_core(
        &self,
        clash_core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.inner.core_lifecycle.select_core(clash_core).await
    }

    pub(crate) async fn shutdown_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.shutdown().await
    }

    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.stop_core().await
    }

    pub(crate) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.inner
            .core_lifecycle
            .replace_core_binary(artifact)
            .await
    }

    pub(crate) async fn install_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.install_service().await
    }

    pub(crate) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.uninstall_service().await
    }

    pub(crate) async fn update_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.update_service().await
    }

    pub(crate) async fn start_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.start_service().await
    }

    pub(crate) async fn restart_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.restart_service().await
    }

    pub(crate) async fn stop_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.stop_service().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    };

    use async_trait::async_trait;
    use chimera_config::clash::config::ClashConfig;
    use chimera_ipc::api::status::CoreState;

    use super::*;
    use crate::{config::chimera::ClashCore, core::clash::core::RunType};

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        recovery_notify: Arc<tokio::sync::Notify>,
    }

    struct RecordingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct PanicOnStopCore {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct PanicOnStopLease {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct BlockingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<tokio::sync::Notify>,
        release_stop: Arc<tokio::sync::Notify>,
    }

    struct BlockingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<tokio::sync::Notify>,
        release_stop: Arc<tokio::sync::Notify>,
    }

    struct ServiceHandoffCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        local: Arc<AtomicBool>,
    }

    struct ServiceHandoffLease {
        events: Arc<Mutex<Vec<&'static str>>>,
        local: Arc<AtomicBool>,
    }

    struct ServiceStartCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        service: Arc<AtomicBool>,
    }

    struct ServiceStartLease {
        events: Arc<Mutex<Vec<&'static str>>>,
        service: Arc<AtomicBool>,
    }

    struct RecordingService {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingServiceTransition {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingInstaller {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl ServiceTransitionLease for RecordingServiceTransition {
        async fn install_daemon(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-install");
            Ok(())
        }

        async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-uninstall");
            Ok(())
        }

        async fn update_daemon(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-update");
            Ok(())
        }

        async fn start_daemon(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-start");
            Ok(())
        }

        async fn restart_daemon(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-restart");
            Ok(())
        }

        async fn stop_daemon(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-stop");
            Ok(())
        }

        async fn confirm_ready(&mut self, _timeout: Duration) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-ready");
            Ok(())
        }

        async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-confirm");
            Ok(())
        }
    }

    #[async_trait]
    impl ServiceLifecyclePort for RecordingService {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.events.lock().unwrap().push("service-probe");
            Ok(chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("1.0.0"),
                status: chimera_ipc::types::ServiceStatus::Stopped,
                server: None,
            })
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.events.lock().unwrap().push("service-begin");
            Ok(Box::new(RecordingServiceTransition {
                events: self.events.clone(),
            }))
        }
    }

    #[async_trait]
    impl ports::BinaryInstaller for RecordingInstaller {
        async fn install(&self, _artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("install");
            Ok(())
        }
    }

    struct RecordingProgress {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl BinaryInstallProgress for RecordingProgress {
        fn restarting(&self) {
            self.events.lock().unwrap().push("restarting");
        }

        fn finished(&self, error: Option<&str>) {
            assert!(error.is_none());
            self.events.lock().unwrap().push("finished");
        }
    }

    #[async_trait]
    impl CoreLifecycleLease for RecordingLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("run");
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop-start");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.events.lock().unwrap().push("stop-end");
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecycleLease for PanicOnStopLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild-after-panic");
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("panic-stop");
            panic!("injected lifecycle panic");
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select-after-panic");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for PanicOnStopCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
            Ok(Box::new(PanicOnStopLease {
                events: self.events.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecycleLease for BlockingLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("blocking-stop");
            self.stop_started.notify_one();
            self.release_stop.notified().await;
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("queued-select");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for BlockingCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
            Ok(Box::new(BlockingLease {
                events: self.events.clone(),
                stop_started: self.stop_started.clone(),
                release_stop: self.release_stop.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecycleLease for ServiceHandoffLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild-local");
            self.local.store(true, AtomicOrdering::Relaxed);
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for ServiceHandoffCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
            Ok(Box::new(ServiceHandoffLease {
                events: self.events.clone(),
                local: self.local.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Running,
                state_changed_at: 0,
                run_type: if self.local.load(AtomicOrdering::Relaxed) {
                    RunType::Normal
                } else {
                    RunType::Service
                },
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecycleLease for ServiceStartLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild-service");
            self.service.store(true, AtomicOrdering::Relaxed);
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for ServiceStartCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
            Ok(Box::new(ServiceStartLease {
                events: self.events.clone(),
                service: self.service.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Running,
                state_changed_at: 0,
                run_type: if self.service.load(AtomicOrdering::Relaxed) {
                    RunType::Service
                } else {
                    RunType::Normal
                },
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
            Ok(Box::new(RecordingLease {
                events: self.events.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            Some(self.recovery_notify.clone())
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[tokio::test]
    async fn mailbox_serializes_lifecycle_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let stop_client = client.clone();
        let stop = tokio::spawn(async move { stop_client.stop_core().await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        client.select_core(ClashCore::Mihomo).await.unwrap();
        stop.await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end", "select"]
        );
    }

    #[tokio::test]
    async fn shutdown_is_idempotent_and_blocks_follow_up_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        client.shutdown().await.unwrap();
        client.shutdown().await.unwrap();

        let status = client.status();
        assert!(status.shutting_down);
        let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("core lifecycle is shutting down")
        );
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end"]
        );
    }

    #[tokio::test]
    async fn pending_queue_rejects_requests_above_bound() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(tokio::sync::Notify::new());
        let release_stop = Arc::new(tokio::sync::Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(BlockingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let stop_client = client.clone();
        let stop = tokio::spawn(async move { stop_client.stop_core().await });
        tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
            .await
            .expect("stop should enter the lifecycle actor");

        let mut queued = Vec::new();
        for _ in 0..MAX_PENDING {
            let queued_client = client.clone();
            queued.push(tokio::spawn(async move {
                queued_client.select_core(ClashCore::Mihomo).await
            }));
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.status().queued.len() == MAX_PENDING {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pending lifecycle queue should fill");

        let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
        assert!(error.to_string().contains("core lifecycle queue is full"));
        assert!(
            client
                .status()
                .completed
                .iter()
                .any(|entry| entry.error.as_deref() == Some("core lifecycle queue is full"))
        );

        release_stop.notify_one();
        stop.await.unwrap().unwrap();
        for task in queued {
            task.await.unwrap().unwrap();
        }

        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "blocking-stop")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "queued-select")
                .count(),
            MAX_PENDING
        );
        assert!(client.status().queued.is_empty());
    }

    #[tokio::test]
    async fn burst_runtime_dirty_requests_coalesce_into_one_reconcile() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        for _ in 0..20 {
            client.request_runtime_rebuild();
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events.lock().unwrap().contains(&"rebuild") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("coalesced runtime rebuild should execute");
        tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(50)).await;

        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
        assert_eq!(client.status().completed.len(), 1);
    }

    #[tokio::test]
    async fn runtime_dirty_after_window_schedules_a_later_reconcile() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        client.request_runtime_rebuild();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events.lock().unwrap().len() == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("first runtime rebuild should execute");

        client.request_runtime_rebuild();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events.lock().unwrap().len() == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("later runtime rebuild should execute");

        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild", "rebuild"]);
        assert_eq!(client.status().completed.len(), 2);
    }

    #[tokio::test]
    async fn workflow_panic_latches_uncertain_and_blocks_follow_up_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(PanicOnStopCore {
                events: events.clone(),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let error = client.stop_core().await.unwrap_err();
        assert!(error.to_string().contains("execution state is uncertain"));
        assert!(client.status().uncertain);

        client.request_runtime_rebuild();
        tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(50)).await;

        let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("previous core lifecycle operation has an uncertain outcome")
        );
        assert_eq!(events.lock().unwrap().as_slice(), ["panic-stop"]);

        let status = client.status();
        assert!(status.uncertain);
        assert_eq!(status.completed.len(), 2);
        assert!(
            status.completed[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("execution state is uncertain"))
        );
        assert!(
            status.completed[1]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("previous core lifecycle operation"))
        );
    }

    #[tokio::test]
    async fn timed_out_wait_does_not_cancel_admitted_operation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let error = client
            .execute_with_timeout(Command::StopCore, Duration::from_millis(1))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("operation 1 timed out"));

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.status().completed.iter().any(|entry| entry.id == 1) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("timed-out lifecycle operation should still settle");

        let status = client.status();
        let completed = status
            .completed
            .iter()
            .find(|entry| entry.id == 1)
            .expect("operation should be recorded");
        assert!(completed.error.is_none());
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end"]
        );
    }

    #[tokio::test]
    async fn crash_recovery_signal_enters_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovery_notify = Arc::new(tokio::sync::Notify::new());
        let _client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: recovery_notify.clone(),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        recovery_notify.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events.lock().unwrap().contains(&"rebuild") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("recovery signal should be admitted by the lifecycle actor");
    }

    #[tokio::test]
    async fn startup_reconcile_enters_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        client.request_startup_reconcile().unwrap();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events.lock().unwrap().contains(&"rebuild") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("startup reconcile should be admitted by the lifecycle actor");
    }

    #[tokio::test]
    async fn reconcile_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        client.reconcile().await.unwrap();

        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
    }

    #[tokio::test]
    async fn probe_service_reads_through_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        let status = client.probe_service().await.unwrap();
        let cached = client.service_status();

        assert_eq!(status.status, chimera_ipc::types::ServiceStatus::Stopped);
        assert_eq!(status.phase, ServicePhase::DaemonStopped);
        assert_eq!(status.name.as_ref(), "chimera-service");
        assert_eq!(cached.phase, ServicePhase::DaemonStopped);
        assert_eq!(events.lock().unwrap().as_slice(), ["service-probe"]);
    }

    #[tokio::test]
    async fn health_observation_updates_cached_service_status_without_reprobe() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(client.service_status().phase, ServicePhase::Probing);
        client.observe_service_status(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.0.0"),
            status: chimera_ipc::types::ServiceStatus::Stopped,
            server: None,
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.service_status().phase == ServicePhase::DaemonStopped {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("health observation should update cached service status");
        assert!(events.lock().unwrap().is_empty());

        client.observe_service_probe_failure();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.service_status().phase == ServicePhase::Unknown {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("probe failure should publish unknown service phase");
        let status = client.service_status();
        assert_eq!(status.status, chimera_ipc::types::ServiceStatus::Stopped);
        assert!(!status.runtime_owned);
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_service_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        client.update_service().await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["service-begin", "service-update", "service-probe"]
        );
    }

    #[tokio::test]
    async fn install_service_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        client.install_service().await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["service-begin", "service-install", "service-probe"]
        );
    }

    #[tokio::test]
    async fn uninstall_service_hands_core_back_to_local_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let local = Arc::new(AtomicBool::new(false));
        let mut app_state = chimera_config::application::ChimeraAppConfig::default();
        app_state.enable_service_mode = true;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(ServiceHandoffCore {
                events: events.clone(),
                local: local.clone(),
            }),
            ApplicationClient::static_state(app_state),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        client.uninstall_service().await.unwrap();

        assert!(local.load(AtomicOrdering::Relaxed));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "service-begin",
                "service-uninstall",
                "service-confirm",
                "rebuild-local",
                "service-probe"
            ]
        );
    }

    #[tokio::test]
    async fn start_service_converges_core_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(AtomicBool::new(false));
        let mut app_state = chimera_config::application::ChimeraAppConfig::default();
        app_state.enable_service_mode = true;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(ServiceStartCore {
                events: events.clone(),
                service: service.clone(),
            }),
            ApplicationClient::static_state(app_state),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        client.start_service().await.unwrap();

        assert!(service.load(AtomicOrdering::Relaxed));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "service-begin",
                "service-start",
                "service-ready",
                "rebuild-service",
                "service-ready",
                "service-probe"
            ]
        );
    }

    #[tokio::test]
    async fn restart_service_converges_core_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(AtomicBool::new(false));
        let mut app_state = chimera_config::application::ChimeraAppConfig::default();
        app_state.enable_service_mode = true;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(ServiceStartCore {
                events: events.clone(),
                service: service.clone(),
            }),
            ApplicationClient::static_state(app_state),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        client.restart_service().await.unwrap();

        assert!(service.load(AtomicOrdering::Relaxed));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "service-begin",
                "service-restart",
                "service-ready",
                "rebuild-service",
                "service-ready",
                "service-probe"
            ]
        );
    }

    #[tokio::test]
    async fn stop_service_hands_core_back_to_local_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let local = Arc::new(AtomicBool::new(false));
        let mut app_state = chimera_config::application::ChimeraAppConfig::default();
        app_state.enable_service_mode = true;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(ServiceHandoffCore {
                events: events.clone(),
                local: local.clone(),
            }),
            ApplicationClient::static_state(app_state),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();

        client.stop_service().await.unwrap();

        assert!(local.load(AtomicOrdering::Relaxed));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "service-begin",
                "service-stop",
                "service-confirm",
                "rebuild-local",
                "service-probe"
            ]
        );
    }

    #[tokio::test]
    async fn replace_binary_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = crate::bridge::verge::legacy_core_from_typed(application.get_typed().core);
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            application,
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let artifact = PreparedCoreBinary {
            target,
            source: staging.path().join("staged-core"),
            destination: staging.path().join("installed-core"),
            staging,
            progress: Arc::new(RecordingProgress {
                events: events.clone(),
            }),
        };

        client.replace_core_binary(artifact).await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "stop-start",
                "stop-end",
                "install",
                "restarting",
                "run",
                "finished"
            ]
        );
    }
}

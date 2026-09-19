//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Lifecycle mutation admission is serialized through a client-owned actor;
//! lower host ownership is delegated to `core::actor_v2::CoreFacade`.

pub(crate) mod adapters;
pub(crate) mod ports;
mod workflow;

use std::{
    collections::VecDeque,
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
    Exhausted,
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
            ServiceStatus::Running if compat.allows_service_backend() && runtime_owned => {
                ServicePhase::Ready
            }
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

    fn with_restart_policy(
        mut self,
        policy: crate::core::actor_v2::facade::ServiceRestartPolicySnapshot,
    ) -> Self {
        self.restart_attempts = policy.attempts;
        if policy.exhausted {
            self.phase = ServicePhase::Exhausted;
        }
        self
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
    BinaryInstallProgress, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
    RunningConfigPort, RuntimeTransformDiagnostics, ServiceLifecyclePort, ServiceTransitionLease,
};

struct Response {
    id: OperationId,
    reply: Option<RpcReplyPort<anyhow::Result<()>>>,
}

struct Request {
    command: Command,
    response: Response,
}

struct ActiveOperation {
    response: Response,
    task: tokio::task::JoinHandle<()>,
    shutdown: bool,
}

enum Message {
    Request(Request),
    Completed {
        id: OperationId,
        workflow: CoreLifecycleWorkflow,
        result: anyhow::Result<()>,
        workflow_panicked: bool,
        lower_outcome_uncertain: bool,
        service_restart_policy: crate::core::actor_v2::facade::ServiceRestartPolicySnapshot,
        service_probe: Option<Result<chimera_ipc::types::StatusInfo<'static>, String>>,
        retry_reconcile_on_failure: bool,
        recover: bool,
        shutdown: bool,
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
    workflow: Option<CoreLifecycleWorkflow>,
    active: Option<ActiveOperation>,
    pending: VecDeque<Request>,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: tokio::sync::watch::Sender<ServiceHostStatus>,
    uncertain: bool,
    shutting_down: bool,
    shutdown_result: Option<Option<String>>,
    shutdown_waiters: Vec<Response>,
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

    fn publish(&self) {
        let mut status = self.status.lock();
        status.active = self.active.as_ref().map(|operation| operation.response.id);
        status.queued = self
            .pending
            .iter()
            .map(|request| request.response.id)
            .chain(self.shutdown_waiters.iter().map(|response| response.id))
            .collect();
        status.uncertain = self.uncertain;
        status.shutting_down = self.shutting_down;
    }

    fn settle(&self, response: Response, result: anyhow::Result<()>) {
        {
            let mut status = self.status.lock();
            if status.completed.len() == COMPLETED_LIMIT {
                status.completed.remove(0);
            }
            status.completed.push(CoreLifecycleOperationResult {
                id: response.id,
                error: result.as_ref().err().map(ToString::to_string),
            });
        }
        if let Some(reply) = response.reply {
            let _ = reply.send(result);
        } else if let Err(error) = result {
            tracing::warn!(%error, id = response.id, "background core lifecycle operation failed");
        }
    }

    fn publish_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        let previous = self.service_status.borrow().clone();
        let mut status = ServiceHostStatus::from_probe(info);
        status.restart_attempts = previous.restart_attempts;
        if previous.phase == ServicePhase::Exhausted {
            status.phase = ServicePhase::Exhausted;
        }
        self.service_status.send_replace(status);
    }

    fn publish_service_status_with_policy(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
        policy: crate::core::actor_v2::facade::ServiceRestartPolicySnapshot,
    ) {
        self.service_status
            .send_replace(ServiceHostStatus::from_probe(info).with_restart_policy(policy));
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
        let mut status = ServiceHostStatus::probe_failed(&previous);
        if previous.phase == ServicePhase::Exhausted {
            status.phase = ServicePhase::Exhausted;
        }
        self.service_status.send_replace(status);
    }

    async fn refresh_service_status(&self) -> anyhow::Result<ServiceHostStatus> {
        let workflow = self
            .workflow
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("core lifecycle workflow is busy"))?;
        let policy = workflow.service_restart_policy();
        match workflow.probe_service().await {
            Ok(info) => {
                let status = ServiceHostStatus::from_probe(info).with_restart_policy(policy);
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

    fn reject_pending_for_uncertain(&mut self) {
        if !self.uncertain || self.shutting_down {
            return;
        }
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            self.settle(
                request.response,
                Err(anyhow::anyhow!(
                    "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                )),
            );
        }
    }

    fn close(&mut self) {
        self.shutting_down = true;
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            self.settle(
                request.response,
                Err(anyhow::anyhow!("core lifecycle is shutting down")),
            );
        }
        self.publish();
    }

    fn push_background(&mut self, command: Command) {
        if self.pending.len() >= MAX_PENDING {
            tracing::warn!("dropping background lifecycle operation because the queue is full");
            return;
        }
        self.pending.push_back(Request {
            command,
            response: Response {
                id: self.allocate_operation_id(),
                reply: None,
            },
        });
    }

    fn drive(&mut self, myself: &ActorRef<Message>) {
        if self.active.is_some() {
            self.publish();
            return;
        }

        self.reject_pending_for_uncertain();

        let request = if self.shutting_down {
            if self.shutdown_result.is_some() {
                self.publish();
                return;
            }
            let Some(response) = self.shutdown_waiters.pop() else {
                self.publish();
                return;
            };
            Request {
                command: Command::Shutdown,
                response,
            }
        } else if let Some(request) = self.pending.pop_front() {
            request
        } else {
            self.publish();
            return;
        };

        let Some(workflow) = self.workflow.take() else {
            tracing::error!("core lifecycle workflow missing without an active operation");
            self.settle(
                request.response,
                Err(anyhow::anyhow!("core lifecycle workflow unavailable")),
            );
            self.publish();
            return;
        };

        let id = request.response.id;
        let shutdown = matches!(&request.command, Command::Shutdown);
        let service_phase = match &request.command {
            Command::InstallService => Some(ServicePhase::Installing),
            Command::StartService => Some(ServicePhase::StartingDaemon),
            Command::RestartService => Some(ServicePhase::Restarting),
            Command::UninstallService => Some(ServicePhase::Uninstalling),
            _ => None,
        };
        let service_mutation = matches!(
            &request.command,
            Command::InstallService
                | Command::UninstallService
                | Command::UpdateService
                | Command::StartService
                | Command::RestartService
                | Command::StopService
                | Command::ServiceEndpointDown
        );
        let retry_reconcile_on_failure = matches!(
            &request.command,
            Command::StartService
                | Command::RestartService
                | Command::StopService
                | Command::UninstallService
                | Command::UpdateService
        );
        let recover = matches!(&request.command, Command::RecoverCore);
        let progress = match &request.command {
            Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
            _ => None,
        };
        if let Some(phase) = service_phase {
            self.publish_service_phase(phase);
        }

        let command = request.command;
        let actor = myself.clone();
        let task = tokio::spawn(async move {
            let (result, workflow_panicked) = match AssertUnwindSafe(workflow.execute(command))
                .catch_unwind()
                .await
            {
                Ok(result) => (result, false),
                Err(_) => (
                    Err(anyhow::anyhow!(
                        "core lifecycle workflow panicked; execution state is uncertain"
                    )),
                    true,
                ),
            };
            if let Some(progress) = progress {
                let error = result.as_ref().err().map(ToString::to_string);
                if std::panic::catch_unwind(AssertUnwindSafe(|| {
                    progress.finished(error.as_deref())
                }))
                .is_err()
                {
                    tracing::error!("binary installation progress observer panicked");
                }
            }
            let lower_outcome_uncertain = workflow.outcome_uncertain();
            let service_restart_policy = workflow.service_restart_policy();
            let service_probe = if service_mutation && !workflow_panicked {
                Some(
                    workflow
                        .probe_service()
                        .await
                        .map_err(|error| error.to_string()),
                )
            } else {
                None
            };
            let _ = actor.cast(Message::Completed {
                id,
                workflow,
                result,
                workflow_panicked,
                lower_outcome_uncertain,
                service_restart_policy,
                service_probe,
                retry_reconcile_on_failure,
                recover,
                shutdown,
            });
        });

        self.active = Some(ActiveOperation {
            response: request.response,
            task,
            shutdown,
        });
        self.publish();
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
            workflow: Some(args.workflow),
            active: None,
            pending: VecDeque::new(),
            next_id: args.next_id,
            status: args.status,
            service_status: args.service_status,
            uncertain: false,
            shutting_down: false,
            shutdown_result: None,
            shutdown_waiters: Vec::new(),
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
            Message::Request(request) => {
                if matches!(&request.command, Command::Shutdown) {
                    if let Some(error) = state.shutdown_result.clone() {
                        let result = match error {
                            Some(error) => Err(anyhow::anyhow!(error)),
                            None => Ok(()),
                        };
                        state.settle(request.response, result);
                    } else if state.shutdown_waiters.len() >= MAX_PENDING {
                        state.settle(
                            request.response,
                            Err(anyhow::anyhow!("too many shutdown waiters")),
                        );
                    } else {
                        state.shutdown_waiters.push(request.response);
                        if !state.shutting_down {
                            state.close();
                        }
                    }
                } else if state.shutting_down {
                    state.settle(
                        request.response,
                        Err(anyhow::anyhow!("core lifecycle is shutting down")),
                    );
                } else if state.uncertain {
                    state.settle(
                        request.response,
                        Err(anyhow::anyhow!(
                            "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                        )),
                    );
                } else if state.pending.len() >= MAX_PENDING {
                    state.settle(
                        request.response,
                        Err(anyhow::anyhow!("core lifecycle queue is full")),
                    );
                } else {
                    state.pending.push_back(request);
                }
            }
            Message::Completed {
                id,
                workflow,
                result,
                workflow_panicked,
                lower_outcome_uncertain,
                service_restart_policy,
                service_probe,
                retry_reconcile_on_failure,
                recover,
                shutdown,
            } => {
                if state.active.as_ref().map(|operation| operation.response.id) != Some(id) {
                    return Ok(());
                }
                let active = state
                    .active
                    .take()
                    .expect("matched active lifecycle operation");
                debug_assert_eq!(active.shutdown, shutdown);
                let _ = active.task.await;
                state.workflow = Some(workflow);
                state.uncertain |= workflow_panicked || lower_outcome_uncertain;

                match service_probe {
                    Some(Ok(info)) => {
                        state.publish_service_status_with_policy(info, service_restart_policy)
                    }
                    Some(Err(error)) => {
                        tracing::debug!(%error, "failed to refresh service status after mutation");
                        state.publish_service_probe_failure();
                    }
                    None => {}
                }

                let failed = result.is_err();
                if shutdown {
                    let shutdown_error = result.as_ref().err().map(ToString::to_string);
                    state.shutdown_result = Some(shutdown_error.clone());
                    state.settle(active.response, result);
                    for waiter in std::mem::take(&mut state.shutdown_waiters) {
                        let waiter_result = match &shutdown_error {
                            Some(error) => Err(anyhow::anyhow!(error.clone())),
                            None => Ok(()),
                        };
                        state.settle(waiter, waiter_result);
                    }
                } else {
                    state.settle(active.response, result);
                }

                if retry_reconcile_on_failure && failed && !state.uncertain && !state.shutting_down
                {
                    state.mark_runtime_dirty(&myself);
                }
                if recover && failed && !state.uncertain && !state.shutting_down {
                    tracing::error!("failed to recover core; scheduling retry");
                    let actor = myself.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let _ = actor.cast(Message::RecoverCore);
                    });
                }
            }
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
            Message::StartupReconcile => {
                if state.uncertain || state.shutting_down {
                    tracing::warn!(
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
                        "ignoring startup reconcile because lifecycle is unavailable"
                    );
                } else {
                    state.push_background(Command::Reconcile);
                }
            }
            Message::RecoverCore => {
                if state.uncertain || state.shutting_down {
                    tracing::warn!(
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
                        "ignoring crash recovery because lifecycle is unavailable"
                    );
                } else {
                    tracing::info!("queueing core recovery through lifecycle actor");
                    state.push_background(Command::RecoverCore);
                }
            }
            Message::RuntimeDirty => {
                if state.uncertain || state.shutting_down {
                    tracing::debug!(
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
                        "ignoring runtime dirty signal because lifecycle is unavailable"
                    );
                } else {
                    state.mark_runtime_dirty(&myself);
                }
            }
            Message::DirtyTick => {
                state.dirty_tick_scheduled = false;
                if state.uncertain || state.shutting_down {
                    state.dirty = false;
                } else if state.dirty {
                    state.dirty = false;
                    state.push_background(Command::Reconcile);
                }
            }
        }
        state.drive(&myself);
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
    #[cfg(test)]
    pub(super) async fn spawn(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_service(
            core,
            application,
            clash,
            runtime_paths,
            Arc::new(LegacyServiceBridge::new(Arc::new(
                crate::core::actor_v2::CoreFacade::new_local(),
            ))),
        )
        .await
    }

    pub(super) async fn spawn_with_service(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> anyhow::Result<Self> {
        let client = Self::spawn_with_installer(
            core,
            application,
            clash,
            runtime_paths,
            Arc::new(FsBinaryInstaller),
            service,
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
                Arc::new(LegacyServiceBridge::new(Arc::new(
                    crate::core::actor_v2::CoreFacade::new_local(),
                ))),
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
                actor_ref, next_id, ..
            } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                match actor_ref
                    .call(
                        |reply| {
                            Message::Request(Request {
                                command,
                                response: Response {
                                    id,
                                    reply: Some(reply),
                                },
                            })
                        },
                        Some(timeout),
                    )
                    .await
                {
                    Ok(CallResult::Success(result)) => result,
                    Ok(CallResult::Timeout) => anyhow::bail!(
                        "core lifecycle operation {id} timed out; it may still be queued or running"
                    ),
                    Ok(CallResult::SenderError) => anyhow::bail!(
                        "core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                    ),
                    Err(error) => Err(error.into()),
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

    pub(super) fn request_service_endpoint_down(&self) {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref, next_id, ..
            } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                let request = Request {
                    command: Command::ServiceEndpointDown,
                    response: Response { id, reply: None },
                };
                if actor_ref.cast(Message::Request(request)).is_err() {
                    tracing::warn!("failed to enqueue service endpoint-down recovery");
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => {}
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

    #[cfg(test)]
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

    pub(crate) fn request_service_endpoint_down(&self) {
        self.inner.core_lifecycle.request_service_endpoint_down();
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

    #[cfg(test)]
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
        atomic::{AtomicBool, AtomicU8, Ordering as AtomicOrdering},
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

    struct PanicOnStopCore {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct LowerUncertainCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        uncertain: AtomicBool,
    }

    struct BlockingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<tokio::sync::Notify>,
        release_stop: Arc<tokio::sync::Notify>,
    }

    struct ServiceHandoffCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        local: Arc<AtomicBool>,
    }

    struct ServiceStartCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        service: Arc<AtomicBool>,
    }

    struct RecordingService {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RestartBudgetService {
        events: Arc<Mutex<Vec<&'static str>>>,
        attempts: AtomicU8,
        exhausted: AtomicBool,
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
    impl ServiceLifecyclePort for RestartBudgetService {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.events.lock().unwrap().push("service-probe");
            Ok(chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("1.0.0"),
                status: chimera_ipc::types::ServiceStatus::Stopped,
                server: None,
            })
        }

        async fn report_endpoint_down(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("endpoint-down");
            let attempts = self.attempts.load(AtomicOrdering::Acquire);
            if attempts >= 3 {
                self.exhausted.store(true, AtomicOrdering::Release);
            } else {
                self.attempts
                    .store(attempts.saturating_add(1), AtomicOrdering::Release);
            }
            Ok(())
        }

        fn restart_policy(&self) -> crate::core::actor_v2::facade::ServiceRestartPolicySnapshot {
            crate::core::actor_v2::facade::ServiceRestartPolicySnapshot {
                attempts: self.attempts.load(AtomicOrdering::Acquire),
                exhausted: self.exhausted.load(AtomicOrdering::Acquire),
            }
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
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
    impl CoreLifecyclePort for PanicOnStopCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild-after-panic");
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("panic-stop");
            panic!("injected lifecycle panic");
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select-after-panic");
            Ok(())
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
    impl CoreLifecyclePort for LowerUncertainCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("lower-rebuild");
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("lower-stop");
            self.uncertain.store(true, AtomicOrdering::Release);
            anyhow::bail!("lower mutation reply lost")
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("lower-select");
            Ok(())
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

        fn outcome_uncertain(&self) -> bool {
            self.uncertain.load(AtomicOrdering::Acquire)
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for BlockingCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("blocking-stop");
            self.stop_started.notify_one();
            self.release_stop.notified().await;
            Ok(())
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("queued-select");
            Ok(())
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
    impl CoreLifecyclePort for ServiceHandoffCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild-local");
            self.local.store(true, AtomicOrdering::Relaxed);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            Ok(())
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
    impl CoreLifecyclePort for ServiceStartCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild-service");
            self.service.store(true, AtomicOrdering::Relaxed);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            Ok(())
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
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop-start");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.events.lock().unwrap().push("stop-end");
            Ok(())
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select");
            Ok(())
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
    async fn shutdown_drains_pending_queue_before_final_stop() {
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

        let active_client = client.clone();
        let active = tokio::spawn(async move { active_client.stop_core().await });
        tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
            .await
            .expect("active stop should start");

        let queued_a = {
            let client = client.clone();
            tokio::spawn(async move { client.select_core(ClashCore::Mihomo).await })
        };
        let queued_b = {
            let client = client.clone();
            tokio::spawn(async move { client.select_core(ClashCore::Mihomo).await })
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.status().queued.len() == 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("two mutations should enter the actor-owned queue");

        let shutdown_client = client.clone();
        let shutdown = tokio::spawn(async move { shutdown_client.shutdown().await });
        let error_a = queued_a.await.unwrap().unwrap_err();
        let error_b = queued_b.await.unwrap().unwrap_err();
        assert!(
            error_a
                .to_string()
                .contains("core lifecycle is shutting down")
        );
        assert!(
            error_b
                .to_string()
                .contains("core lifecycle is shutting down")
        );
        assert!(client.status().shutting_down);

        release_stop.notify_one();
        active.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
            .await
            .expect("shutdown stop should start after the active operation settles");
        release_stop.notify_one();
        shutdown.await.unwrap().unwrap();

        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "blocking-stop")
                .count(),
            2
        );
        assert!(!events.iter().any(|event| *event == "queued-select"));
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
    async fn lower_outcome_uncertain_latches_actor_and_blocks_follow_up_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(LowerUncertainCore {
                events: events.clone(),
                uncertain: AtomicBool::new(false),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let error = client.stop_core().await.unwrap_err();
        assert!(error.to_string().contains("lower mutation reply lost"));
        assert!(client.status().uncertain);

        let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("previous core lifecycle operation has an uncertain outcome")
        );
        assert_eq!(events.lock().unwrap().as_slice(), ["lower-stop"]);
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
    async fn endpoint_down_restart_budget_latches_exhausted_in_cached_status() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        });
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: Arc::new(Mutex::new(Vec::new())),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            service,
        )
        .await
        .unwrap();

        for expected_attempts in 1..=3 {
            client.request_service_endpoint_down();
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let status = client.service_status();
                    if status.restart_attempts == expected_attempts {
                        assert_eq!(status.phase, ServicePhase::DaemonStopped);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("endpoint-down recovery should publish the restart attempt");
        }

        client.request_service_endpoint_down();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let status = client.service_status();
                if status.phase == ServicePhase::Exhausted {
                    assert_eq!(status.restart_attempts, 3);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("spent restart budget should latch exhausted");

        client.observe_service_status(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.0.0"),
            status: chimera_ipc::types::ServiceStatus::Stopped,
            server: None,
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let status = client.service_status();
        assert_eq!(status.phase, ServicePhase::Exhausted);
        assert_eq!(status.restart_attempts, 3);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "endpoint-down",
                "service-probe",
                "endpoint-down",
                "service-probe",
                "endpoint-down",
                "service-probe",
                "endpoint-down",
                "service-probe"
            ]
        );
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
                "rebuild",
                "finished"
            ]
        );
    }
}

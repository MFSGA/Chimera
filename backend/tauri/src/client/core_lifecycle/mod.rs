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
    ChimeraClient, application::ApplicationClient,
    application_workflow::workflow::ApplicationWorkflow, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};
use workflow::{Command, CoreLifecycleWorkflow};

const CALL_WAIT: Duration = Duration::from_secs(180);
const DIRTY_WINDOW: Duration = Duration::from_millis(500);
const MAX_PENDING: usize = 32;
const COMPLETED_LIMIT: usize = 32;

type OperationId = u64;

struct CompletedLifecycleOperation(anyhow::Result<()>);

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

enum Message {
    ConnectApplicationWorkflow {
        workflow: ApplicationWorkflow,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
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
        application_workflow: Option<ApplicationWorkflow>,
        request_runtime_rebuild: bool,
        deferred_retry: Option<String>,
        reconciled: bool,
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
    RuntimeDirty,
    DirtyTick,
    DeferredRetry {
        digest: String,
    },
}

struct ApplicationWorkflowActor;

struct ApplicationWorkflowState {
    workflow: Option<CoreLifecycleWorkflow>,
    application_workflow: Option<ApplicationWorkflow>,
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

struct ApplicationWorkflowArgs {
    workflow: CoreLifecycleWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: tokio::sync::watch::Sender<ServiceHostStatus>,
}

struct Response {
    id: OperationId,
    reply: Option<RpcReplyPort<anyhow::Result<()>>>,
}

struct Request {
    command: Command,
    response: Response,
    deferred_retry: Option<String>,
}

struct ActiveOperation {
    response: Response,
    task: tokio::task::JoinHandle<()>,
    shutdown: bool,
}

impl ApplicationWorkflowState {
    fn allocate_operation_id(&self) -> OperationId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
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

    async fn refresh_service_status(&mut self) -> anyhow::Result<ServiceHostStatus> {
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

    fn schedule_deferred_retry(
        &self,
        myself: &ActorRef<Message>,
        digest: String,
        next_attempt: tokio::time::Instant,
    ) {
        let actor = myself.clone();
        tokio::spawn(async move {
            tokio::time::sleep_until(next_attempt).await;
            let _ = actor.cast(Message::DeferredRetry { digest });
        });
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

    fn reject(&self, mut request: Request, error: anyhow::Error) {
        if let Command::ProfileMutation(mutation) = &mut request.command {
            let message = error.to_string();
            mutation.answer(
                crate::client::application_workflow::mutation::TryAck::Failed(message.clone()),
            );
            mutation.settle(Err(message));
        }
        if let Command::ReplaceCoreBinary(artifact) = &request.command {
            let message = error.to_string();
            if std::panic::catch_unwind(AssertUnwindSafe(|| {
                artifact.progress.finished(Some(&message));
            }))
            .is_err()
            {
                tracing::error!("binary installation progress observer panicked");
            }
        }
        self.settle(request.response, Err(error));
    }

    fn reject_pending_for_uncertain(&mut self) {
        if !self.uncertain || self.shutting_down {
            return;
        }
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            self.reject(
                request,
                anyhow::anyhow!(
                    "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                ),
            );
        }
    }

    fn close(&mut self) {
        self.shutting_down = true;
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            self.reject(request, anyhow::anyhow!("core lifecycle is shutting down"));
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
            deferred_retry: None,
        });
    }

    fn push_deferred_retry(&mut self, digest: String) -> bool {
        if self.pending.len() >= MAX_PENDING {
            tracing::warn!("dropping deferred Profile retry because the lifecycle queue is full");
            return false;
        }
        self.pending.push_back(Request {
            command: Command::RetryDeferredProfiles {
                digest: digest.clone(),
            },
            response: Response {
                id: self.allocate_operation_id(),
                reply: None,
            },
            deferred_retry: Some(digest),
        });
        true
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
                deferred_retry: None,
            }
        } else if let Some(request) = self.pending.pop_front() {
            request
        } else {
            self.publish();
            return;
        };

        let Some(workflow) = self.workflow.take() else {
            tracing::error!("core lifecycle workflow missing without an active operation");
            self.reject(
                request,
                anyhow::anyhow!("core lifecycle workflow unavailable"),
            );
            self.publish();
            return;
        };

        let profile_mutation = matches!(&request.command, Command::ProfileMutation(_));
        let application_workflow = if profile_mutation {
            match self.application_workflow.take() {
                Some(workflow) => Some(workflow),
                None => {
                    self.workflow = Some(workflow);
                    self.reject(
                        request,
                        anyhow::anyhow!("application workflow is not connected"),
                    );
                    self.publish();
                    return;
                }
            }
        } else {
            None
        };

        let id = request.response.id;
        let shutdown = matches!(&request.command, Command::Shutdown);
        let deferred_retry = request.deferred_retry.clone();
        let is_service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
        let application_runtime_reconcile = matches!(
            &request.command,
            Command::PatchApplicationConfig { patch, .. }
                if crate::client::application_workflow::impact::application_patch_requires_reconcile(
                    patch,
                    is_service_connected,
                )
        );
        let reconciled = matches!(
            &request.command,
            Command::Reconcile
                | Command::RetryDeferredProfiles { .. }
                | Command::ReconcileProfiles { .. }
                | Command::RecoverCore
                | Command::SelectCore(_)
                | Command::ReplaceCoreBinary(_)
        ) || application_runtime_reconcile;
        let service_phase = match &request.command {
            Command::InstallService(_) => Some(ServicePhase::Installing),
            Command::StartService { .. } => Some(ServicePhase::StartingDaemon),
            Command::RestartService { .. } => Some(ServicePhase::Restarting),
            Command::UninstallService(_) => Some(ServicePhase::Uninstalling),
            _ => None,
        };
        let service_mutation = matches!(
            &request.command,
            Command::InstallService(_)
                | Command::UninstallService(_)
                | Command::UpdateService(_)
                | Command::StartService { .. }
                | Command::RestartService { .. }
                | Command::StopService(_)
                | Command::ServiceEndpointDown(_)
        );
        let retry_reconcile_on_failure = matches!(
            &request.command,
            Command::StartService { .. }
                | Command::RestartService { .. }
                | Command::StopService(_)
                | Command::UninstallService(_)
                | Command::UpdateService(_)
        );
        let recover = matches!(&request.command, Command::RecoverCore);
        let progress = match &request.command {
            Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
            _ => None,
        };
        if let Some(phase) = service_phase {
            self.publish_service_phase(phase);
        }

        let mut command = request.command;
        if let Command::PatchApplicationConfig {
            service_connected, ..
        } = &mut command
        {
            *service_connected = is_service_connected;
        }
        let actor = myself.clone();
        let task = tokio::spawn(async move {
            let (result, workflow_panicked, request_runtime_rebuild, application_workflow) =
                match command {
                    Command::ProfileMutation(mut mutation) => {
                        let mut application_workflow = application_workflow
                            .expect("Profile mutation takes the application workflow");
                        let run = AssertUnwindSafe(
                            application_workflow.run_mutation(&mut mutation, &workflow),
                        )
                        .catch_unwind()
                        .await;
                        match run {
                            Ok((request_runtime_rebuild, settlement)) => (
                                settlement.map_err(anyhow::Error::msg),
                                false,
                                request_runtime_rebuild,
                                Some(application_workflow),
                            ),
                            Err(_) => {
                                let message = format!(
                                    "application workflow operation {} panicked; runtime outcome requires recovery",
                                    mutation.operation_id
                                );
                                application_workflow
                                    .mark_recovery_required(message.clone())
                                    .await;
                                mutation.answer(
                                    crate::client::application_workflow::mutation::TryAck::Failed(
                                        message.clone(),
                                    ),
                                );
                                let _ = tokio::time::timeout(
                                crate::client::application_workflow::tcc::MUTATION_DECISION_TIMEOUT,
                                mutation.decision.wait(),
                            )
                            .await;
                                mutation.settle(Err(message.clone()));
                                (
                                    Err(anyhow::anyhow!(message)),
                                    true,
                                    false,
                                    Some(application_workflow),
                                )
                            }
                        }
                    }
                    command => {
                        match AssertUnwindSafe(workflow.execute(command))
                            .catch_unwind()
                            .await
                        {
                            Ok(result) => (result, false, false, None),
                            Err(_) => (
                                Err(anyhow::anyhow!(
                                    "core lifecycle workflow panicked; execution state is uncertain"
                                )),
                                true,
                                false,
                                None,
                            ),
                        }
                    }
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
                    match AssertUnwindSafe(workflow.probe_service())
                        .catch_unwind()
                        .await
                    {
                        Ok(Ok(info)) => Ok(info),
                        Ok(Err(error)) => Err(error.to_string()),
                        Err(_) => Err("service status probe panicked".to_string()),
                    },
                )
            } else {
                None
            };
            if actor
                .cast(Message::Completed {
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
                    application_workflow,
                    request_runtime_rebuild,
                    deferred_retry,
                    reconciled,
                })
                .is_err()
            {
                tracing::error!(
                    id,
                    "core lifecycle actor stopped before operation completion"
                );
            }
        });

        self.active = Some(ActiveOperation {
            response: request.response,
            task,
            shutdown,
        });
        self.publish();
    }
}

impl Actor for ApplicationWorkflowActor {
    type Msg = Message;
    type State = ApplicationWorkflowState;
    type Arguments = ApplicationWorkflowArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ApplicationWorkflowState {
            workflow: Some(args.workflow),
            application_workflow: None,
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
            Message::ConnectApplicationWorkflow { workflow, reply } => {
                let result = if state.application_workflow.is_some() {
                    Err(anyhow::anyhow!("application workflow is already connected"))
                } else if state.shutting_down {
                    Err(anyhow::anyhow!("core lifecycle is shutting down"))
                } else {
                    state.application_workflow = Some(workflow);
                    Ok(())
                };
                let _ = reply.send(result);
            }
            Message::Request(request) => {
                if matches!(&request.command, Command::Shutdown) {
                    if let Some(error) = state.shutdown_result.clone() {
                        let result = match error {
                            Some(error) => Err(anyhow::anyhow!(error)),
                            None => Ok(()),
                        };
                        state.settle(request.response, result);
                    } else if state.shutdown_waiters.len() >= MAX_PENDING {
                        state.reject(request, anyhow::anyhow!("too many shutdown waiters"));
                    } else {
                        state.shutdown_waiters.push(request.response);
                        if !state.shutting_down {
                            state.close();
                        }
                    }
                } else if state.shutting_down {
                    state.reject(request, anyhow::anyhow!("core lifecycle is shutting down"));
                } else if state.uncertain {
                    state.reject(
                        request,
                        anyhow::anyhow!(
                            "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                        ),
                    );
                } else if state.pending.len() >= MAX_PENDING {
                    state.reject(request, anyhow::anyhow!("core lifecycle queue is full"));
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
                application_workflow,
                request_runtime_rebuild,
                deferred_retry,
                reconciled,
            } => {
                if state.active.as_ref().map(|operation| operation.response.id) == Some(id) {
                    let refresh_runtime_binding = reconciled || service_probe.is_some() || shutdown;
                    let runtime_binding = if refresh_runtime_binding {
                        Some(workflow.effect_port_binding().await)
                    } else {
                        None
                    };
                    let active = state
                        .active
                        .take()
                        .expect("matched active lifecycle operation");
                    debug_assert_eq!(active.shutdown, shutdown);
                    let _ = active.task.await;
                    state.workflow = Some(workflow);
                    if application_workflow.is_some() {
                        state.application_workflow = application_workflow;
                    }
                    if let Some(ports) = runtime_binding
                        && let Some(application_workflow) = state.application_workflow.as_ref()
                    {
                        application_workflow.runtime_bound(ports);
                    }
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
                    let deferred_retry_error = result.as_ref().err().map(ToString::to_string);
                    let deferred_retry_retryable = result.as_ref().err().is_some_and(
                        crate::client::application_workflow::tcc::safe_to_retry_deferred_runtime,
                    );
                    let deferred_retry_waiting = result.as_ref().err().is_some_and(
                        crate::client::application_workflow::tcc::
                            deferred_runtime_waiting_dependency,
                    );
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

                    if let Some(digest) = deferred_retry {
                        if let Some(application_workflow) = state.application_workflow.as_mut() {
                            if let Some(next_attempt) = application_workflow
                                .finish_deferred_retry(
                                    &digest,
                                    !failed,
                                    deferred_retry_retryable,
                                    deferred_retry_waiting,
                                    state.uncertain,
                                    deferred_retry_error,
                                )
                                .await
                            {
                                state.schedule_deferred_retry(&myself, digest, next_attempt);
                            }
                        }
                    } else if reconciled && !failed {
                        if let Some(application_workflow) = state.application_workflow.as_mut() {
                            application_workflow.runtime_reconciled();
                        }
                    }

                    if retry_reconcile_on_failure
                        && failed
                        && !state.uncertain
                        && !state.shutting_down
                    {
                        state.mark_runtime_dirty(&myself);
                    }
                    if request_runtime_rebuild && !state.shutting_down {
                        let deferred = state
                            .application_workflow
                            .as_ref()
                            .and_then(ApplicationWorkflow::deferred_retry);
                        if let Some((digest, next_attempt)) = deferred {
                            state.schedule_deferred_retry(&myself, digest, next_attempt);
                        } else {
                            state.mark_runtime_dirty(&myself);
                        }
                    }
                    if recover && failed && !state.uncertain && !state.shutting_down {
                        tracing::error!("failed to recover core; scheduling retry");
                        let actor = myself.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            let _ = actor.cast(Message::RecoverCore);
                        });
                    }
                } else {
                    tracing::warn!(id, "ignoring stale core lifecycle completion");
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
            Message::DeferredRetry { digest } => {
                if state.uncertain || state.shutting_down {
                    if let Some(application_workflow) = state.application_workflow.as_mut() {
                        application_workflow
                            .finish_deferred_retry(
                                &digest,
                                false,
                                false,
                                false,
                                state.uncertain,
                                None,
                            )
                            .await;
                    }
                } else if state.application_workflow.is_none() {
                    state.schedule_deferred_retry(
                        &myself,
                        digest,
                        tokio::time::Instant::now() + DIRTY_WINDOW,
                    );
                } else {
                    use crate::client::application_workflow::workflow::ProfileRuntime;

                    let core_status = match state.workflow.as_ref() {
                        Some(workflow) => workflow.core_status().await.ok(),
                        None => None,
                    };
                    match core_status {
                        Some(status)
                            if matches!(
                                status.state,
                                chimera_ipc::api::status::CoreState::Running
                            ) =>
                        {
                            let should_retry = state
                                .application_workflow
                                .as_mut()
                                .is_some_and(|workflow| workflow.begin_deferred_retry(&digest));
                            if should_retry && !state.push_deferred_retry(digest.clone()) {
                                let next_attempt =
                                    state.application_workflow.as_mut().and_then(|workflow| {
                                        workflow.reschedule_deferred_retry(
                                            &digest,
                                            Duration::from_secs(1),
                                        )
                                    });
                                if let Some(next_attempt) = next_attempt {
                                    state.schedule_deferred_retry(&myself, digest, next_attempt);
                                }
                            }
                        }
                        Some(_) => {
                            if let Some(application_workflow) = state.application_workflow.as_mut()
                            {
                                application_workflow.wait_for_deferred_dependency(&digest);
                            }
                        }
                        None => {
                            let next_attempt =
                                state.application_workflow.as_mut().and_then(|workflow| {
                                    workflow
                                        .reschedule_deferred_retry(&digest, Duration::from_secs(5))
                                });
                            if let Some(next_attempt) = next_attempt {
                                state.schedule_deferred_retry(&myself, digest, next_attempt);
                            }
                        }
                    }
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
        service: Arc<dyn ports::ServiceLifecyclePort>,
        core: Arc<dyn CoreLifecyclePort>,
    },
    #[cfg(test)]
    Direct {
        workflow: tokio::sync::Mutex<CoreLifecycleWorkflow>,
        service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        core: Arc<dyn CoreLifecyclePort>,
    },
}

#[derive(Clone)]
pub(crate) struct CoreLifecycleClient(Arc<CoreLifecycleClientInner>);

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
            Arc::new(FsBinaryInstaller),
            Arc::new(LegacyServiceBridge::new(Arc::new(
                crate::core::actor_v2::CoreFacade::new_local(),
            ))),
        )
        .await
    }

    #[cfg(test)]
    async fn spawn_with_installer(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
    ) -> anyhow::Result<Self> {
        let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(LegacyServiceBridge::new(
            Arc::new(crate::core::actor_v2::CoreFacade::new_local()),
        ));
        Self::spawn_with_service(core, application, clash, runtime_paths, installer, service).await
    }

    pub(super) async fn spawn_with_service(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_profiles(
            core,
            application,
            clash,
            None,
            runtime_paths,
            installer,
            service,
        )
        .await
    }

    pub(super) async fn spawn_with_profiles(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
    ) -> anyhow::Result<Self> {
        let recovery_notify = core.recovery_notify();
        let workflow = CoreLifecycleWorkflow::new(
            application,
            clash,
            profiles,
            core.clone(),
            installer,
            runtime_paths,
            service.clone(),
        );
        let status = Arc::new(parking_lot::Mutex::new(CoreLifecycleStatus::default()));
        let next_id = Arc::new(AtomicU64::new(1));
        let (service_status_tx, service_status_rx) =
            tokio::sync::watch::channel(ServiceHostStatus::probing());
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationWorkflowActor,
            ApplicationWorkflowArgs {
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
        let client = Self(Arc::new(CoreLifecycleClientInner::Actor {
            actor_ref,
            next_id,
            status,
            service_status: service_status_rx,
            service,
            core,
        }));
        #[cfg(not(test))]
        client.request_service_status_refresh();
        Ok(client)
    }

    #[cfg(test)]
    pub(super) fn direct(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> Self {
        let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(LegacyServiceBridge::new(
            Arc::new(crate::core::actor_v2::CoreFacade::new_local()),
        ));
        let (_service_status_tx, service_status_rx) =
            tokio::sync::watch::channel(ServiceHostStatus::probing());
        Self(Arc::new(CoreLifecycleClientInner::Direct {
            workflow: tokio::sync::Mutex::new(CoreLifecycleWorkflow::new(
                application,
                clash,
                None,
                core.clone(),
                Arc::new(FsBinaryInstaller),
                runtime_paths,
                service.clone(),
            )),
            service_status: service_status_rx,
            service,
            core,
        }))
    }

    pub(crate) async fn connect_application_workflow(
        &self,
        workflow: ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| Message::ConnectApplicationWorkflow { workflow, reply },
                        Some(CALL_WAIT),
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => {
                        anyhow::bail!("application workflow connection timed out")
                    }
                    CallResult::SenderError => {
                        anyhow::bail!("application workflow connection reply dropped")
                    }
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => {
                anyhow::bail!("application workflow requires the lifecycle actor")
            }
        }
    }

    pub(crate) fn begin_profile_mutation(
        &self,
        request: crate::client::application_workflow::mutation::MutationRequest,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref, next_id, ..
            } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                actor_ref
                    .cast(Message::Request(Request {
                        command: Command::ProfileMutation(Box::new(request)),
                        response: Response { id, reply: None },
                        deferred_retry: None,
                    }))
                    .map_err(|error| anyhow::anyhow!("failed to enqueue Profile mutation: {error}"))
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => {
                anyhow::bail!("Profile mutations require the lifecycle actor")
            }
        }
    }

    async fn execute(&self, command: Command) -> anyhow::Result<()> {
        self.execute_with_timeout(command, CALL_WAIT).await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: crate::client::system_proxy::ports::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        self.execute(Command::DisableStaleSystemProxy {
            system_proxy,
            expected_port,
            expected_core_state_changed_at,
            desired_before,
            original,
            notifications,
            persist_legacy_mirror,
        })
        .await
        .map_err(|error| {
            error
                .downcast::<crate::client::system_proxy::SystemProxyActionError>()
                .unwrap_or_else(|error| {
                    crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
                })
        })
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
                                deferred_retry: None,
                            })
                        },
                        Some(timeout),
                    )
                    .await
                {
                    Ok(CallResult::Success(result)) => result,
                    Ok(CallResult::Timeout) => anyhow::bail!(
                        "core lifecycle operation {id} timed out; it may still be queued or running; inspect get_core_lifecycle_status before retrying"
                    ),
                    Ok(CallResult::SenderError) => {
                        anyhow::bail!(
                            "core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                        )
                    }
                    Err(error) => Err(error.into()),
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow, .. } => {
                workflow.lock().await.execute(command).await
            }
        }
    }

    async fn startup_reconcile(&self) -> anyhow::Result<CompletedLifecycleOperation> {
        self.startup_reconcile_with_timeout(None).await
    }

    async fn startup_reconcile_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref, next_id, ..
            } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                match actor_ref
                    .call(
                        |reply| {
                            Message::Request(Request {
                                command: Command::Reconcile,
                                response: Response {
                                    id,
                                    reply: Some(reply),
                                },
                                deferred_retry: None,
                            })
                        },
                        timeout,
                    )
                    .await
                {
                    Ok(CallResult::Success(result)) => Ok(CompletedLifecycleOperation(result)),
                    Ok(CallResult::Timeout) => anyhow::bail!(
                        "startup core lifecycle operation {id} timed out; it may still be queued or running"
                    ),
                    Ok(CallResult::SenderError) => anyhow::bail!(
                        "startup core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                    ),
                    Err(error) => Err(error.into()),
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow, .. } => Ok(CompletedLifecycleOperation(
                workflow.lock().await.execute(Command::Reconcile).await,
            )),
        }
    }

    pub(crate) fn status(&self) -> CoreLifecycleStatus {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { status, .. } => status.lock().clone(),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => CoreLifecycleStatus::default(),
        }
    }

    pub(crate) fn request_runtime_rebuild(&self) {
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

    pub(super) async fn request_service_endpoint_down(&self) {
        let (actor_ref, next_id, service) = match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref,
                next_id,
                service,
                ..
            } => (actor_ref.clone(), next_id.clone(), service.clone()),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => return,
        };

        // Preserve the existing host-lock-before-mailbox ordering used by
        // service transitions and TUN preflight.
        let transition = match service.begin_transition().await {
            Ok(transition) => transition,
            Err(error) => {
                tracing::warn!(%error, "failed to acquire Service transition for endpoint recovery");
                return;
            }
        };
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        let request = Request {
            command: Command::ServiceEndpointDown(transition),
            response: Response { id, reply: None },
            deferred_retry: None,
        };
        if actor_ref.cast(Message::Request(request)).is_err() {
            tracing::warn!("failed to enqueue service endpoint-down recovery");
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

    #[cfg(test)]
    pub(super) async fn probe_service(&self) -> anyhow::Result<ServiceHostStatus> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(|reply| Message::ProbeService { reply }, None)
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => anyhow::bail!("service probe timed out"),
                    CallResult::SenderError => anyhow::bail!("service probe reply dropped"),
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow, .. } => workflow
                .lock()
                .await
                .probe_service()
                .await
                .map(ServiceHostStatus::from_probe),
        }
    }

    #[cfg(test)]
    pub(super) async fn stop_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StopCore).await
    }

    pub(super) async fn shutdown(&self) -> anyhow::Result<()> {
        self.execute(Command::Shutdown).await
    }

    pub(super) async fn reconcile(&self) -> anyhow::Result<()> {
        self.execute(Command::Reconcile).await
    }

    pub(crate) async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReconcileProfiles {
            profiles,
            staged_content,
        })
        .await
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { core, .. } => core.status().await,
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { core, .. } => core.status().await,
        }
    }

    pub(super) async fn select_core(
        &self,
        core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.execute(Command::SelectCore(core)).await
    }

    pub(super) async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<()> {
        self.execute(Command::PatchApplicationConfig {
            patch,
            service_connected: false,
        })
        .await
    }

    pub(super) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReplaceCoreBinary(artifact)).await
    }

    async fn execute_service_command<F>(&self, command: F) -> anyhow::Result<()>
    where
        F: FnOnce(Box<dyn ports::ServiceTransitionLease>) -> Command,
    {
        let service = match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { service, .. } => service.clone(),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { service, .. } => service.clone(),
        };
        let transition = service.begin_transition().await?;
        self.execute(command(transition)).await
    }

    pub(super) async fn install_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::InstallService).await
    }

    pub(super) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UninstallService)
            .await
    }

    pub(super) async fn update_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UpdateService).await
    }

    pub(super) async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::StartService {
            transition,
            ready_timeout,
        })
        .await
    }

    pub(super) async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::RestartService {
            transition,
            ready_timeout,
        })
        .await
    }

    pub(super) async fn stop_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::StopService).await
    }
}

impl ChimeraClient {
    pub(crate) async fn init_core(&self) -> anyhow::Result<()> {
        // The producer gate is released only after a terminal lifecycle reply.
        // An ambiguous transport error must leave it held until startup is retried.
        let startup = self
            .inner
            .core_lifecycle
            .startup_reconcile()
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "startup reconciliation did not return a terminal reply; Profile background producers remain held: {error:#}"
                )
            })?
            .0;
        let producers = match self.inner.typed_profiles.as_ref() {
            Some(profiles) => profiles.start_producers().await.map_err(anyhow::Error::new),
            None => Ok(()),
        };

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        self.publish_full_effects().await;

        match (startup, producers) {
            (Err(startup), Err(producers)) => {
                tracing::warn!(%producers, "profile producers could not be released after startup failure");
                Err(startup)
            }
            (Err(startup), _) => Err(startup),
            (_, Err(producers)) => Err(producers),
            (Ok(()), Ok(_)) => Ok(()),
        }
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

    pub(crate) async fn request_service_endpoint_down(&self) {
        self.inner
            .core_lifecycle
            .request_service_endpoint_down()
            .await;
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

    #[cfg(test)]
    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.stop_core().await
    }

    pub(crate) async fn shutdown_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.shutdown().await
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

    pub(crate) async fn stop_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.stop_service().await
    }

    pub(crate) async fn install_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.install_service().await?;
        crate::core::service::ipc::ensure_health_check(self.clone());
        Ok(())
    }

    pub(crate) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.uninstall_service().await
    }

    pub(crate) async fn update_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.update_service().await
    }

    pub(crate) async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.inner
            .core_lifecycle
            .start_service(ready_timeout)
            .await?;
        crate::core::service::ipc::ensure_health_check(self.clone());
        Ok(())
    }

    pub(crate) async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.inner
            .core_lifecycle
            .restart_service(ready_timeout)
            .await?;
        crate::core::service::ipc::ensure_health_check(self.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, AtomicU8, Ordering as AtomicOrdering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use chimera_config::application::ChimeraAppConfigPatch;
    use chimera_config::clash::config::ClashConfig;
    use chimera_ipc::api::status::CoreState;
    use tokio::sync::{Notify, oneshot};
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    use super::*;
    use crate::{config::chimera::ClashCore, core::RunType};

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<Notify>,
        release_stop: Arc<Notify>,
        recovery_notify: Arc<Notify>,
        run_type: RunType,
    }

    struct PanicOnStopCore {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct LowerUncertainCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        uncertain: AtomicBool,
    }

    struct BindingCore {
        running: AtomicBool,
    }

    #[derive(Default)]
    struct RecordingRuntimeBindings {
        ports: Mutex<Vec<Option<u16>>>,
    }

    impl crate::client::effects::ports::CommitNotifications for RecordingRuntimeBindings {
        fn application_committed(
            &self,
            _: crate::client::effects::plan::ApplicationEffectFields,
            _: Vec<crate::client::effects::plan::EffectKind>,
        ) {
        }

        fn clash_committed(&self, _: crate::client::effects::plan::ClashEffectFields) {}

        fn profiles_committed(&self, _: Vec<chimera_config::profile::ProfileId>) {}

        fn runtime_bound(
            &self,
            ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
            _: bool,
        ) {
            self.ports
                .lock()
                .unwrap()
                .push(ports.map(|binding| binding.mixed_port));
        }

        fn publish_full(&self, _: Option<chimera_config::runtime::executor::ResolvedPortBindings>) {
        }
    }

    struct RecordingInstaller {
        events: Arc<Mutex<Vec<&'static str>>>,
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

    struct ServiceHandoffCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        local: Arc<std::sync::atomic::AtomicBool>,
    }

    struct ServiceModeRecordingCore {
        application: ApplicationClient,
        observations: Arc<Mutex<Vec<(RunType, ClashCore, bool)>>>,
        mutate_application_on_first_reconcile: AtomicBool,
        fail_first_reconcile: AtomicBool,
    }

    struct ApplicationPatchRecordingCore {
        application: ApplicationClient,
        observations: Arc<Mutex<Vec<(bool, bool)>>>,
    }

    struct TestApplicationBridge;

    impl crate::state::mirror::VergeLegacyBridge for TestApplicationBridge {
        fn prepare(
            &self,
            _snap: &chimera_config::application::ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
            Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<chimera_config::application::ChimeraAppConfig> {
            Ok(chimera_config::application::ChimeraAppConfig::default())
        }
    }

    struct LifecycleTestProxy {
        state: Mutex<crate::client::system_proxy::ports::OsProxyConfig>,
        writes: Mutex<Vec<crate::client::system_proxy::ports::OsProxyConfig>>,
        pause_disable: bool,
        started: Mutex<Option<oneshot::Sender<()>>>,
        released: Mutex<bool>,
        release: Condvar,
    }

    impl LifecycleTestProxy {
        fn new(state: crate::client::system_proxy::ports::OsProxyConfig) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(state),
                writes: Mutex::new(Vec::new()),
                pause_disable: false,
                started: Mutex::new(None),
                released: Mutex::new(true),
                release: Condvar::new(),
            })
        }

        fn pause_first_disable(
            state: crate::client::system_proxy::ports::OsProxyConfig,
        ) -> (Arc<Self>, oneshot::Receiver<()>) {
            let (started_tx, started_rx) = oneshot::channel();
            (
                Arc::new(Self {
                    state: Mutex::new(state),
                    writes: Mutex::new(Vec::new()),
                    pause_disable: true,
                    started: Mutex::new(Some(started_tx)),
                    released: Mutex::new(false),
                    release: Condvar::new(),
                }),
                started_rx,
            )
        }

        fn release_disable(&self) {
            *self.released.lock().unwrap() = true;
            self.release.notify_all();
        }

        fn current(&self) -> crate::client::system_proxy::ports::OsProxyConfig {
            self.state.lock().unwrap().clone()
        }

        fn writes(&self) -> Vec<crate::client::system_proxy::ports::OsProxyConfig> {
            self.writes.lock().unwrap().clone()
        }
    }

    impl crate::client::system_proxy::ports::OsProxyPort for LifecycleTestProxy {
        fn get(
            &self,
        ) -> Result<
            crate::client::system_proxy::ports::OsProxyConfig,
            crate::client::system_proxy::ports::OsProxyError,
        > {
            Ok(self.current())
        }

        fn set(
            &self,
            config: &crate::client::system_proxy::ports::OsProxyConfig,
        ) -> Result<(), crate::client::system_proxy::ports::OsProxyError> {
            if self.pause_disable
                && !config.enable
                && let Some(started) = self.started.lock().unwrap().take()
            {
                let _ = started.send(());
                let mut released = self.released.lock().unwrap();
                while !*released {
                    released = self.release.wait(released).unwrap();
                }
            }
            self.writes.lock().unwrap().push(config.clone());
            *self.state.lock().unwrap() = config.clone();
            Ok(())
        }

        fn default_bypass(&self) -> &'static str {
            "localhost"
        }
    }

    struct LifecycleTestAutoLaunch;

    impl crate::client::system_proxy::ports::AutoLaunchPort for LifecycleTestAutoLaunch {
        fn is_enabled(&self) -> Result<bool, crate::client::system_proxy::ports::AutoLaunchError> {
            Ok(false)
        }

        fn set_enabled(
            &self,
            _enabled: bool,
        ) -> Result<(), crate::client::system_proxy::ports::AutoLaunchError> {
            Ok(())
        }
    }

    struct LifecycleTestPac;

    #[async_trait]
    impl crate::client::system_proxy::ports::PacPort for LifecycleTestPac {
        fn is_supported(&self) -> bool {
            false
        }

        async fn apply(
            &self,
            _url: &url::Url,
            _cancel: CancellationToken,
        ) -> Result<(), crate::client::system_proxy::ports::PacError> {
            Err(crate::client::system_proxy::ports::PacError::DownloadCancelled)
        }

        fn disable(&self) -> Result<(), crate::client::system_proxy::ports::PacError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingProxyActionNotifications {
        applications: Mutex<Vec<(bool, Vec<crate::client::effects::plan::EffectKind>)>>,
        ports: Mutex<Vec<Option<u16>>>,
    }

    impl crate::client::effects::ports::CommitNotifications for RecordingProxyActionNotifications {
        fn application_committed(
            &self,
            fields: crate::client::effects::plan::ApplicationEffectFields,
            kinds: Vec<crate::client::effects::plan::EffectKind>,
        ) {
            self.applications
                .lock()
                .unwrap()
                .push((fields.enable_system_proxy, kinds));
        }

        fn clash_committed(&self, _: crate::client::effects::plan::ClashEffectFields) {}

        fn profiles_committed(&self, _: Vec<chimera_config::profile::ProfileId>) {}

        fn runtime_bound(
            &self,
            ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
            _: bool,
        ) {
            self.ports
                .lock()
                .unwrap()
                .push(ports.map(|binding| binding.mixed_port));
        }

        fn publish_full(&self, _: Option<chimera_config::runtime::executor::ResolvedPortBindings>) {
        }
    }

    async fn spawn_lifecycle_system_proxy(
        os: Arc<dyn crate::client::system_proxy::ports::OsProxyPort>,
    ) -> (
        crate::client::system_proxy::SystemProxyClient,
        CancellationToken,
        TaskTracker,
    ) {
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = crate::client::system_proxy::SystemProxyClient::spawn(
            crate::client::system_proxy::SystemProxyArgs {
                os,
                auto_launch: Arc::new(LifecycleTestAutoLaunch),
                pac: Arc::new(LifecycleTestPac),
                schedule_guard_ticks: false,
                shutdown: shutdown.child_token(),
            },
            &tasks,
        )
        .await
        .unwrap();
        (client, shutdown, tasks)
    }

    fn stale_proxy_config() -> crate::client::system_proxy::ports::OsProxyConfig {
        crate::client::system_proxy::ports::OsProxyConfig {
            enable: true,
            host: "127.0.0.1".into(),
            port: 7890,
            bypass: "localhost".into(),
        }
    }

    async fn application_with_system_proxy(
        enabled: bool,
    ) -> (tempfile::TempDir, ApplicationClient) {
        let directory = tempfile::tempdir().unwrap();
        let config_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().join("application.yaml")).unwrap();
        let application = ApplicationClient::new(
            config_path,
            chimera_config::application::ChimeraAppConfig {
                enable_system_proxy: enabled,
                ..Default::default()
            },
            Arc::new(TestApplicationBridge),
        )
        .await
        .unwrap();
        (directory, application)
    }

    fn direct_lifecycle_client(
        application: ApplicationClient,
        core: Arc<dyn CoreLifecyclePort>,
    ) -> CoreLifecycleClient {
        direct_lifecycle_client_with_mixed_port(application, core, 7890)
    }

    fn direct_lifecycle_client_with_mixed_port(
        application: ApplicationClient,
        core: Arc<dyn CoreLifecyclePort>,
        mixed_port: u16,
    ) -> CoreLifecycleClient {
        CoreLifecycleClient::direct(
            core,
            application,
            ClashConfigClient::legacy_with(ClashConfig {
                mixed_port: chimera_config::clash::config::clash_strategy::PortStrategy {
                    kind: chimera_config::clash::config::clash_strategy::PortStrategyKind::Fixed,
                    start_port: mixed_port,
                },
                ..Default::default()
            })
            .unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
    }

    fn counted_mirror_persistence(
        calls: Arc<AtomicU8>,
    ) -> Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync> {
        Arc::new(move || {
            calls.fetch_add(1, AtomicOrdering::AcqRel);
            Ok(())
        })
    }

    #[tokio::test]
    async fn stale_proxy_lifecycle_rejects_changed_core_without_host_or_mirror_writes() {
        use crate::client::system_proxy::SystemProxyActionError;

        let (_directory, application) = application_with_system_proxy(true).await;
        let os = LifecycleTestProxy::new(stale_proxy_config());
        let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
        let core = Arc::new(BindingCore {
            running: AtomicBool::new(false),
        });
        let lifecycle = direct_lifecycle_client(application, core.clone());
        let persist_calls = Arc::new(AtomicU8::new(0));
        let notifications = Arc::new(RecordingProxyActionNotifications::default());

        let error = lifecycle
            .disable_stale_system_proxy(
                system_proxy.clone(),
                7890,
                2,
                true,
                stale_proxy_config(),
                notifications.clone(),
                counted_mirror_persistence(persist_calls.clone()),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, SystemProxyActionError::StateChanged));

        core.running.store(true, AtomicOrdering::Release);
        let error = lifecycle
            .disable_stale_system_proxy(
                system_proxy,
                7890,
                1,
                true,
                stale_proxy_config(),
                notifications,
                counted_mirror_persistence(persist_calls.clone()),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, SystemProxyActionError::StateChanged));
        assert!(os.writes().is_empty());
        assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 0);

        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn stale_proxy_lifecycle_rejects_changed_mixed_port_without_host_or_mirror_writes() {
        use crate::client::system_proxy::SystemProxyActionError;

        let (_directory, application) = application_with_system_proxy(true).await;
        let os = LifecycleTestProxy::new(stale_proxy_config());
        let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
        let lifecycle = direct_lifecycle_client_with_mixed_port(
            application,
            Arc::new(BindingCore {
                running: AtomicBool::new(false),
            }),
            7891,
        );
        let persist_calls = Arc::new(AtomicU8::new(0));

        let error = lifecycle
            .disable_stale_system_proxy(
                system_proxy,
                7890,
                1,
                true,
                stale_proxy_config(),
                Arc::new(RecordingProxyActionNotifications::default()),
                counted_mirror_persistence(persist_calls.clone()),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, SystemProxyActionError::StateChanged));
        assert!(os.writes().is_empty());
        assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 0);

        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn stale_proxy_lifecycle_commits_and_persists_preference_after_host_disable() {
        let (_directory, application) = application_with_system_proxy(true).await;
        let os = LifecycleTestProxy::new(stale_proxy_config());
        let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
        let lifecycle = direct_lifecycle_client(
            application.clone(),
            Arc::new(BindingCore {
                running: AtomicBool::new(false),
            }),
        );
        let persist_calls = Arc::new(AtomicU8::new(0));
        let notifications = Arc::new(RecordingProxyActionNotifications::default());

        lifecycle
            .disable_stale_system_proxy(
                system_proxy,
                7890,
                1,
                true,
                stale_proxy_config(),
                notifications.clone(),
                counted_mirror_persistence(persist_calls.clone()),
            )
            .await
            .unwrap();

        assert!(!application.get_typed().enable_system_proxy);
        assert!(!os.current().enable);
        assert_eq!(os.writes().len(), 1);
        assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 1);
        assert_eq!(
            notifications.applications.lock().unwrap().as_slice(),
            [(
                false,
                vec![crate::client::effects::plan::EffectKind::SystemProxy]
            )]
        );
        assert_eq!(notifications.ports.lock().unwrap().as_slice(), [None]);

        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn stale_proxy_lifecycle_reports_mirror_failure_without_reenabling_proxy() {
        use crate::client::system_proxy::SystemProxyActionError;

        let (_directory, application) = application_with_system_proxy(true).await;
        let os = LifecycleTestProxy::new(stale_proxy_config());
        let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
        let lifecycle = direct_lifecycle_client(
            application.clone(),
            Arc::new(BindingCore {
                running: AtomicBool::new(false),
            }),
        );
        let notifications = Arc::new(RecordingProxyActionNotifications::default());
        let persist_calls = Arc::new(AtomicU8::new(0));
        let persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync> = {
            let persist_calls = persist_calls.clone();
            Arc::new(move || {
                persist_calls.fetch_add(1, AtomicOrdering::AcqRel);
                anyhow::bail!("injected mirror failure")
            })
        };

        let error = lifecycle
            .disable_stale_system_proxy(
                system_proxy,
                7890,
                1,
                true,
                stale_proxy_config(),
                notifications.clone(),
                persist_legacy_mirror,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, SystemProxyActionError::PartialApply(_)));
        assert!(!application.get_typed().enable_system_proxy);
        assert!(!os.current().enable);
        assert_eq!(os.writes().len(), 1);
        assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 1);
        assert_eq!(notifications.applications.lock().unwrap().len(), 1);

        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn stale_proxy_lifecycle_conflict_keeps_latest_preference_without_stale_rollback() {
        use crate::client::system_proxy::SystemProxyActionError;

        let (_directory, application) = application_with_system_proxy(true).await;
        let (os, disable_started) = LifecycleTestProxy::pause_first_disable(stale_proxy_config());
        let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
        let lifecycle = direct_lifecycle_client(
            application.clone(),
            Arc::new(BindingCore {
                running: AtomicBool::new(false),
            }),
        );
        let notifications = Arc::new(RecordingProxyActionNotifications::default());
        let persist_calls = Arc::new(AtomicU8::new(0));
        let action = {
            let lifecycle = lifecycle.clone();
            let system_proxy = system_proxy.clone();
            let notifications = notifications.clone();
            let persist_calls = persist_calls.clone();
            tokio::spawn(async move {
                lifecycle
                    .disable_stale_system_proxy(
                        system_proxy,
                        7890,
                        1,
                        true,
                        stale_proxy_config(),
                        notifications,
                        counted_mirror_persistence(persist_calls),
                    )
                    .await
            })
        };
        disable_started.await.unwrap();

        application
            .patch_typed(ChimeraAppConfigPatch {
                enable_system_proxy: Some(false),
                ..Default::default()
            })
            .await
            .unwrap();
        os.release_disable();

        let error = action.await.unwrap().unwrap_err();
        assert!(matches!(error, SystemProxyActionError::StateChanged));
        assert!(!application.get_typed().enable_system_proxy);
        assert!(!os.current().enable);
        assert_eq!(os.writes().len(), 1, "stale recovery must not re-enable");
        assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 0);
        assert_eq!(
            notifications.applications.lock().unwrap().last().unwrap().0,
            false
        );
        assert_eq!(notifications.ports.lock().unwrap().as_slice(), [None]);

        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
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

        async fn report_endpoint_down(
            &self,
            _transition: &mut dyn ServiceTransitionLease,
        ) -> anyhow::Result<()> {
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

    struct FailingInstaller {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingProgress {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl ports::BinaryInstaller for RecordingInstaller {
        async fn install(&self, _artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("install");
            Ok(())
        }
    }

    #[async_trait]
    impl ports::BinaryInstaller for FailingInstaller {
        async fn install(&self, _artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("install");
            anyhow::bail!("test install failure")
        }
    }

    impl BinaryInstallProgress for RecordingProgress {
        fn restarting(&self) {
            self.events.lock().unwrap().push("restarting");
        }

        fn finished(&self, error: Option<&str>) {
            self.events.lock().unwrap().push(if error.is_some() {
                "finished-error"
            } else {
                "finished"
            });
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

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select-after-panic");
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for BindingCore {
        async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
            self.running.store(true, AtomicOrdering::Release);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.running.store(false, AtomicOrdering::Release);
            Ok(())
        }

        async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
            self.running.store(true, AtomicOrdering::Release);
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: if self.running.load(AtomicOrdering::Acquire) {
                    CoreState::Running
                } else {
                    CoreState::Stopped(None)
                },
                state_changed_at: 1,
                run_type: RunType::Normal,
            })
        }

        async fn active_runtime_receipt(
            &self,
        ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
            Ok(self.running.load(AtomicOrdering::Acquire).then(|| {
                Arc::new(crate::client::ports::test_runtime_receipt_with_mixed_port(
                    7892,
                ))
            }))
        }

        fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
            crate::config::clash::ClashInfo {
                port: 7892,
                server: "127.0.0.1".into(),
                secret: None,
            }
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _: bool) {}
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

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("lower-select");
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        fn outcome_uncertain(&self) -> bool {
            self.uncertain.load(AtomicOrdering::Acquire)
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
            self.stop_started.notify_one();
            self.release_stop.notified().await;
            self.events.lock().unwrap().push("stop-end");
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select");
            Ok(())
        }
        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: self.run_type,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            Some(self.recovery_notify.clone())
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
            self.local.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Running,
                state_changed_at: 0,
                run_type: if self.local.load(std::sync::atomic::Ordering::Relaxed) {
                    RunType::Normal
                } else {
                    RunType::Service
                },
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for ServiceModeRecordingCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            target_core: ClashCore,
            run_type: RunType,
        ) -> anyhow::Result<()> {
            let app = self.application.get_typed();
            self.observations.lock().unwrap().push((
                run_type,
                target_core,
                app.enable_service_mode,
            ));
            if self
                .mutate_application_on_first_reconcile
                .swap(false, AtomicOrdering::AcqRel)
            {
                self.application
                    .patch_typed(ChimeraAppConfigPatch {
                        core: Some(ClashCore::ClashRsAlpha),
                        enable_builtin_enhanced: Some(false),
                        ..Default::default()
                    })
                    .await?;
            }
            if self
                .fail_first_reconcile
                .swap(false, AtomicOrdering::AcqRel)
            {
                anyhow::bail!("injected Service Mode reconcile failure");
            }
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for ApplicationPatchRecordingCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            anyhow::bail!("application patch must pass its typed runtime candidate")
        }

        async fn reconcile_profiles(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
            _profiles: Arc<chimera_config::profile::Profiles>,
            app: chimera_config::application::ChimeraAppConfig,
            _staged_content: std::collections::BTreeMap<String, String>,
        ) -> anyhow::Result<()> {
            self.observations.lock().unwrap().push((
                app.enable_builtin_enhanced,
                self.application.get_typed().enable_builtin_enhanced,
            ));
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[tokio::test]
    async fn service_mode_config_patch_reconciles_before_persisting_candidate() {
        let application = ApplicationClient::static_state(
            chimera_config::application::ChimeraAppConfig::default(),
        );
        let observations = Arc::new(Mutex::new(Vec::new()));
        let workflow = CoreLifecycleWorkflow::new(
            application.clone(),
            ClashConfigClient::legacy().unwrap(),
            None,
            Arc::new(ServiceModeRecordingCore {
                application: application.clone(),
                observations: observations.clone(),
                mutate_application_on_first_reconcile: AtomicBool::new(false),
                fail_first_reconcile: AtomicBool::new(false),
            }),
            Arc::new(RecordingInstaller {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        workflow
            .patch_application_config(
                ChimeraAppConfigPatch {
                    enable_service_mode: Some(true),
                    ..Default::default()
                },
                true,
            )
            .await
            .unwrap();

        assert_eq!(
            *observations.lock().unwrap(),
            [(
                RunType::Service,
                chimera_config::application::ChimeraAppConfig::default().core,
                false
            )]
        );
        assert!(application.get_typed().enable_service_mode);
    }

    #[tokio::test]
    async fn builtin_enhancement_patch_reconciles_candidate_before_persisting() {
        let application = ApplicationClient::static_state(
            chimera_config::application::ChimeraAppConfig::default(),
        );
        let profiles_dir = tempfile::tempdir().unwrap();
        let profiles_path =
            camino::Utf8PathBuf::from_path_buf(profiles_dir.path().join("profiles.yaml")).unwrap();
        let profiles_manager = chimera_core::state::PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(profiles_path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .unwrap();
        let observations = Arc::new(Mutex::new(Vec::new()));
        let workflow = CoreLifecycleWorkflow::new(
            application.clone(),
            ClashConfigClient::legacy().unwrap(),
            Some(profiles_manager.snapshot_handle()),
            Arc::new(ApplicationPatchRecordingCore {
                application: application.clone(),
                observations: observations.clone(),
            }),
            Arc::new(RecordingInstaller {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        workflow
            .patch_application_config(
                ChimeraAppConfigPatch {
                    enable_builtin_enhanced: Some(false),
                    ..Default::default()
                },
                false,
            )
            .await
            .unwrap();

        assert_eq!(*observations.lock().unwrap(), [(false, true)]);
        assert!(!application.get_typed().enable_builtin_enhanced);
    }

    #[tokio::test]
    async fn service_mode_config_patch_restores_latest_runtime_after_version_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let config_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().join("application.yaml")).unwrap();
        let application = ApplicationClient::new(
            config_path,
            chimera_config::application::ChimeraAppConfig::default(),
            Arc::new(TestApplicationBridge),
        )
        .await
        .unwrap();
        let observations = Arc::new(Mutex::new(Vec::new()));
        let workflow = CoreLifecycleWorkflow::new(
            application.clone(),
            ClashConfigClient::legacy().unwrap(),
            None,
            Arc::new(ServiceModeRecordingCore {
                application: application.clone(),
                observations: observations.clone(),
                mutate_application_on_first_reconcile: AtomicBool::new(true),
                fail_first_reconcile: AtomicBool::new(false),
            }),
            Arc::new(RecordingInstaller {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        let error = workflow
            .patch_application_config(
                ChimeraAppConfigPatch {
                    enable_service_mode: Some(true),
                    ..Default::default()
                },
                true,
            )
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("application config version conflict")
        );
        assert_eq!(
            *observations.lock().unwrap(),
            [
                (
                    RunType::Service,
                    chimera_config::application::ChimeraAppConfig::default().core,
                    false
                ),
                (RunType::Normal, ClashCore::ClashRsAlpha, false)
            ]
        );
        let latest = application.get_typed();
        assert!(!latest.enable_service_mode);
        assert!(!latest.enable_builtin_enhanced);
        assert_eq!(latest.core, ClashCore::ClashRsAlpha);
    }

    #[tokio::test]
    async fn service_mode_config_patch_restores_latest_runtime_after_candidate_failure() {
        let directory = tempfile::tempdir().unwrap();
        let config_path =
            camino::Utf8PathBuf::from_path_buf(directory.path().join("application.yaml")).unwrap();
        let application = ApplicationClient::new(
            config_path,
            chimera_config::application::ChimeraAppConfig::default(),
            Arc::new(TestApplicationBridge),
        )
        .await
        .unwrap();
        let observations = Arc::new(Mutex::new(Vec::new()));
        let workflow = CoreLifecycleWorkflow::new(
            application.clone(),
            ClashConfigClient::legacy().unwrap(),
            None,
            Arc::new(ServiceModeRecordingCore {
                application: application.clone(),
                observations: observations.clone(),
                mutate_application_on_first_reconcile: AtomicBool::new(true),
                fail_first_reconcile: AtomicBool::new(true),
            }),
            Arc::new(RecordingInstaller {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        let error = workflow
            .patch_application_config(
                ChimeraAppConfigPatch {
                    enable_service_mode: Some(true),
                    ..Default::default()
                },
                true,
            )
            .await
            .unwrap_err();

        assert!(format!("{error:#}").contains("injected Service Mode reconcile failure"));
        assert!(format!("{error:#}").contains("restored the latest persisted runtime"));
        assert_eq!(
            *observations.lock().unwrap(),
            [
                (
                    RunType::Service,
                    chimera_config::application::ChimeraAppConfig::default().core,
                    false
                ),
                (RunType::Normal, ClashCore::ClashRsAlpha, false)
            ]
        );
        assert!(!application.get_typed().enable_service_mode);
        assert!(!application.get_typed().enable_builtin_enhanced);
        assert_eq!(application.get_typed().core, ClashCore::ClashRsAlpha);
    }

    #[tokio::test]
    async fn mailbox_serializes_stop_and_core_selection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let stop_client = client.clone();
        let stop = tokio::spawn(async move { stop_client.stop_core().await.unwrap() });
        if tokio::time::timeout(Duration::from_secs(5), stop_started.notified())
            .await
            .is_err()
        {
            release_stop.notify_one();
            stop.abort();
            panic!("stop command did not reach the lifecycle actor");
        }
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = client.0.as_ref() else {
            panic!("production lifecycle client should own an actor");
        };
        let (reply, result) = tokio::sync::oneshot::channel();
        if actor_ref
            .send_message(Message::Request(Request {
                command: Command::SelectCore(ClashCore::Mihomo),
                response: Response {
                    id: 2,
                    reply: Some(reply.into()),
                },
                deferred_retry: None,
            }))
            .is_err()
        {
            release_stop.notify_one();
            panic!("failed to queue core selection");
        }
        release_stop.notify_one();
        tokio::time::timeout(Duration::from_secs(5), async {
            stop.await.unwrap();
            result.await.expect("selection reply was dropped").unwrap();
        })
        .await
        .expect("lifecycle mailbox did not finish queued commands");

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end", "select"]
        );
    }

    #[tokio::test]
    async fn lifecycle_reconcile_and_shutdown_publish_active_runtime_binding() {
        let runtime_bindings = Arc::new(RecordingRuntimeBindings::default());
        let core = Arc::new(BindingCore {
            running: AtomicBool::new(false),
        });
        let client = CoreLifecycleClient::spawn(
            core,
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();
        let mutations = crate::state::mutation::MutationCoordinator::pending_with_notifications(
            runtime_bindings.clone(),
        );
        mutations.connect(client.clone()).await.unwrap();

        client.select_core(ClashCore::Mihomo).await.unwrap();
        assert_eq!(*runtime_bindings.ports.lock().unwrap(), [Some(7892)]);

        client.shutdown().await.unwrap();
        assert_eq!(
            *runtime_bindings.ports.lock().unwrap(),
            [Some(7892), None],
            "stopping the active core must clear its published port"
        );
    }

    #[tokio::test]
    async fn stop_service_hands_core_back_to_local_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let local = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut app_state = chimera_config::application::ChimeraAppConfig::default();
        app_state.enable_service_mode = true;
        let client = CoreLifecycleClient::spawn_with_service(
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

        assert!(local.load(std::sync::atomic::Ordering::Relaxed));
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
    async fn service_control_commands_share_the_lifecycle_actor() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_service(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
        client.update_service().await.unwrap();
        client.start_service(Duration::from_secs(1)).await.unwrap();
        client
            .restart_service(Duration::from_secs(1))
            .await
            .unwrap();
        client.uninstall_service().await.unwrap();
        client.stop_service().await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "service-begin",
                "service-install",
                "service-probe",
                "service-begin",
                "service-update",
                "service-probe",
                "service-begin",
                "service-start",
                "service-probe",
                "service-begin",
                "service-restart",
                "service-probe",
                "service-begin",
                "service-uninstall",
                "service-confirm",
                "service-probe",
                "service-begin",
                "service-stop",
                "service-confirm",
                "service-probe"
            ]
        );
    }

    #[tokio::test]
    async fn probe_service_reads_through_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_service(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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

        assert_eq!(status.status, chimera_ipc::types::ServiceStatus::Stopped);
        assert_eq!(status.phase, ServicePhase::DaemonStopped);
        assert_eq!(status.name.as_ref(), "chimera-service");
        assert_eq!(client.service_status().phase, ServicePhase::DaemonStopped);
        assert_eq!(events.lock().unwrap().as_slice(), ["service-probe"]);
    }

    #[tokio::test]
    async fn health_observation_updates_cached_service_status_without_reprobe() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn_with_service(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
    async fn start_and_restart_service_confirm_ready_before_returning() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut app_state = chimera_config::application::ChimeraAppConfig::default();
        app_state.enable_service_mode = true;
        let client = CoreLifecycleClient::spawn_with_service(
            Arc::new(ServiceHandoffCore {
                events: events.clone(),
                local: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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

        client.start_service(Duration::from_secs(1)).await.unwrap();
        client
            .restart_service(Duration::from_secs(1))
            .await
            .unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "service-begin",
                "service-start",
                "service-ready",
                "service-ready",
                "service-probe",
                "service-begin",
                "service-restart",
                "service-ready",
                "service-ready",
                "service-probe"
            ]
        );
    }

    #[tokio::test]
    async fn shutdown_is_idempotent_and_blocks_follow_up_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_stop = Arc::new(Notify::new());
        release_stop.notify_one();
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop,
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        client.shutdown().await.unwrap();
        client.shutdown().await.unwrap();

        assert!(client.status().shutting_down);
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
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
            .expect("first stop should become the active operation");

        let select_client = client.clone();
        let select =
            tokio::spawn(async move { select_client.select_core(ClashCore::Mihomo).await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !client.status().queued.contains(&2) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("core selection should be queued behind the active stop");

        let shutdown_client = client.clone();
        let shutdown = tokio::spawn(async move { shutdown_client.shutdown().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !client.status().shutting_down {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown should close and drain the pending queue");

        let error = select
            .await
            .expect("drained selection should settle")
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("core lifecycle is shutting down")
        );
        assert!(
            !shutdown.is_finished(),
            "shutdown must wait for the active stop"
        );

        release_stop.notify_one();
        stop.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
            .await
            .expect("shutdown should run one final stop after the active operation");
        assert!(!shutdown.is_finished(), "shutdown reply follows final stop");

        release_stop.notify_one();
        shutdown.await.unwrap().unwrap();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end", "stop-start", "stop-end"]
        );
    }

    #[tokio::test]
    async fn pending_queue_rejects_overflow_and_finishes_rejected_binary_update() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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

        let progress_events = Arc::new(Mutex::new(Vec::new()));
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let error = client
            .replace_core_binary(PreparedCoreBinary {
                target: ClashCore::Mihomo,
                source: staging.path().join("staged-core"),
                destination: staging.path().join("installed-core"),
                staging,
                progress: Arc::new(RecordingProgress {
                    events: progress_events.clone(),
                }),
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("core lifecycle queue is full"));
        assert_eq!(
            progress_events.lock().unwrap().as_slice(),
            ["finished-error"]
        );
        assert!(
            client
                .status()
                .completed
                .iter()
                .any(|entry| entry.error.as_deref() == Some("core lifecycle queue is full"))
        );

        release_stop.notify_one();
        stop.await.unwrap().unwrap();
        for request in queued {
            request.await.unwrap().unwrap();
        }

        assert!(client.status().queued.is_empty());
        let events = events.lock().unwrap();
        assert_eq!(
            events.iter().filter(|event| **event == "select").count(),
            MAX_PENDING
        );
    }

    #[tokio::test]
    async fn startup_reconcile_waits_for_lifecycle_completion() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let completed = client
            .startup_reconcile()
            .await
            .expect("startup reconcile should complete through the lifecycle actor");
        completed
            .0
            .expect("startup reconcile should return its terminal operation result");
        assert!(events.lock().unwrap().contains(&"rebuild"));
    }

    #[tokio::test]
    async fn timed_out_startup_reconcile_has_no_terminal_completion() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
            .expect("stop should occupy the lifecycle actor");

        let startup = client
            .startup_reconcile_with_timeout(Some(Duration::from_millis(1)))
            .await;
        assert!(startup.is_err());
        assert!(!events.lock().unwrap().contains(&"rebuild"));

        release_stop.notify_one();
        stop.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if events.lock().unwrap().contains(&"rebuild") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("timed-out startup reconcile remains queued and later completes");
    }

    #[tokio::test]
    async fn crash_recovery_signal_enters_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovery_notify = Arc::new(Notify::new());
        let _client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: recovery_notify.clone(),
                run_type: RunType::Normal,
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
    async fn timed_out_wait_does_not_cancel_admitted_operation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
        tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
            .await
            .expect("timed-out operation should still be executing");

        assert_eq!(client.status().active, Some(1));
        let queued_client = client.clone();
        let queued = tokio::spawn(async move {
            queued_client.select_core(ClashCore::Mihomo).await.unwrap();
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.status().queued.contains(&2) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("second lifecycle operation should be visible in the queue");
        assert_eq!(client.status().active, Some(1));
        release_stop.notify_one();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.status().completed.iter().any(|entry| entry.id == 2) {
                    break;
                }
                tokio::task::yield_now().await;
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
        assert!(status.active.is_none());
        assert!(status.queued.is_empty());
        queued.await.unwrap();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end", "select"]
        );
    }

    #[tokio::test]
    async fn burst_runtime_dirty_requests_coalesce_into_one_reconcile() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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

        let progress_events = Arc::new(Mutex::new(Vec::new()));
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let error = client
            .replace_core_binary(PreparedCoreBinary {
                target: ClashCore::Mihomo,
                source: staging.path().join("source"),
                destination: staging.path().join("destination"),
                staging,
                progress: Arc::new(RecordingProgress {
                    events: progress_events.clone(),
                }),
            })
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("previous core lifecycle operation has an uncertain outcome")
        );
        assert_eq!(
            progress_events.lock().unwrap().as_slice(),
            ["finished-error"]
        );

        let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("previous core lifecycle operation has an uncertain outcome")
        );
        assert_eq!(events.lock().unwrap().as_slice(), ["panic-stop"]);

        let status = client.status();
        assert!(status.uncertain);
        assert_eq!(status.completed.len(), 3);
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
        assert!(
            status.completed[2]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("previous core lifecycle operation"))
        );
    }

    #[tokio::test]
    async fn endpoint_down_restart_budget_latches_exhausted_in_cached_status() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        });
        let client = CoreLifecycleClient::spawn_with_service(
            Arc::new(RecordingCore {
                events: Arc::new(Mutex::new(Vec::new())),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
            client.request_service_endpoint_down().await;
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

        client.request_service_endpoint_down().await;
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
    async fn reconcile_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
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
    async fn binary_replacement_runs_stop_install_restart_in_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = application.get_typed().core;
        let release_stop = Arc::new(Notify::new());
        release_stop.notify_one();
        let runtime_paths =
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root"));
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop,
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Service,
            }),
            application,
            ClashConfigClient::legacy().unwrap(),
            runtime_paths,
            Arc::new(RecordingInstaller {
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

    #[tokio::test]
    async fn binary_install_failure_finishes_update_without_restarting_core() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = application.get_typed().core;
        let release_stop = Arc::new(Notify::new());
        release_stop.notify_one();
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop,
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            application,
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(FailingInstaller {
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

        assert!(client.replace_core_binary(artifact).await.is_err());

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end", "install", "finished-error"]
        );
    }
}

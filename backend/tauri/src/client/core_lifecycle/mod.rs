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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[cfg(test)]
use super::application_workflow::startup::StartupWorkflowState;
use super::{
    ChimeraClient,
    application::ApplicationClient,
    application_workflow::startup::{StartupOutcome, StartupReport},
    application_workflow::workflow::ApplicationWorkflow,
    clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};
pub(in crate::client) use workflow::{Command, CoreLifecycleWorkflow};

const CALL_WAIT: Duration = Duration::from_secs(180);
const DIRTY_WINDOW: Duration = Duration::from_millis(500);
const MAX_PENDING: usize = 32;
const COMPLETED_LIMIT: usize = 32;

type OperationId = u64;

struct CompletedLifecycleOperation {
    result: anyhow::Result<()>,
    report: StartupReport,
}

fn completed_from_startup_report(report: StartupReport) -> CompletedLifecycleOperation {
    let result = report
        .outcome
        .failure_reason()
        .map_or(Ok(()), |reason| Err(anyhow::anyhow!(reason.to_owned())));
    CompletedLifecycleOperation { result, report }
}

fn is_source_only_application_patch(command: &Command) -> bool {
    matches!(
        command,
        Command::PatchApplicationConfig {
            patch,
            service_connected,
        }
            if !crate::client::application_workflow::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            )
    )
}

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

use chimera_core::actor_v2::{
    service_policy::{ServicePhase, ServiceRestartPolicySnapshot},
    service_status::{ServiceHostStatus, ServiceHostWatch},
};

// OS-specific runtime ownership evidence enters the core through an input.
#[cfg(test)]
fn classify_service_probe(info: chimera_ipc::types::StatusInfo<'static>) -> ServiceHostStatus {
    let owned = crate::core::service::is_service_runtime_owned(&info);
    ServiceHostStatus::from_probe(info, owned)
}

fn startup_recovery_evidence_changed(
    previous: &ServiceHostStatus,
    current: &ServiceHostStatus,
    owner_unproven: bool,
) -> bool {
    let readiness_advanced = current.phase.readiness() == Some(true)
        && current.ready_generation != previous.ready_generation;
    let owner_evidence_changed = owner_unproven
        && crate::client::application_workflow::startup::ServiceEvidence::from_status(previous)
            != crate::client::application_workflow::startup::ServiceEvidence::from_status(current);
    readiness_advanced || owner_evidence_changed
}

#[allow(unused_imports)]
pub(crate) use adapters::{LegacyCoreBridge, LegacyRunningConfigBridge, LegacyServiceBridge};
#[allow(unused_imports)]
pub(crate) use ports::{
    BinaryInstallProgress, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
    RunningConfigPort, RuntimeTransformDiagnostics, ServiceLifecyclePort, ServiceTransitionLease,
};

enum Message {
    ConnectApplicationWorkflow {
        workflow: ApplicationWorkflow,
        reply: RpcReplyPort<anyhow::Result<()>>,
        registered: Option<tokio::sync::oneshot::Sender<()>>,
    },
    Request(Request),
    Completed {
        id: OperationId,
        workflow: ApplicationWorkflow,
        result: anyhow::Result<()>,
        workflow_panicked: bool,
        lower_outcome_uncertain: bool,
        startup_owner_unproven: bool,
        service_restart_policy: ServiceRestartPolicySnapshot,
        service_probe: Option<Result<chimera_ipc::types::StatusInfo<'static>, String>>,
        retry_reconcile_on_failure: bool,
        recover: bool,
        shutdown: bool,
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
    StartupRetryDue {
        identity: String,
    },
    StartupRetryLeaseFailed {
        identity: String,
        reason: String,
        #[cfg(test)]
        observer: Option<tokio::sync::oneshot::Sender<()>>,
    },
    #[cfg(test)]
    ArmStartupRetryLeaseFailureObserver {
        observer: tokio::sync::oneshot::Sender<()>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    #[cfg(test)]
    ReadStartupRetryScheduleForTest {
        reply: RpcReplyPort<Option<(String, tokio::time::Instant)>>,
    },
    #[cfg(test)]
    SeedUnresolvedApplicationAttemptForTest {
        reply: RpcReplyPort<anyhow::Result<chimera_core_manager::OperationId>>,
    },
    #[cfg(test)]
    SeedUnresolvedSourceDecisionForTest {
        decision: chimera_core::state::DecisionHandle,
        reply: RpcReplyPort<anyhow::Result<chimera_core_manager::OperationId>>,
    },
    #[cfg(test)]
    SeedUnresolvedAttemptWithDeferredRetryForTest {
        decision: chimera_core::state::DecisionHandle,
        reply: RpcReplyPort<anyhow::Result<(chimera_core_manager::OperationId, String)>>,
    },
    #[cfg(test)]
    SeedUnresolvedCommittedTargetForTest {
        decision: chimera_core::state::DecisionHandle,
        reply: RpcReplyPort<anyhow::Result<chimera_core_manager::OperationId>>,
    },
    #[cfg(test)]
    ReadApplicationRecoveryStateForTest {
        reply: RpcReplyPort<(
            Option<(chimera_core_manager::OperationId, bool)>,
            Option<(String, tokio::time::Instant)>,
            Option<(u8, u32)>,
        )>,
    },
    DeferredRetry {
        digest: String,
    },
    ReadStartupReport {
        reply: RpcReplyPort<Option<StartupReport>>,
    },
}

struct ApplicationWorkflowActor;

struct ApplicationWorkflowState {
    workflow: Option<ApplicationWorkflow>,
    pending_mutation_connection: Option<(ApplicationWorkflow, RpcReplyPort<anyhow::Result<()>>)>,
    active: Option<ActiveOperation>,
    pending: VecDeque<Request>,
    pending_startup_retry_lease_failures: VecDeque<(String, String)>,
    #[cfg(test)]
    startup_retry_lease_failure_observer: Option<tokio::sync::oneshot::Sender<()>>,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: ServiceHostWatch,
    service: Arc<dyn ServiceLifecyclePort>,
    uncertain: bool,
    startup_owner_unproven: bool,
    startup_owner_unproven_shared: Arc<AtomicBool>,
    shutting_down: bool,
    shutdown_result: Option<Option<String>>,
    shutdown_waiters: Vec<Response>,
    dirty: bool,
    dirty_tick_scheduled: bool,
}

fn record_startup_retry_lease_failure(
    workflow: Option<&mut ApplicationWorkflow>,
    pending: &mut VecDeque<(String, String)>,
    identity: String,
    reason: String,
) {
    if let Some(workflow) = workflow {
        workflow.defer_startup_retry(&identity, reason);
    } else {
        pending.push_back((identity, reason));
    }
}

fn apply_pending_startup_retry_lease_failures(
    workflow: Option<&mut ApplicationWorkflow>,
    pending: &mut VecDeque<(String, String)>,
) {
    let Some(workflow) = workflow else {
        return;
    };
    while let Some((identity, reason)) = pending.pop_front() {
        workflow.defer_startup_retry(&identity, reason);
    }
}

struct ApplicationWorkflowArgs {
    workflow: ApplicationWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: ServiceHostWatch,
    service: Arc<dyn ServiceLifecyclePort>,
    startup_owner_unproven_shared: Arc<AtomicBool>,
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
        let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
        self.service_status.observe_probe(info, runtime_owned, None);
    }

    fn publish_service_status_with_policy(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
        policy: ServiceRestartPolicySnapshot,
    ) {
        let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
        self.service_status
            .observe_probe(info, runtime_owned, Some(policy));
    }

    fn publish_service_phase(&self, phase: ServicePhase) {
        self.service_status.publish_phase(phase);
    }

    fn publish_service_probe_failure(&self) {
        self.service_status.probe_failed();
    }

    async fn refresh_service_status(&mut self) -> anyhow::Result<ServiceHostStatus> {
        let workflow = self
            .workflow
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("core lifecycle workflow is busy"))?;
        let lifecycle = workflow.lifecycle();
        let policy = lifecycle.service_restart_policy();
        match lifecycle.probe_service().await {
            Ok(info) => {
                let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
                Ok(self
                    .service_status
                    .observe_probe(info, runtime_owned, Some(policy)))
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

    fn reschedule_deferred_retry(
        &mut self,
        myself: &ActorRef<Message>,
        digest: &str,
        delay: Duration,
    ) {
        let next_attempt = self
            .workflow
            .as_mut()
            .and_then(|workflow| workflow.reschedule_deferred_retry(digest, delay));
        if let Some(next_attempt) = next_attempt {
            self.schedule_deferred_retry(myself, digest.to_owned(), next_attempt);
        }
    }

    fn schedule_startup_retry(
        &self,
        myself: &ActorRef<Message>,
        identity: String,
        next_attempt: tokio::time::Instant,
    ) {
        let actor = myself.clone();
        tokio::spawn(async move {
            tokio::time::sleep_until(next_attempt).await;
            let _ = actor.cast(Message::StartupRetryDue { identity });
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
        status.uncertain = self.uncertain
            || self.startup_owner_unproven
            || self.startup_owner_unproven_shared.load(Ordering::Acquire);
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
        let mut allowed = VecDeque::new();
        while let Some(request) = self.pending.pop_front() {
            if matches!(&request.command, Command::CacheStartupUnsettled { .. })
                || is_source_only_application_patch(&request.command)
            {
                allowed.push_back(request);
            } else {
                self.reject(request, anyhow::anyhow!(
                    "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                ));
            }
        }
        self.pending = allowed;
    }

    fn reject_pending_for_unproven_startup_owner(&mut self, myself: &ActorRef<Message>) {
        if !(self.startup_owner_unproven
            || self.startup_owner_unproven_shared.load(Ordering::Acquire))
            || self.shutting_down
        {
            return;
        }
        let mut allowed = VecDeque::new();
        while let Some(request) = self.pending.pop_front() {
            if matches!(
                &request.command,
                Command::StartupReconcile { .. }
                    | Command::RecoverStartup { .. }
                    | Command::RetryStartup { .. }
                    | Command::CacheStartupUnsettled { .. }
            ) || is_source_only_application_patch(&request.command)
            {
                allowed.push_back(request);
            } else {
                if let Some(digest) = request.deferred_retry.as_deref() {
                    self.reschedule_deferred_retry(myself, digest, Duration::from_secs(5));
                }
                self.reject(
                    request,
                    anyhow::anyhow!(
                        "startup has not established the Core owner; retry startup reconciliation before runtime mutations"
                    ),
                );
            }
        }
        self.pending = allowed;
    }

    fn reject_pending_for_application_recovery(&mut self, myself: &ActorRef<Message>) {
        let isolated = self
            .workflow
            .as_ref()
            .is_some_and(ApplicationWorkflow::has_unresolved_attempt);
        if !isolated || self.shutting_down {
            return;
        }
        let mut allowed = VecDeque::new();
        while let Some(request) = self.pending.pop_front() {
            let can_recover = self
                .workflow
                .as_ref()
                .is_some_and(|workflow| workflow.recovery_command_allowed(&request.command));
            if can_recover || matches!(&request.command, Command::Shutdown) {
                allowed.push_back(request);
            } else {
                if let Some(digest) = request.deferred_retry.as_deref() {
                    self.reschedule_deferred_retry(myself, digest, Duration::from_secs(5));
                }
                self.reject(
                    request,
                    anyhow::anyhow!(
                        "an application runtime attempt is unresolved; run its explicit recovery before another command"
                    ),
                );
            }
        }
        self.pending = allowed;
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
        self.reject_pending_for_unproven_startup_owner(myself);
        self.reject_pending_for_application_recovery(myself);

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

        let Some(mut workflow) = self.workflow.take() else {
            tracing::error!("core lifecycle workflow missing without an active operation");
            self.reject(
                request,
                anyhow::anyhow!("core lifecycle workflow unavailable"),
            );
            self.publish();
            return;
        };

        let profile_mutation = matches!(&request.command, Command::ProfileMutation(_));
        if profile_mutation && !workflow.mutation_connected() {
            self.workflow = Some(workflow);
            self.reject(
                request,
                anyhow::anyhow!("application workflow is not connected"),
            );
            self.publish();
            return;
        }

        let id = request.response.id;
        let shutdown = matches!(&request.command, Command::Shutdown);
        let deferred_retry = request.deferred_retry.clone();
        let application_runtime_reconcile = matches!(
            &request.command,
            Command::PatchApplicationConfig {
                patch,
                service_connected,
            }
                if crate::client::application_workflow::impact::application_patch_requires_reconcile(
                    patch,
                    *service_connected,
                )
        );
        let reconciled = matches!(
            &request.command,
            Command::Reconcile
                | Command::StartupReconcile { .. }
                | Command::RecoverStartup { .. }
                | Command::RetryStartup { .. }
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
                | Command::StartupReconcile { .. }
                | Command::RecoverStartup { .. }
                | Command::RetryStartup { .. }
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

        let command = request.command;
        let actor = myself.clone();
        let task = tokio::spawn(async move {
            let (result, workflow_panicked, request_runtime_rebuild) = match command {
                Command::ProfileMutation(mut mutation) => {
                    let run = AssertUnwindSafe(workflow.run_profile_mutation(&mut mutation))
                        .catch_unwind()
                        .await;
                    match run {
                        Ok((request_runtime_rebuild, settlement)) => (
                            settlement.map_err(anyhow::Error::msg),
                            false,
                            request_runtime_rebuild,
                        ),
                        Err(_) => {
                            let message = format!(
                                "application workflow operation {} panicked; runtime outcome requires recovery",
                                mutation.operation_id
                            );
                            workflow.mark_recovery_required(message.clone()).await;
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
                            (Err(anyhow::anyhow!(message)), true, false)
                        }
                    }
                }
                command => {
                    match AssertUnwindSafe(workflow.execute_lifecycle(command))
                        .catch_unwind()
                        .await
                    {
                        Ok(result) => (result, false, false),
                        Err(_) => {
                            let message =
                                "core lifecycle workflow panicked; execution state is uncertain";
                            workflow.mark_recovery_required(message.into()).await;
                            (Err(anyhow::anyhow!(message)), true, false)
                        }
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
            let lifecycle = workflow.lifecycle();
            let lower_outcome_uncertain = lifecycle.outcome_uncertain();
            let startup_owner_unproven = workflow.startup_owner_unproven();
            let service_restart_policy = lifecycle.service_restart_policy();
            let service_probe = if service_mutation && !workflow_panicked {
                Some(
                    match AssertUnwindSafe(lifecycle.probe_service())
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
                    startup_owner_unproven,
                    service_restart_policy,
                    service_probe,
                    retry_reconcile_on_failure,
                    recover,
                    shutdown,
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
            pending_mutation_connection: None,
            active: None,
            pending: VecDeque::new(),
            pending_startup_retry_lease_failures: VecDeque::new(),
            #[cfg(test)]
            startup_retry_lease_failure_observer: None,
            next_id: args.next_id,
            status: args.status,
            service_status: args.service_status,
            service: args.service,
            uncertain: false,
            startup_owner_unproven: false,
            startup_owner_unproven_shared: args.startup_owner_unproven_shared,
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
            Message::ConnectApplicationWorkflow {
                workflow: incoming,
                reply,
                registered,
            } => {
                let validation = if state.shutting_down {
                    Err(anyhow::anyhow!("core lifecycle is shutting down"))
                } else if state.pending_mutation_connection.is_some()
                    || state
                        .workflow
                        .as_ref()
                        .is_some_and(ApplicationWorkflow::mutation_connected)
                {
                    Err(anyhow::anyhow!("application workflow is already connected"))
                } else if state.workflow.is_none() && !reply.is_closed() {
                    state.pending_mutation_connection = Some((incoming, reply));
                    if let Some(registered) = registered {
                        let _ = registered.send(());
                    }
                    return Ok(());
                } else if let Some(workflow) = state.workflow.as_ref() {
                    workflow.validate_mutation_connection(&incoming)
                } else {
                    Err(anyhow::anyhow!("core lifecycle workflow is unavailable"))
                };
                if let Some(registered) = registered {
                    let _ = registered.send(());
                }
                match validation {
                    Ok(()) => {
                        if reply.send(Ok(())).is_ok() {
                            state
                                .workflow
                                .as_mut()
                                .expect("validated workflow remains available")
                                .attach_mutation_workflow(incoming)
                                .expect("validated application workflow connection attaches");
                        }
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Message::Request(mut request) => {
                let service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
                if let Command::PatchApplicationConfig {
                    service_connected: patch_service_connected,
                    ..
                } = &mut request.command
                {
                    *patch_service_connected = service_connected;
                }
                let source_only_application_patch =
                    is_source_only_application_patch(&request.command);
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
                } else if state.uncertain
                    && !source_only_application_patch
                    && !matches!(&request.command, Command::CacheStartupUnsettled { .. })
                {
                    state.reject(
                        request,
                        anyhow::anyhow!(
                            "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
                        ),
                    );
                } else if (state.startup_owner_unproven
                    || state.startup_owner_unproven_shared.load(Ordering::Acquire))
                    && !matches!(
                        &request.command,
                        Command::StartupReconcile { .. }
                            | Command::RecoverStartup { .. }
                            | Command::RetryStartup { .. }
                            | Command::CacheStartupUnsettled { .. }
                    )
                    && !source_only_application_patch
                {
                    state.reject(
                        request,
                        anyhow::anyhow!(
                            "startup has not established the Core owner; retry startup reconciliation before runtime mutations"
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
                startup_owner_unproven,
                service_restart_policy,
                service_probe,
                retry_reconcile_on_failure,
                recover,
                shutdown,
                request_runtime_rebuild,
                deferred_retry,
                reconciled,
            } => {
                if state.active.as_ref().map(|operation| operation.response.id) == Some(id) {
                    let refresh_runtime_binding = reconciled || service_probe.is_some() || shutdown;
                    let runtime_binding = if refresh_runtime_binding {
                        Some(workflow.lifecycle().effect_port_binding().await)
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
                    state.startup_owner_unproven = startup_owner_unproven;
                    state
                        .startup_owner_unproven_shared
                        .store(startup_owner_unproven, Ordering::Release);
                    if let Some((incoming, reply)) = state.pending_mutation_connection.take() {
                        let validation = if state.shutting_down {
                            Err(anyhow::anyhow!("core lifecycle is shutting down"))
                        } else if reply.is_closed() {
                            Err(anyhow::anyhow!(
                                "application workflow connection was cancelled before attachment"
                            ))
                        } else {
                            state
                                .workflow
                                .as_ref()
                                .expect("completed workflow was restored")
                                .validate_mutation_connection(&incoming)
                        };
                        match validation {
                            Ok(()) => {
                                if reply.send(Ok(())).is_ok() {
                                    state
                                        .workflow
                                        .as_mut()
                                        .expect("completed workflow was restored")
                                        .attach_mutation_workflow(incoming)
                                        .expect(
                                            "validated application workflow connection attaches",
                                        );
                                }
                            }
                            Err(error) => {
                                let _ = reply.send(Err(error));
                            }
                        }
                    }
                    if let Some(ports) = runtime_binding
                        && let Some(application_workflow) = state.workflow.as_ref()
                    {
                        application_workflow.runtime_bound(ports);
                    }
                    apply_pending_startup_retry_lease_failures(
                        state.workflow.as_mut(),
                        &mut state.pending_startup_retry_lease_failures,
                    );
                    if let Some((identity, next_attempt)) = state
                        .workflow
                        .as_ref()
                        .and_then(ApplicationWorkflow::startup_retry_schedule)
                    {
                        state.schedule_startup_retry(&myself, identity, next_attempt);
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
                        if let Some(application_workflow) = state.workflow.as_mut() {
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
                        if let Some(application_workflow) = state.workflow.as_mut() {
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
                            .workflow
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
            Message::ReadStartupReport { reply } => {
                let report = state
                    .workflow
                    .as_ref()
                    .and_then(ApplicationWorkflow::startup_report);
                let _ = reply.send(report);
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
            Message::StartupRetryDue { identity } => {
                if !state.uncertain && !state.shutting_down {
                    if state.workflow.is_none() {
                        state.schedule_startup_retry(
                            &myself,
                            identity,
                            tokio::time::Instant::now() + DIRTY_WINDOW,
                        );
                    } else {
                        let should_retry = state.workflow.as_mut().is_some_and(|workflow| {
                            workflow.startup_retry_due(&identity)
                                && workflow.begin_startup_retry(&identity)
                        });
                        if should_retry {
                            let service = state.service.clone();
                            let actor = myself.clone();
                            let operation_id = state.allocate_operation_id();
                            #[cfg(test)]
                            let observer = state.startup_retry_lease_failure_observer.take();
                            tokio::spawn(async move {
                                match service.begin_transition().await {
                                    Ok(transition) => {
                                        let request = Request {
                                            command: Command::RetryStartup {
                                                operation_id,
                                                identity,
                                                transition,
                                            },
                                            response: Response {
                                                id: operation_id,
                                                reply: None,
                                            },
                                            deferred_retry: None,
                                        };
                                        if actor.cast(Message::Request(request)).is_err() {
                                            tracing::warn!(
                                                operation_id,
                                                "failed to enqueue startup retry"
                                            );
                                        }
                                    }
                                    Err(error) => {
                                        let _ = actor.cast(Message::StartupRetryLeaseFailed {
                                            identity,
                                            reason: format!("could not acquire the Service transition lease: {error:#}"),
                                            #[cfg(test)]
                                            observer,
                                        });
                                    }
                                }
                            });
                        }
                    }
                }
            }
            Message::StartupRetryLeaseFailed {
                identity,
                reason,
                #[cfg(test)]
                observer,
            } => {
                record_startup_retry_lease_failure(
                    state.workflow.as_mut(),
                    &mut state.pending_startup_retry_lease_failures,
                    identity,
                    reason,
                );
                if let Some((identity, next_attempt)) = state
                    .workflow
                    .as_ref()
                    .and_then(ApplicationWorkflow::startup_retry_schedule)
                {
                    state.schedule_startup_retry(&myself, identity, next_attempt);
                }
                #[cfg(test)]
                if let Some(observer) = observer {
                    let _ = observer.send(());
                }
            }
            #[cfg(test)]
            Message::ArmStartupRetryLeaseFailureObserver { observer, reply } => {
                if state.startup_retry_lease_failure_observer.is_some() {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "a startup retry lease failure observer is already armed"
                    )));
                } else {
                    state.startup_retry_lease_failure_observer = Some(observer);
                    let _ = reply.send(Ok(()));
                }
            }
            #[cfg(test)]
            Message::ReadStartupRetryScheduleForTest { reply } => {
                let schedule = state
                    .workflow
                    .as_ref()
                    .and_then(ApplicationWorkflow::startup_retry_schedule);
                let _ = reply.send(schedule);
            }
            #[cfg(test)]
            Message::SeedUnresolvedApplicationAttemptForTest { reply } => {
                let result = state
                    .workflow
                    .as_mut()
                    .map(ApplicationWorkflow::seed_unresolved_lifecycle_attempt_for_tests);
                let _ = reply.send(
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable")),
                );
            }
            #[cfg(test)]
            Message::SeedUnresolvedSourceDecisionForTest { decision, reply } => {
                let result = state
                    .workflow
                    .as_mut()
                    .map(|workflow| workflow.seed_unresolved_source_decision_for_tests(decision));
                let _ = reply.send(
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable")),
                );
            }
            #[cfg(test)]
            Message::SeedUnresolvedAttemptWithDeferredRetryForTest { decision, reply } => {
                let result = state.workflow.as_mut().map(|workflow| {
                    workflow.seed_unresolved_attempt_with_deferred_retry_for_tests(decision)
                });
                let result =
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable"));
                if let Ok((_, digest)) = &result {
                    let id = state.allocate_operation_id();
                    state.pending.push_back(Request {
                        command: Command::RetryDeferredProfiles {
                            digest: digest.clone(),
                        },
                        response: Response { id, reply: None },
                        deferred_retry: Some(digest.clone()),
                    });
                }
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Message::SeedUnresolvedCommittedTargetForTest { decision, reply } => {
                let result = state
                    .workflow
                    .as_mut()
                    .map(|workflow| workflow.seed_unresolved_committed_target_for_tests(decision));
                let _ = reply.send(
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable")),
                );
            }
            #[cfg(test)]
            Message::ReadApplicationRecoveryStateForTest { reply } => {
                let state = state.workflow.as_ref().map_or(
                    (None, None, None),
                    ApplicationWorkflow::recovery_state_for_tests,
                );
                let _ = reply.send(state);
            }
            Message::DeferredRetry { digest } => {
                if state.uncertain || state.shutting_down {
                    if let Some(application_workflow) = state.workflow.as_mut() {
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
                } else if state.workflow.is_none() {
                    state.schedule_deferred_retry(
                        &myself,
                        digest,
                        tokio::time::Instant::now() + DIRTY_WINDOW,
                    );
                } else if state.active.is_some()
                    || !state.pending.is_empty()
                    || state
                        .workflow
                        .as_ref()
                        .is_some_and(ApplicationWorkflow::has_unresolved_attempt)
                {
                    state.reschedule_deferred_retry(&myself, &digest, Duration::from_secs(5));
                } else {
                    use crate::client::application_workflow::workflow::ProfileRuntime;

                    let core_status = match state.workflow.as_ref() {
                        Some(workflow) => workflow.lifecycle().core_status().await.ok(),
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
                                .workflow
                                .as_mut()
                                .is_some_and(|workflow| workflow.begin_deferred_retry(&digest));
                            if should_retry && !state.push_deferred_retry(digest.clone()) {
                                let next_attempt = state.workflow.as_mut().and_then(|workflow| {
                                    workflow
                                        .reschedule_deferred_retry(&digest, Duration::from_secs(1))
                                });
                                if let Some(next_attempt) = next_attempt {
                                    state.schedule_deferred_retry(&myself, digest, next_attempt);
                                }
                            }
                        }
                        Some(_) => {
                            if let Some(application_workflow) = state.workflow.as_mut() {
                                application_workflow.wait_for_deferred_dependency(&digest);
                            }
                        }
                        None => {
                            let next_attempt = state.workflow.as_mut().and_then(|workflow| {
                                workflow.reschedule_deferred_retry(&digest, Duration::from_secs(5))
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
        startup_owner_unproven: Arc<AtomicBool>,
        service_failed_generation: Arc<AtomicU64>,
        startup_recovery_pending: Arc<AtomicBool>,
    },
    #[cfg(test)]
    Direct {
        workflow: tokio::sync::Mutex<CoreLifecycleWorkflow>,
        service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        core: Arc<dyn CoreLifecyclePort>,
        startup_state: tokio::sync::Mutex<StartupWorkflowState>,
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
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
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
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        Self::spawn_with_profiles(
            core,
            application,
            clash,
            None,
            runtime_paths,
            installer,
            service,
            shutdown,
            &tasks,
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
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let recovery_notify = core.recovery_notify();
        let service_status = ServiceHostWatch::new();
        let service_status_rx = service_status.subscribe();
        let lifecycle = CoreLifecycleWorkflow::new(
            application,
            clash,
            profiles,
            core.clone(),
            installer,
            runtime_paths,
            service.clone(),
        )
        .with_service_status(service_status_rx.clone());
        let startup_owner_unproven = lifecycle.startup_owner_unproven_handle();
        #[cfg(not(test))]
        let startup_owner_unproven_signal = startup_owner_unproven.clone();
        let service_failed_generation = lifecycle.service_failed_generation_handle();
        let workflow = ApplicationWorkflow::with_lifecycle(lifecycle);
        let startup_recovery_pending = Arc::new(AtomicBool::new(false));
        let status = Arc::new(parking_lot::Mutex::new(CoreLifecycleStatus::default()));
        let next_id = Arc::new(AtomicU64::new(1));
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationWorkflowActor,
            ApplicationWorkflowArgs {
                workflow,
                next_id: next_id.clone(),
                status: status.clone(),
                service_status,
                service: service.clone(),
                startup_owner_unproven_shared: startup_owner_unproven.clone(),
            },
        )
        .await?;
        if let Some(recovery_notify) = recovery_notify {
            let recovery_actor = actor_ref.clone();
            let recovery_shutdown = shutdown.child_token();
            tasks.spawn(async move {
                loop {
                    tokio::select! {
                        _ = recovery_shutdown.cancelled() => break,
                        _ = recovery_notify.notified() => {
                            if recovery_actor.cast(Message::RecoverCore).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        let client = Self(Arc::new(CoreLifecycleClientInner::Actor {
            actor_ref,
            next_id,
            status,
            service_status: service_status_rx.clone(),
            service,
            core,
            startup_owner_unproven,
            service_failed_generation,
            startup_recovery_pending,
        }));
        #[cfg(not(test))]
        {
            let recovery_client = client.clone();
            let mut status_updates = service_status_rx.clone();
            let recovery_shutdown = shutdown.child_token();
            tasks.spawn(async move {
                let mut previous_status = status_updates.borrow().clone();
                loop {
                    tokio::select! {
                        _ = recovery_shutdown.cancelled() => break,
                        changed = status_updates.changed() => {
                            if changed.is_err() {
                                break;
                            }
                            let status = status_updates.borrow().clone();
                            let recovery_evidence_changed = startup_recovery_evidence_changed(
                                &previous_status,
                                &status,
                                startup_owner_unproven_signal.load(Ordering::Acquire),
                            );
                            previous_status = status;
                            if recovery_evidence_changed && recovery_client.startup_recovery_needed() {
                                recovery_client.request_startup_recovery();
                            }
                        }
                    }
                }
            });
        }
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
        let workflow = CoreLifecycleWorkflow::new(
            application,
            clash,
            None,
            core.clone(),
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            runtime_paths,
            service.clone(),
        );
        let startup_state =
            StartupWorkflowState::with_owner_signal(workflow.startup_owner_unproven_handle());
        Self(Arc::new(CoreLifecycleClientInner::Direct {
            workflow: tokio::sync::Mutex::new(workflow),
            service_status: service_status_rx,
            service,
            core,
            startup_state: tokio::sync::Mutex::new(startup_state),
        }))
    }

    pub(crate) async fn connect_application_workflow(
        &self,
        workflow: ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        self.connect_application_workflow_with_registration(workflow, None)
            .await
    }

    pub(in crate::client) async fn connect_application_workflow_with_registration(
        &self,
        workflow: ApplicationWorkflow,
        registered: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| Message::ConnectApplicationWorkflow {
                            workflow,
                            reply,
                            registered,
                        },
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
        original: chimera_contracts::system_proxy::OsProxyConfig,
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
            CoreLifecycleClientInner::Direct {
                workflow,
                startup_state,
                ..
            } => {
                let workflow = workflow.lock().await;
                let mut startup_state = startup_state.lock().await;
                workflow
                    .execute(command, startup_state.retry_target_mut())
                    .await
            }
        }
    }

    #[cfg(test)]
    async fn startup_retry_schedule_for_tests(
        &self,
    ) -> anyhow::Result<Option<(String, tokio::time::Instant)>> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("startup retry schedule test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::ReadStartupRetryScheduleForTest { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(schedule) => Ok(schedule),
            CallResult::Timeout => anyhow::bail!("startup retry schedule query timed out"),
            CallResult::SenderError => {
                anyhow::bail!("startup retry schedule query reply was dropped")
            }
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_application_attempt_for_tests(
        &self,
    ) -> anyhow::Result<chimera_core_manager::OperationId> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("application attempt test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedApplicationAttemptForTest { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("application attempt seeding timed out"),
            CallResult::SenderError => anyhow::bail!("application attempt seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_source_decision_for_tests(
        &self,
        decision: chimera_core::state::DecisionHandle,
    ) -> anyhow::Result<chimera_core_manager::OperationId> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("source decision test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedSourceDecisionForTest { decision, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("source decision seeding timed out"),
            CallResult::SenderError => anyhow::bail!("source decision seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_attempt_with_deferred_retry_for_tests(
        &self,
        decision: chimera_core::state::DecisionHandle,
    ) -> anyhow::Result<(chimera_core_manager::OperationId, String)> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("deferred retry test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedAttemptWithDeferredRetryForTest { decision, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("deferred retry test seeding timed out"),
            CallResult::SenderError => anyhow::bail!("deferred retry seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_committed_target_for_tests(
        &self,
        decision: chimera_core::state::DecisionHandle,
    ) -> anyhow::Result<chimera_core_manager::OperationId> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("committed target test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedCommittedTargetForTest { decision, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("committed target seeding timed out"),
            CallResult::SenderError => anyhow::bail!("committed target seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn application_recovery_state_for_tests(
        &self,
    ) -> anyhow::Result<(
        Option<(chimera_core_manager::OperationId, bool)>,
        Option<(String, tokio::time::Instant)>,
        Option<(u8, u32)>,
    )> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("application recovery test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::ReadApplicationRecoveryStateForTest { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(state) => Ok(state),
            CallResult::Timeout => anyhow::bail!("application recovery state query timed out"),
            CallResult::SenderError => {
                anyhow::bail!("application recovery state reply was dropped")
            }
        }
    }

    #[cfg(test)]
    async fn observe_next_startup_retry_lease_failure(
        &self,
    ) -> anyhow::Result<tokio::sync::oneshot::Receiver<()>> {
        let CoreLifecycleClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("startup retry observer requires the lifecycle actor");
        };
        let (observer, observed) = tokio::sync::oneshot::channel();
        match actor_ref
            .call(
                |reply| Message::ArmStartupRetryLeaseFailureObserver { observer, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result.map(|()| observed),
            CallResult::Timeout => anyhow::bail!("startup retry observer arm timed out"),
            CallResult::SenderError => {
                anyhow::bail!("startup retry observer reply was dropped")
            }
        }
    }

    async fn startup_reconcile(&self) -> anyhow::Result<CompletedLifecycleOperation> {
        self.startup_reconcile_with_timeout(None).await
    }

    async fn cached_startup_report(&self) -> anyhow::Result<Option<StartupReport>> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| Message::ReadStartupReport { reply },
                        Some(CALL_WAIT),
                    )
                    .await?
                {
                    CallResult::Success(report) => Ok(report),
                    CallResult::Timeout => anyhow::bail!("startup report query timed out"),
                    CallResult::SenderError => {
                        anyhow::bail!("startup report query reply was dropped")
                    }
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow, .. } => {
                Ok(workflow.lock().await.startup_report())
            }
        }
    }

    async fn startup_reconcile_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref,
                next_id,
                service,
                ..
            } => {
                if let Some(report) = self.cached_startup_report().await? {
                    return Ok(completed_from_startup_report(report));
                }
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                let transition = match service.begin_transition().await {
                    Ok(transition) => transition,
                    Err(error) => {
                        let reason =
                            format!("could not acquire the Service transition lease: {error:#}");
                        let cache_id = next_id.fetch_add(1, Ordering::Relaxed);
                        let _ = actor_ref
                            .call(
                                |reply| {
                                    Message::Request(Request {
                                        command: Command::CacheStartupUnsettled {
                                            operation_id: id,
                                            reason,
                                        },
                                        response: Response {
                                            id: cache_id,
                                            reply: Some(reply),
                                        },
                                        deferred_retry: None,
                                    })
                                },
                                timeout,
                            )
                            .await?;
                        return self
                            .cached_startup_report()
                            .await?
                            .map(completed_from_startup_report)
                            .ok_or_else(|| {
                                anyhow::anyhow!("startup failure report was not cached")
                            });
                    }
                };
                if let Some(report) = self.cached_startup_report().await? {
                    return Ok(completed_from_startup_report(report));
                }
                let result = match actor_ref
                    .call(
                        |reply| {
                            Message::Request(Request {
                                command: Command::StartupReconcile {
                                    operation_id: id,
                                    transition,
                                },
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
                    Ok(CallResult::Success(result)) => result,
                    Ok(CallResult::Timeout) => anyhow::bail!(
                        "startup core lifecycle operation {id} timed out; it may still be queued or running"
                    ),
                    Ok(CallResult::SenderError) => anyhow::bail!(
                        "startup core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                    ),
                    Err(error) => Err(error.into()),
                };
                let report = match self.cached_startup_report().await? {
                    Some(report) => report,
                    None => {
                        let reason = result
                            .as_ref()
                            .err()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| {
                                "startup lifecycle completed without a report".into()
                            });
                        let cache_id = next_id.fetch_add(1, Ordering::Relaxed);
                        let _ = actor_ref
                            .call(
                                |reply| {
                                    Message::Request(Request {
                                        command: Command::CacheStartupUnsettled {
                                            operation_id: id,
                                            reason,
                                        },
                                        response: Response {
                                            id: cache_id,
                                            reply: Some(reply),
                                        },
                                        deferred_retry: None,
                                    })
                                },
                                Some(CALL_WAIT),
                            )
                            .await?;
                        self.cached_startup_report().await?.ok_or_else(|| {
                            anyhow::anyhow!("startup failure report was not cached")
                        })?
                    }
                };
                Ok(CompletedLifecycleOperation { result, report })
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct {
                workflow,
                service,
                startup_state,
                ..
            } => {
                if let Some(report) = workflow.lock().await.startup_report() {
                    return Ok(completed_from_startup_report(report));
                }
                let transition = match service.begin_transition().await {
                    Ok(transition) => transition,
                    Err(error) => {
                        let reason =
                            format!("could not acquire the Service transition lease: {error:#}");
                        let workflow = workflow.lock().await;
                        startup_state.lock().await.mark_owner_unproven();
                        let report = StartupReport {
                            operation_id: 1,
                            observation: None,
                            outcome: StartupOutcome::Unsettled { reason },
                        };
                        workflow.cache_startup_report_for_tests(&report);
                        return Ok(completed_from_startup_report(report));
                    }
                };
                if let Some(report) = workflow.lock().await.startup_report() {
                    return Ok(completed_from_startup_report(report));
                }
                let workflow = workflow.lock().await;
                let mut startup_state = startup_state.lock().await;
                let result = workflow
                    .execute(
                        Command::StartupReconcile {
                            operation_id: 1,
                            transition,
                        },
                        startup_state.retry_target_mut(),
                    )
                    .await;
                let report = match workflow.startup_report() {
                    Some(report) => report,
                    None => {
                        let reason = result
                            .as_ref()
                            .err()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| {
                                "direct startup lifecycle completed without a report".into()
                            });
                        startup_state.mark_owner_unproven();
                        let report = StartupReport {
                            operation_id: 1,
                            observation: None,
                            outcome: StartupOutcome::Unsettled { reason },
                        };
                        workflow.cache_startup_report_for_tests(&report);
                        report
                    }
                };
                Ok(CompletedLifecycleOperation { result, report })
            }
        }
    }

    fn startup_recovery_needed(&self) -> bool {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                startup_owner_unproven,
                service_failed_generation,
                service_status,
                core,
                ..
            } => {
                !core.outcome_uncertain()
                    && (startup_owner_unproven.load(Ordering::Acquire) || {
                        let failed_generation = service_failed_generation.load(Ordering::Acquire);
                        let status = service_status.borrow();
                        failed_generation != 0
                            && status.ready_generation != failed_generation
                            && status.phase.readiness() == Some(true)
                    })
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => false,
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
        if self.startup_recovery_needed() {
            self.request_startup_recovery();
            return;
        }
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

    fn request_startup_recovery(&self) {
        let (actor_ref, next_id, service, startup_recovery_pending) = match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref,
                next_id,
                service,
                startup_recovery_pending,
                ..
            } => (actor_ref, next_id, service, startup_recovery_pending),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => return,
        };
        if startup_recovery_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let actor_ref = actor_ref.clone();
        let next_id = next_id.clone();
        let service = service.clone();
        let startup_recovery_pending = startup_recovery_pending.clone();
        let client = self.clone();
        tokio::spawn(async move {
            let result = async {
                // Acquire the host lease before asking the serialized actor to
                // run recovery, preserving the global host-lock/mailbox order.
                let transition = service.begin_transition().await?;
                if !client.startup_recovery_needed() {
                    return Ok(());
                }
                let operation_id = next_id.fetch_add(1, Ordering::Relaxed);
                match actor_ref
                    .call(
                        |reply| {
                            Message::Request(Request {
                                command: Command::RecoverStartup {
                                    operation_id,
                                    transition,
                                },
                                response: Response {
                                    id: operation_id,
                                    reply: Some(reply),
                                },
                                deferred_retry: None,
                            })
                        },
                        Some(CALL_WAIT),
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => anyhow::bail!("startup owner recovery timed out"),
                    CallResult::SenderError => {
                        anyhow::bail!("startup owner recovery reply was dropped")
                    }
                }
            }
            .await;
            startup_recovery_pending.store(false, Ordering::Release);
            if let Err(error) = result {
                tracing::warn!(%error, "automatic startup owner recovery did not complete");
            }
        });
    }

    pub(super) async fn request_service_endpoint_down(&self) {
        self.request_service_endpoint_down_if(|| true).await;
    }

    pub(super) async fn request_service_endpoint_down_if<F>(&self, still_current: F)
    where
        F: Fn() -> bool + Send + 'static,
    {
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
        if !still_current() {
            // The Service host lock is now ours. A retired health observation
            // must drop its lease without submitting recovery after stop/restart.
            return;
        }
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
                .map(classify_service_probe),
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
        if self.startup_recovery_needed() {
            return self.recover_startup().await;
        }
        self.execute(Command::Reconcile).await
    }

    async fn recover_startup(&self) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                next_id, service, ..
            } => {
                // Keep host-lock-before-mailbox ordering, matching every
                // Service transition and the initial StartupReconcile path.
                let transition = service.begin_transition().await?;
                let operation_id = next_id.fetch_add(1, Ordering::Relaxed);
                self.execute(Command::RecoverStartup {
                    operation_id,
                    transition,
                })
                .await
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct {
                workflow,
                service,
                startup_state,
                ..
            } => {
                let transition = service.begin_transition().await?;
                let workflow = workflow.lock().await;
                let mut startup_state = startup_state.lock().await;
                let operation_id = workflow
                    .startup_report()
                    .map_or(1, |report| report.operation_id.saturating_add(1));
                workflow
                    .execute(
                        Command::RecoverStartup {
                            operation_id,
                            transition,
                        },
                        startup_state.retry_target_mut(),
                    )
                    .await
            }
        }
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
        let completed = self
            .inner
            .core_lifecycle
            .startup_reconcile()
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "startup reconciliation did not return a terminal reply; Profile background producers remain held: {error:#}"
                )
            })?;
        let startup = match &completed.report.outcome {
            StartupOutcome::Ready => completed.result,
            outcome => {
                let reason = outcome
                    .failure_reason()
                    .unwrap_or("startup did not reach a ready state");
                tracing::warn!(
                    operation_id = completed.report.operation_id,
                    ?outcome,
                    "startup reconcile did not reach the ready state"
                );
                Err(anyhow::anyhow!(
                    "startup reconcile {} did not reach ready: {reason}",
                    completed.report.operation_id
                ))
            }
        };
        let may_start_producers = !self.inner.core_lifecycle.status().uncertain;
        let producers = match (may_start_producers, self.inner.typed_profiles.as_ref()) {
            (true, Some(profiles)) => profiles.start_producers().await.map_err(anyhow::Error::new),
            (false, Some(_)) => Ok(()),
            (_, None) => Ok(()),
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

    pub(crate) async fn request_service_endpoint_down_if<F>(&self, still_current: F)
    where
        F: Fn() -> bool + Send + 'static,
    {
        self.inner
            .core_lifecycle
            .request_service_endpoint_down_if(still_current)
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
        self.inner.core_lifecycle.install_service().await
    }

    pub(crate) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.uninstall_service().await
    }

    pub(crate) async fn update_service(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.update_service().await
    }

    pub(crate) async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.inner.core_lifecycle.start_service(ready_timeout).await
    }

    pub(crate) async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.inner
            .core_lifecycle
            .restart_service(ready_timeout)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering as AtomicOrdering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use camino::Utf8PathBuf;
    use chimera_config::application::ChimeraAppConfigPatch;
    use chimera_config::clash::config::ClashConfig;
    use chimera_core::state::{
        PersistentStateManagerSetup, ReplaceIfVersionResult, StateAckSubscriber, StateChange,
        StateDecision, SubscriberName,
    };
    use chimera_ipc::api::status::CoreState;
    use tokio::sync::{Notify, oneshot};
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    use super::*;
    use crate::{config::chimera::ClashCore, core::RunType};

    #[test]
    fn service_status_tracks_ready_generation_without_changing_ipc_fields() {
        let mut current = ServiceHostStatus::probing();
        assert!(!current.settled_ready);
        assert_eq!(current.ready_generation, 0);

        let mut ready = current.clone();
        ready.phase = ServicePhase::Ready;
        current = ready.settle_after(&current);
        assert!(current.settled_ready);
        assert_eq!(current.ready_generation, 1);

        let mut unknown = current.clone();
        unknown.phase = ServicePhase::Unknown;
        current = unknown.settle_after(&current);
        assert!(
            current.settled_ready,
            "failed probe cannot invent a stopped daemon"
        );
        assert_eq!(current.ready_generation, 1);

        let mut stopped = current.clone();
        stopped.phase = ServicePhase::DaemonStopped;
        current = stopped.settle_after(&current);
        assert!(!current.settled_ready);
        assert_eq!(current.ready_generation, 1);

        let mut restarted = current.clone();
        restarted.phase = ServicePhase::Ready;
        current = restarted.settle_after(&current);
        assert!(current.settled_ready);
        assert_eq!(current.ready_generation, 2);

        let json = serde_json::to_value(&current).unwrap();
        assert!(!json.as_object().unwrap().contains_key("settled_ready"));
        assert!(!json.as_object().unwrap().contains_key("ready_generation"));
        assert_eq!(json["phase"], "ready");
    }

    #[test]
    fn startup_recovery_evidence_detects_same_generation_probe_recovery() {
        let mut previous = ServiceHostStatus::probing();
        previous = previous.observe_probe(
            chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("1.0.0"),
                status: chimera_ipc::types::ServiceStatus::Running,
                server: Some(chimera_ipc::api::status::StatusResBody {
                    version: std::borrow::Cow::Borrowed("1.11.1"),
                    core_infos: chimera_ipc::api::status::CoreInfos {
                        instance_id: Some("service-instance".into()),
                        r#type: None,
                        state: CoreState::Running,
                        state_changed_at: 1,
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: None,
                        detail: Some(chimera_ipc::api::status::CoreStateDetail::Stopped {
                            reason: None,
                        }),
                    },
                    runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                        service_data_dir: std::borrow::Cow::Owned(Default::default()),
                        service_config_dir: std::borrow::Cow::Owned(Default::default()),
                        nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                        nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                    },
                }),
            },
            true,
            None,
        );
        let unavailable = previous.on_probe_failure();
        let recovered = unavailable.observe_probe(
            chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("1.0.0"),
                status: chimera_ipc::types::ServiceStatus::Running,
                server: previous.server.clone(),
            },
            true,
            None,
        );

        assert_eq!(unavailable.ready_generation, recovered.ready_generation);
        assert!(startup_recovery_evidence_changed(
            &unavailable,
            &recovered,
            true
        ));
        assert!(!startup_recovery_evidence_changed(
            &unavailable,
            &recovered,
            false
        ));
    }

    #[test]
    fn startup_recovery_evidence_detects_ready_generation_change_alone() {
        let mut previous = ServiceHostStatus::probing();
        previous.phase = ServicePhase::Ready;
        previous.settled_ready = true;
        previous.ready_generation = 1;

        let mut current = previous.clone();
        current.ready_generation = 2;

        assert_eq!(
            crate::client::application_workflow::startup::ServiceEvidence::from_status(&previous),
            crate::client::application_workflow::startup::ServiceEvidence::from_status(&current),
            "a generation advance alone must be enough to request recovery"
        );
        assert!(startup_recovery_evidence_changed(
            &previous, &current, false
        ));
    }

    #[tokio::test]
    async fn application_workflow_connection_keeps_owner_and_runs_startup() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(ConfirmedStoppedCore {
                events: events.clone(),
                run_type: RunType::Normal,
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;
        assert!(client.cached_startup_report().await.unwrap().is_none());

        let _connected = crate::client::application_workflow::ApplicationWorkflowClient::spawn(
            client.clone(),
            Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            Arc::new(tokio::sync::Mutex::new(None)),
            None,
        )
        .await
        .expect("the production ApplicationWorkflow connection should succeed");

        assert!(client.cached_startup_report().await.unwrap().is_none());

        let completed = client
            .startup_reconcile()
            .await
            .expect("startup should execute through the connected application workflow");
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        assert!(
            events.lock().unwrap().contains(&"rebuild"),
            "startup must reach the core adapter through the connected workflow"
        );
        let cached = client
            .cached_startup_report()
            .await
            .expect("the production actor should answer the report query")
            .expect("the connected ApplicationWorkflow should own the startup report");
        assert_eq!(cached.operation_id, completed.report.operation_id);
        assert_eq!(cached.outcome, completed.report.outcome);
    }

    #[tokio::test]
    async fn application_workflow_connection_waits_for_active_operation_and_rejects_duplicates() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let core = Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        });
        let client = spawn_startup_client(
            core.clone(),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let stop_client = client.clone();
        let stop = tokio::spawn(async move { stop_client.stop_core().await });
        core.stop_started.notified().await;

        let connect_client = client.clone();
        let (registered, pending) = oneshot::channel();
        let connect = tokio::spawn(async move {
            crate::client::application_workflow::ApplicationWorkflowClient::spawn_with_registration(
                connect_client,
                Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
                Arc::new(tokio::sync::Mutex::new(None)),
                None,
                registered,
            )
            .await
        });
        pending
            .await
            .expect("the actor should acknowledge that it registered the first connection");

        let duplicate = crate::client::application_workflow::ApplicationWorkflowClient::spawn(
            client.clone(),
            Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            Arc::new(tokio::sync::Mutex::new(None)),
            None,
        )
        .await;
        assert!(
            duplicate.is_err(),
            "a second mutation workflow must be rejected while the first connection is pending"
        );

        core.release_stop.notify_one();
        stop.await
            .expect("StopCore task should complete")
            .expect("StopCore should succeed");
        connect
            .await
            .expect("connection task should complete")
            .expect("the pending mutation workflow should attach after completion");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end"]
        );
    }

    #[tokio::test]
    async fn cancelled_pending_application_workflow_does_not_poison_connection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let core = Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        });
        let client = spawn_startup_client(
            core.clone(),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let stop_client = client.clone();
        let stop = tokio::spawn(async move { stop_client.stop_core().await });
        core.stop_started.notified().await;

        let connect_client = client.clone();
        let (registered, pending) = oneshot::channel();
        let connect = tokio::spawn(async move {
            crate::client::application_workflow::ApplicationWorkflowClient::spawn_with_registration(
                connect_client,
                Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
                Arc::new(tokio::sync::Mutex::new(None)),
                None,
                registered,
            )
            .await
        });
        pending
            .await
            .expect("the actor should confirm that it stored the pending connection");
        connect.abort();
        assert!(
            connect.await.is_err(),
            "cancel the waiting connection caller"
        );

        core.release_stop.notify_one();
        stop.await
            .expect("StopCore task should complete")
            .expect("StopCore should succeed");

        crate::client::application_workflow::ApplicationWorkflowClient::spawn(
            client,
            Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            Arc::new(tokio::sync::Mutex::new(None)),
            None,
        )
        .await
        .expect("a cancelled pending connection must leave the actor connectable");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end"]
        );
    }

    #[tokio::test]
    async fn explicit_reconcile_keeps_committed_profiles_active_after_deferred_retry_recovery() {
        let directory = tempfile::tempdir().expect("temporary Profiles directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
            .expect("UTF-8 temporary Profiles path");
        let mut manager = PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .expect("Profiles state manager");
        let before = manager.snapshot_handle().load();
        let version = before.version;
        let mut candidate = before.state.clone();
        candidate.valid.push("production-actor-tcc-test".into());
        let events = Arc::new(Mutex::new(Vec::new()));
        let core = Arc::new(ProfileTccCore {
            validations: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            fail_next_reconcile: AtomicBool::new(false),
            active_profiles: Mutex::new(None),
        });
        let client = spawn_startup_client_with_profiles(
            core.clone(),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
            manager.snapshot_handle(),
        )
        .await;
        let mutations = crate::state::mutation::MutationCoordinator::pending();
        mutations
            .connect(client.clone())
            .await
            .expect("production ApplicationWorkflow should attach to Core Lifecycle");
        let operation_id = chimera_core_manager::OperationId::generate();
        let participant = mutations
            .participant(
                operation_id.clone(),
                crate::client::application_workflow::impact::MutationHints::default(),
                crate::client::application_workflow::policy::CommandClass::Save,
            )
            .expect("connected mutation coordinator should create a participant");
        let decision_capture = Arc::new(Mutex::new(None));

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let decision_capture = decision_capture.clone();
                    move |decision| {
                        *decision_capture.lock().unwrap() = Some(decision.clone());
                        participant(decision)
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("Profile source transaction");
        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        let committed = manager.snapshot_handle().load();
        assert_eq!(committed.state.valid, candidate.valid);
        assert_eq!(core.validations.load(AtomicOrdering::SeqCst), 1);

        let (receipt, degradations) = mutations
            .finish(operation_id, "profiles", *committed.version.as_ref())
            .await;
        assert_eq!(
            receipt.runtime,
            crate::client::runtime::RuntimeCommitStatus::SavedInactive
        );
        assert!(degradations.is_empty());

        let decision = decision_capture
            .lock()
            .unwrap()
            .clone()
            .expect("source transaction must expose its decision to the participant");
        let (unresolved_operation, digest) = client
            .seed_unresolved_attempt_with_deferred_retry_for_tests(decision.clone())
            .await
            .expect("seed the actor's unresolved source decision and due retry");
        let (attempt, deferred_retry, _) = client
            .application_recovery_state_for_tests()
            .await
            .expect("read the actor's recovery state");
        assert_eq!(attempt, Some((unresolved_operation, true)));
        let (deferred_digest, next_attempt) =
            deferred_retry.expect("rejected retry must be rescheduled");
        assert_eq!(deferred_digest, digest);
        assert!(
            next_attempt > tokio::time::Instant::now(),
            "a rejected retry must retain a future deadline without spending an attempt"
        );
        client
            .reconcile()
            .await
            .expect("the normal reconcile API must recover a Profile source attempt");
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .expect("read the recovered actor state")
                .0,
            None,
            "the explicit Profile recovery must release the live attempt"
        );
        assert_eq!(
            core.active_profiles
                .lock()
                .unwrap()
                .as_ref()
                .map(|profiles| profiles.valid.clone()),
            Some(candidate.valid.clone()),
            "recovery must leave the reconciled committed Profiles active"
        );
        assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 0);

        let committed_target = client
            .seed_unresolved_committed_target_for_tests(decision)
            .await
            .expect("seed a committed deferred Profile retry with an uncertain result");
        let (attempt, _, retry_budget) = client
            .application_recovery_state_for_tests()
            .await
            .expect("read the committed target recovery state");
        assert_eq!(attempt, Some((committed_target, true)));
        assert_eq!(retry_budget, Some((2, 1)));
        core.fail_next_reconcile
            .store(true, AtomicOrdering::Release);
        assert!(
            client.reconcile().await.is_err(),
            "a failed current-config reconcile must remain a failed recovery"
        );
        let (attempt, _, retry_budget) = client
            .application_recovery_state_for_tests()
            .await
            .expect("read the still-isolated committed target state");
        assert_eq!(attempt, Some((committed_target.clone(), true)));
        assert_eq!(retry_budget, Some((2, 1)));
        assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 0);
        client
            .reconcile()
            .await
            .expect("reconcile should recover the current committed Profile target");
        assert_eq!(
            core.active_profiles
                .lock()
                .unwrap()
                .as_ref()
                .map(|profiles| profiles.valid.clone()),
            Some(candidate.valid),
            "committed-target recovery must not restore its old runtime baseline"
        );
        assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .expect("read the settled committed target state"),
            (None, None, None),
            "successful reconciliation must settle the committed retry target"
        );
        client.shutdown().await.unwrap();
    }

    #[test]
    fn exhausted_service_probe_stays_latched_not_ready() {
        let mut previous = ServiceHostStatus::probing();
        previous.phase = ServicePhase::Ready;
        previous = previous.settle_after(&ServiceHostStatus::probing());

        let mut exhausted = previous.clone();
        exhausted.phase = ServicePhase::Exhausted;
        exhausted = exhausted.settle_after(&previous);
        assert!(!exhausted.settled_ready);
        assert_eq!(exhausted.ready_generation, 1);

        let mut failed_probe = ServiceHostStatus::probe_failed(&exhausted);
        failed_probe.phase = ServicePhase::Exhausted;
        failed_probe = failed_probe.settle_after(&exhausted);
        assert!(!failed_probe.settled_ready);
        assert_eq!(failed_probe.ready_generation, 1);
    }

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<Notify>,
        release_stop: Arc<Notify>,
        recovery_notify: Arc<Notify>,
        run_type: RunType,
    }

    struct ConfirmedStoppedCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        run_type: RunType,
    }

    struct ProfileTccCore {
        validations: AtomicUsize,
        stops: AtomicUsize,
        fail_next_reconcile: AtomicBool,
        active_profiles: Mutex<Option<chimera_config::profile::Profiles>>,
    }

    struct AckOnlyProfileChange;

    #[async_trait]
    impl StateAckSubscriber<chimera_config::profile::Profiles> for AckOnlyProfileChange {
        fn name(&self) -> SubscriberName<'_> {
            "pending-profile-source-decision-test".into()
        }

        async fn on_prepare(
            &self,
            _change: StateChange<chimera_config::profile::Profiles>,
        ) -> chimera_core::state::Ack {
            chimera_core::state::Ack::Ok
        }
    }

    struct CoreUpdateRecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        state: CoreState,
        state_changed_at: i64,
        run_type: RunType,
        active_kind: Option<chimera_core_manager::CoreKind>,
        receipt_error: bool,
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

    struct TransitionUnavailableService {
        inner: RecordingService,
    }

    struct UnreadableService {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct StartupServiceProbe {
        events: Arc<Mutex<Vec<&'static str>>>,
        probes: AtomicU8,
        confirms_stopped: bool,
        runtime_owned: bool,
    }

    struct StartupFallbackServiceProbe {
        inner: StartupServiceProbe,
    }

    struct StartupTransitionRaceService {
        inner: StartupServiceProbe,
        transition_lock: Arc<tokio::sync::Mutex<()>>,
        transition_attempts: Arc<AtomicUsize>,
        probe_entered: Arc<Notify>,
        release_probe: Arc<Notify>,
    }

    struct RetiringCore {
        inner: RecordingCore,
        selected: Mutex<RunType>,
    }

    struct StartupOwnerCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        selected: Mutex<RunType>,
        local_status: Mutex<(CoreState, i64)>,
        service_running: Arc<AtomicBool>,
        local_receipt: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
        service_receipt: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
        receipt_queries: AtomicU8,
        invalidate_receipt_after_first_query: bool,
        stop_confirms: bool,
        stop_error: Option<(chimera_core_manager::CoreErrorKind, bool)>,
        service_retire_error: Option<(chimera_core_manager::CoreErrorKind, bool)>,
        service_recover_error: Option<(chimera_core_manager::CoreErrorKind, bool)>,
        outcome_uncertain: AtomicBool,
    }

    struct MutableStartupServiceProbe {
        events: Arc<Mutex<Vec<&'static str>>>,
        running: Arc<AtomicBool>,
        stopped_reason: Option<String>,
        runtime_owned: bool,
    }

    struct ForeignAfterRecoveryServiceProbe {
        inner: MutableStartupServiceProbe,
    }

    struct RestartBudgetService {
        events: Arc<Mutex<Vec<&'static str>>>,
        attempts: AtomicU8,
        exhausted: AtomicBool,
    }

    struct StartupRetryLeaseService {
        inner: RestartBudgetService,
        transition_attempts: AtomicU8,
        retry_lease_gate: Option<(Arc<Notify>, Arc<Notify>)>,
    }

    struct RecordingServiceTransition {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct StartupTransitionRaceLease {
        _guard: tokio::sync::OwnedMutexGuard<()>,
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

    struct StartupDegradedServiceCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        reconciles: AtomicU8,
        stopped: AtomicBool,
        selected: Mutex<RunType>,
    }

    struct StartupTransientCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        reconciles: AtomicU8,
        stops: AtomicU8,
        recovered: Arc<Notify>,
        outcome_uncertain: bool,
        stop_gate: Option<(Arc<Notify>, Arc<Notify>)>,
    }

    struct StartupReceiptCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        status: Mutex<(CoreState, i64)>,
        receipt: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
        stop_confirms: bool,
    }

    fn startup_owner_core(
        events: Arc<Mutex<Vec<&'static str>>>,
        local_state: (CoreState, i64),
        service_running: Arc<AtomicBool>,
    ) -> StartupOwnerCore {
        StartupOwnerCore {
            events,
            selected: Mutex::new(RunType::Normal),
            local_status: Mutex::new(local_state),
            service_running,
            local_receipt: Mutex::new(None),
            service_receipt: Mutex::new(None),
            receipt_queries: AtomicU8::new(0),
            invalidate_receipt_after_first_query: false,
            stop_confirms: true,
            stop_error: None,
            service_retire_error: None,
            service_recover_error: None,
            outcome_uncertain: AtomicBool::new(false),
        }
    }

    async fn spawn_startup_client(
        core: Arc<dyn CoreLifecyclePort>,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> CoreLifecycleClient {
        spawn_startup_client_with_application(core, service, ApplicationClient::legacy().unwrap())
            .await
    }

    async fn spawn_startup_client_with_application(
        core: Arc<dyn CoreLifecyclePort>,
        service: Arc<dyn ServiceLifecyclePort>,
        application: ApplicationClient,
    ) -> CoreLifecycleClient {
        spawn_startup_client_with_application_and_clash(
            core,
            service,
            application,
            ClashConfigClient::legacy().unwrap(),
        )
        .await
    }

    async fn spawn_startup_client_with_application_and_clash(
        core: Arc<dyn CoreLifecyclePort>,
        service: Arc<dyn ServiceLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
    ) -> CoreLifecycleClient {
        let installer: Arc<dyn ports::BinaryInstaller> =
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            )));
        CoreLifecycleClient::spawn_with_service(
            core,
            application,
            clash,
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            installer,
            service,
        )
        .await
        .unwrap()
    }

    async fn spawn_startup_client_with_profiles(
        core: Arc<dyn CoreLifecyclePort>,
        service: Arc<dyn ServiceLifecyclePort>,
        profiles: chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>,
    ) -> CoreLifecycleClient {
        let installer: Arc<dyn ports::BinaryInstaller> =
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            )));
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        CoreLifecycleClient::spawn_with_profiles(
            core,
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            Some(profiles),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            installer,
            service,
            shutdown,
            &tasks,
        )
        .await
        .unwrap()
    }

    async fn startup_target_for(
        application: ApplicationClient,
        clash: ClashConfigClient,
        core: Arc<dyn CoreLifecyclePort>,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> String {
        let workflow = CoreLifecycleWorkflow::new(
            application,
            clash,
            None,
            core,
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            service,
        );
        workflow
            .startup_target_identity()
            .await
            .expect("startup target should be available")
    }

    fn startup_test_receipt(
        target: Option<String>,
    ) -> Arc<crate::client::runtime::RuntimeApplyReceipt> {
        startup_test_receipt_for(
            target,
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local,
        )
    }

    fn startup_test_receipt_for(
        target: Option<String>,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> Arc<crate::client::runtime::RuntimeApplyReceipt> {
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.target = target;
        receipt.host = host;
        Arc::new(receipt)
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
        state: Mutex<chimera_contracts::system_proxy::OsProxyConfig>,
        writes: Mutex<Vec<chimera_contracts::system_proxy::OsProxyConfig>>,
        pause_disable: bool,
        started: Mutex<Option<oneshot::Sender<()>>>,
        released: Mutex<bool>,
        release: Condvar,
    }

    impl LifecycleTestProxy {
        fn new(state: chimera_contracts::system_proxy::OsProxyConfig) -> Arc<Self> {
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
            state: chimera_contracts::system_proxy::OsProxyConfig,
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

        fn current(&self) -> chimera_contracts::system_proxy::OsProxyConfig {
            self.state.lock().unwrap().clone()
        }

        fn writes(&self) -> Vec<chimera_contracts::system_proxy::OsProxyConfig> {
            self.writes.lock().unwrap().clone()
        }
    }

    impl chimera_contracts::system_proxy::OsProxyPort for LifecycleTestProxy {
        fn get(
            &self,
        ) -> Result<
            chimera_contracts::system_proxy::OsProxyConfig,
            chimera_contracts::system_proxy::OsProxyError,
        > {
            Ok(self.current())
        }

        fn set(
            &self,
            config: &chimera_contracts::system_proxy::OsProxyConfig,
        ) -> Result<(), chimera_contracts::system_proxy::OsProxyError> {
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

    impl chimera_contracts::system_proxy::AutoLaunchPort for LifecycleTestAutoLaunch {
        fn is_enabled(&self) -> Result<bool, chimera_contracts::system_proxy::AutoLaunchError> {
            Ok(false)
        }

        fn set_enabled(
            &self,
            _enabled: bool,
        ) -> Result<(), chimera_contracts::system_proxy::AutoLaunchError> {
            Ok(())
        }
    }

    struct LifecycleTestPac;

    #[async_trait]
    impl chimera_contracts::system_proxy::PacPort for LifecycleTestPac {
        fn is_supported(&self) -> bool {
            false
        }

        async fn apply(
            &self,
            _url: &url::Url,
            _cancel: CancellationToken,
        ) -> Result<(), chimera_contracts::system_proxy::PacError> {
            Err(chimera_contracts::system_proxy::PacError::DownloadCancelled)
        }

        fn disable(&self) -> Result<(), chimera_contracts::system_proxy::PacError> {
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
        os: Arc<dyn chimera_contracts::system_proxy::OsProxyPort>,
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

    fn stale_proxy_config() -> chimera_contracts::system_proxy::OsProxyConfig {
        chimera_contracts::system_proxy::OsProxyConfig {
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
            chimera_config::application::ReleaseChannel::Stable,
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
    impl ServiceLifecyclePort for TransitionUnavailableService {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.inner.probe().await
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.inner
                .events
                .lock()
                .unwrap()
                .push("service-begin-failed");
            anyhow::bail!("test Service transition lease is unavailable")
        }
    }

    #[async_trait]
    impl ServiceLifecyclePort for UnreadableService {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.events.lock().unwrap().push("service-probe-failed");
            anyhow::bail!("test Service probe is unavailable")
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.events.lock().unwrap().push("service-begin");
            Ok(Box::new(RecordingServiceTransition {
                events: self.events.clone(),
            }))
        }
    }

    #[async_trait]
    impl ServiceLifecyclePort for StartupServiceProbe {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.events.lock().unwrap().push("service-probe");
            let probe = self.probes.fetch_add(1, AtomicOrdering::AcqRel);
            let core_detail = if probe == 0 || !self.confirms_stopped {
                chimera_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }
            } else {
                chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }
            };
            Ok(chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("1.11.1"),
                status: chimera_ipc::types::ServiceStatus::Running,
                server: Some(chimera_ipc::api::status::StatusResBody {
                    version: std::borrow::Cow::Borrowed("1.11.1"),
                    core_infos: chimera_ipc::api::status::CoreInfos {
                        instance_id: Some("9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
                        r#type: None,
                        state: CoreState::Running,
                        state_changed_at: 1,
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: Some(chimera_ipc::api::status::ConfigRevisionInfo {
                            epoch: 1,
                            generation: 3,
                            source_hash: "89abcdef01234567".into(),
                            effective_hash: "0123456789abcdef".into(),
                        }),
                        detail: Some(core_detail),
                    },
                    runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                        service_data_dir: std::borrow::Cow::Owned(Default::default()),
                        service_config_dir: std::borrow::Cow::Owned(Default::default()),
                        nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                        nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                    },
                }),
            })
        }

        fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
            self.runtime_owned
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.events.lock().unwrap().push("service-begin");
            Ok(Box::new(RecordingServiceTransition {
                events: self.events.clone(),
            }))
        }
    }

    #[async_trait]
    impl ServiceLifecyclePort for StartupFallbackServiceProbe {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            let mut info = self.inner.probe().await?;
            if self.inner.probes.load(AtomicOrdering::Acquire) >= 2
                && let Some(server) = info.server.as_mut()
            {
                server.core_infos.detail =
                    Some(chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None });
            }
            Ok(info)
        }

        fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
            self.inner.runtime_owned
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.inner.begin_transition().await
        }
    }

    #[async_trait]
    impl ServiceLifecyclePort for StartupTransitionRaceService {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            if self.inner.probes.load(AtomicOrdering::Acquire) == 0 {
                self.probe_entered.notify_one();
                self.release_probe.notified().await;
            }
            self.inner.probe().await
        }

        fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
            true
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.transition_attempts
                .fetch_add(1, AtomicOrdering::AcqRel);
            let guard = self.transition_lock.clone().lock_owned().await;
            self.inner.events.lock().unwrap().push("service-begin");
            Ok(Box::new(StartupTransitionRaceLease {
                _guard: guard,
                events: self.inner.events.clone(),
            }))
        }
    }

    #[async_trait]
    impl ServiceTransitionLease for StartupTransitionRaceLease {
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
    impl CoreLifecyclePort for RetiringCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.inner.events.lock().unwrap().push("rebuild");
            Ok(())
        }

        async fn retire_service_core(
            &self,
            _expected_owner: chimera_core_manager::CoreOwner,
        ) -> anyhow::Result<()> {
            self.inner
                .events
                .lock()
                .unwrap()
                .push("service-core-retire");
            Ok(())
        }

        async fn reconcile_after_service_release(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
        ) -> anyhow::Result<()> {
            self.inner
                .events
                .lock()
                .unwrap()
                .push("rebuild-after-service-release");
            Ok(())
        }

        async fn reconcile_profiles_after_service_release(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _profiles: Arc<chimera_config::profile::Profiles>,
            _app: chimera_config::application::ChimeraAppConfig,
            _staged_content: std::collections::BTreeMap<String, String>,
            _target: Option<String>,
        ) -> anyhow::Result<()> {
            self.inner
                .events
                .lock()
                .unwrap()
                .push("rebuild-after-service-release");
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.inner.stop().await
        }

        async fn change_core(&self, core: ClashCore) -> anyhow::Result<()> {
            self.inner.change_core(core).await
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            let mut status = self.inner.status().await?;
            let selected = *self.selected.lock().unwrap();
            status.run_type = selected;
            if selected == RunType::Normal && self.inner.run_type == RunType::Service {
                status.instance_id = None;
                status.state_changed_at = 0;
            }
            Ok(status)
        }

        async fn select_execution_host(
            &self,
            host: crate::core::actor_v2::control_endpoint::ExecutionHost,
        ) -> anyhow::Result<()> {
            *self.selected.lock().unwrap() = match host {
                crate::core::actor_v2::control_endpoint::ExecutionHost::Local => RunType::Normal,
                crate::core::actor_v2::control_endpoint::ExecutionHost::Service => RunType::Service,
            };
            self.inner.events.lock().unwrap().push(match host {
                crate::core::actor_v2::control_endpoint::ExecutionHost::Local => "select-local",
                crate::core::actor_v2::control_endpoint::ExecutionHost::Service => "select-service",
            });
            Ok(())
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            Some(self.inner.recovery_notify.clone())
        }

        async fn on_profile_change(&self, break_when: bool) {
            self.inner.on_profile_change(break_when).await;
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for StartupOwnerCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            run_type: RunType,
        ) -> anyhow::Result<()> {
            let (host, label) = match run_type {
                RunType::Normal => {
                    *self.local_status.lock().unwrap() = (CoreState::Running, 3);
                    ("local", "rebuild-local")
                }
                RunType::Service => {
                    self.service_running.store(true, AtomicOrdering::Release);
                    ("service", "rebuild-service")
                }
                RunType::Elevated => anyhow::bail!("elevated Core is unsupported in this fixture"),
            };
            *self.selected.lock().unwrap() = run_type;
            self.events.lock().unwrap().push(label);
            tracing::debug!(host, "test startup Core reconciled");
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            let selected = *self.selected.lock().unwrap();
            let (state, event) = match selected {
                RunType::Normal => {
                    if self.stop_confirms {
                        *self.local_status.lock().unwrap() = (CoreState::Stopped(None), 2);
                    }
                    (self.local_status.lock().unwrap().0.clone(), "stop-local")
                }
                RunType::Service => {
                    if self.stop_confirms {
                        self.service_running.store(false, AtomicOrdering::Release);
                    }
                    (
                        if self.service_running.load(AtomicOrdering::Acquire) {
                            CoreState::Running
                        } else {
                            CoreState::Stopped(None)
                        },
                        "stop-service",
                    )
                }
                RunType::Elevated => anyhow::bail!("elevated Core is unsupported in this fixture"),
            };
            self.events.lock().unwrap().push(event);
            if let Some((kind, retryable)) = self.stop_error {
                if kind == chimera_core_manager::CoreErrorKind::StopUnconfirmed
                    || kind == chimera_core_manager::CoreErrorKind::Quarantined
                {
                    self.outcome_uncertain.store(true, AtomicOrdering::Release);
                }
                return Err(chimera_core_manager::CoreError::new(
                    kind,
                    format!("injected {kind:?} stop error while {state:?}"),
                    retryable,
                )
                .into());
            }
            Ok(())
        }

        async fn recover_service_quarantine(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-core-recover");
            if let Some((kind, retryable)) = self.service_recover_error {
                if matches!(
                    kind,
                    chimera_core_manager::CoreErrorKind::StopUnconfirmed
                        | chimera_core_manager::CoreErrorKind::Quarantined
                ) {
                    self.outcome_uncertain.store(true, AtomicOrdering::Release);
                }
                return Err(chimera_core_manager::CoreError::new(
                    kind,
                    format!("injected {kind:?} Service recovery error"),
                    retryable,
                )
                .into());
            }
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            let run_type = *self.selected.lock().unwrap();
            let (instance_id, state, state_changed_at) = match run_type {
                RunType::Normal => {
                    let (state, changed_at) = self.local_status.lock().unwrap().clone();
                    ("local-core-instance".into(), state, changed_at)
                }
                RunType::Service if self.service_running.load(AtomicOrdering::Acquire) => (
                    "9af54a67-fc31-4dcb-8422-26644c6277c8".into(),
                    CoreState::Running,
                    1,
                ),
                RunType::Service => (
                    "9af54a67-fc31-4dcb-8422-26644c6277c8".into(),
                    CoreState::Stopped(None),
                    2,
                ),
                RunType::Elevated => (String::new(), CoreState::Stopped(None), 0),
            };
            Ok(CoreStatusSnapshot {
                instance_id: (!instance_id.is_empty()).then_some(instance_id),
                state,
                state_changed_at,
                run_type,
            })
        }

        async fn select_execution_host(
            &self,
            host: crate::core::actor_v2::control_endpoint::ExecutionHost,
        ) -> anyhow::Result<()> {
            let run_type = match host {
                crate::core::actor_v2::control_endpoint::ExecutionHost::Local => RunType::Normal,
                crate::core::actor_v2::control_endpoint::ExecutionHost::Service => RunType::Service,
            };
            *self.selected.lock().unwrap() = run_type;
            self.events.lock().unwrap().push(match run_type {
                RunType::Normal => "select-local",
                RunType::Service => "select-service",
                RunType::Elevated => unreachable!(),
            });
            Ok(())
        }

        async fn active_runtime_receipt(
            &self,
        ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
            let query = self.receipt_queries.fetch_add(1, AtomicOrdering::AcqRel);
            if self.invalidate_receipt_after_first_query && query > 0 {
                return Ok(None);
            }
            let status = self.status().await?;
            if !matches!(status.state, CoreState::Running) {
                return Ok(None);
            }
            Ok(match status.run_type {
                RunType::Normal => self.local_receipt.lock().unwrap().clone(),
                RunType::Service => self.service_receipt.lock().unwrap().clone(),
                RunType::Elevated => None,
            })
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            let status = self.status().await?;
            if matches!(status.state, CoreState::Stopped(_)) {
                Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
            } else {
                Ok(crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied)
            }
        }

        async fn retire_service_core(
            &self,
            _expected_owner: chimera_core_manager::CoreOwner,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-core-retire");
            self.service_running.store(false, AtomicOrdering::Release);
            if let Some((kind, retryable)) = self.service_retire_error {
                if matches!(
                    kind,
                    chimera_core_manager::CoreErrorKind::StopUnconfirmed
                        | chimera_core_manager::CoreErrorKind::Quarantined
                ) {
                    self.outcome_uncertain.store(true, AtomicOrdering::Release);
                }
                return Err(chimera_core_manager::CoreError::new(
                    kind,
                    format!("injected {kind:?} Service stop error"),
                    retryable,
                )
                .into());
            }
            Ok(())
        }

        fn outcome_uncertain(&self) -> bool {
            self.outcome_uncertain.load(AtomicOrdering::Acquire)
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl ServiceLifecyclePort for MutableStartupServiceProbe {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.events.lock().unwrap().push("service-probe");
            let running = self.running.load(AtomicOrdering::Acquire);
            let state = if running {
                CoreState::Running
            } else {
                CoreState::Stopped(self.stopped_reason.clone())
            };
            let detail = if running {
                chimera_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }
            } else {
                chimera_ipc::api::status::CoreStateDetail::Stopped {
                    reason: self.stopped_reason.clone(),
                }
            };
            Ok(chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("1.11.1"),
                status: chimera_ipc::types::ServiceStatus::Running,
                server: Some(chimera_ipc::api::status::StatusResBody {
                    version: std::borrow::Cow::Borrowed("1.11.1"),
                    core_infos: chimera_ipc::api::status::CoreInfos {
                        instance_id: Some("9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
                        r#type: None,
                        state,
                        state_changed_at: if running { 1 } else { 2 },
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: Some(chimera_ipc::api::status::ConfigRevisionInfo {
                            epoch: 1,
                            generation: 3,
                            source_hash: "89abcdef01234567".into(),
                            effective_hash: "0123456789abcdef".into(),
                        }),
                        detail: Some(detail),
                    },
                    runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                        service_data_dir: std::borrow::Cow::Owned(Default::default()),
                        service_config_dir: std::borrow::Cow::Owned(Default::default()),
                        nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                        nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                    },
                }),
            })
        }

        fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
            self.runtime_owned
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            Ok(Box::new(RecordingServiceTransition {
                events: self.events.clone(),
            }))
        }
    }

    #[async_trait]
    impl ServiceLifecyclePort for ForeignAfterRecoveryServiceProbe {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.inner.probe().await
        }

        fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
            self.inner.runtime_owned
                && !self
                    .inner
                    .events
                    .lock()
                    .unwrap()
                    .contains(&"service-core-recover")
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            self.inner.begin_transition().await
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for StartupReceiptCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            *self.status.lock().unwrap() = (CoreState::Running, 3);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop");
            if self.stop_confirms {
                *self.status.lock().unwrap() = (CoreState::Stopped(None), 2);
            }
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            anyhow::bail!("startup receipt test does not select a core")
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            let (state, state_changed_at) = self.status.lock().unwrap().clone();
            Ok(CoreStatusSnapshot {
                instance_id: Some("local-core-instance".into()),
                state,
                state_changed_at,
                run_type: RunType::Normal,
            })
        }

        async fn active_runtime_receipt(
            &self,
        ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
            let (state, _) = self.status.lock().unwrap().clone();
            Ok(matches!(state, CoreState::Running)
                .then(|| self.receipt.lock().unwrap().clone())
                .flatten())
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            let (state, _) = self.status.lock().unwrap().clone();
            match state {
                CoreState::Stopped(_) => {
                    Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
                }
                CoreState::Running => Ok(
                    crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied,
                ),
            }
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for StartupDegradedServiceCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            let attempt = self.reconciles.fetch_add(1, AtomicOrdering::AcqRel);
            if attempt == 0 {
                self.events.lock().unwrap().push("rebuild-service-failed");
                anyhow::bail!("injected Service reconciliation failure")
            }
            self.events.lock().unwrap().push("rebuild-local");
            Ok(())
        }

        async fn retire_service_core(
            &self,
            _expected_owner: chimera_core_manager::CoreOwner,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("service-core-retire");
            self.stopped.store(true, AtomicOrdering::Release);
            Ok(())
        }

        async fn reconcile_after_service_release(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
        ) -> anyhow::Result<()> {
            self.events
                .lock()
                .unwrap()
                .push("rebuild-after-service-release");
            *self.selected.lock().unwrap() = RunType::Normal;
            self.stopped.store(false, AtomicOrdering::Release);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn select_execution_host(
            &self,
            host: crate::core::actor_v2::control_endpoint::ExecutionHost,
        ) -> anyhow::Result<()> {
            *self.selected.lock().unwrap() = match host {
                crate::core::actor_v2::control_endpoint::ExecutionHost::Local => RunType::Normal,
                crate::core::actor_v2::control_endpoint::ExecutionHost::Service => RunType::Service,
            };
            self.events.lock().unwrap().push(match host {
                crate::core::actor_v2::control_endpoint::ExecutionHost::Local => "select-local",
                crate::core::actor_v2::control_endpoint::ExecutionHost::Service => "select-service",
            });
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            let stopped = self.stopped.load(AtomicOrdering::Acquire);
            let run_type = *self.selected.lock().unwrap();
            Ok(CoreStatusSnapshot {
                instance_id: (run_type == RunType::Service).then(|| "service-core-instance".into()),
                state: if stopped {
                    CoreState::Stopped(None)
                } else {
                    CoreState::Running
                },
                state_changed_at: if stopped { 2 } else { 1 },
                run_type,
            })
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for StartupTransientCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            let attempt = self.reconciles.fetch_add(1, AtomicOrdering::AcqRel);
            if attempt == 0 {
                self.events
                    .lock()
                    .unwrap()
                    .push("rebuild-transient-failure");
                return Err(anyhow::Error::new(chimera_core_manager::CoreError::new(
                    chimera_core_manager::CoreErrorKind::BackendUnavailable,
                    "temporary backend outage",
                    true,
                )));
            }
            self.events.lock().unwrap().push("rebuild-recovered");
            self.recovered.notify_one();
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            let attempt = self.stops.fetch_add(1, AtomicOrdering::AcqRel);
            if attempt == 1
                && let Some((started, release)) = &self.stop_gate
            {
                started.notify_one();
                release.notified().await;
            }
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            let reconciles = self.reconciles.load(AtomicOrdering::Acquire);
            let stopped = self.stops.load(AtomicOrdering::Acquire) > 0 && reconciles < 2;
            Ok(CoreStatusSnapshot {
                instance_id: Some("local-core-instance".into()),
                state: if stopped {
                    CoreState::Stopped(None)
                } else {
                    CoreState::Running
                },
                state_changed_at: if stopped { 2 } else { 1 },
                run_type: RunType::Normal,
            })
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            let reconciles = self.reconciles.load(AtomicOrdering::Acquire);
            let stopped = self.stops.load(AtomicOrdering::Acquire) > 0 && reconciles < 2;
            Ok(if stopped {
                crate::client::application_workflow::mutation::KnownRuntimeState::Stopped
            } else {
                crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied
            })
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        fn outcome_uncertain(&self) -> bool {
            self.outcome_uncertain
        }

        async fn on_profile_change(&self, _break_when: bool) {}
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

        fn restart_policy(&self) -> ServiceRestartPolicySnapshot {
            ServiceRestartPolicySnapshot {
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
    impl ServiceLifecyclePort for StartupRetryLeaseService {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            self.inner.probe().await
        }

        fn restart_policy(&self) -> ServiceRestartPolicySnapshot {
            self.inner.restart_policy()
        }

        async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
            let attempt = self
                .transition_attempts
                .fetch_add(1, AtomicOrdering::AcqRel);
            if attempt == 1 {
                self.inner
                    .events
                    .lock()
                    .unwrap()
                    .push("startup-retry-lease-failed");
                if let Some((started, release)) = &self.retry_lease_gate {
                    started.notify_one();
                    release.notified().await;
                }
                anyhow::bail!("test Service lease failure");
            }
            self.inner.begin_transition().await
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
        async fn install(
            &self,
            artifact: &chimera_contracts::core_install::CoreBinaryInstallRequest,
        ) -> Result<(), chimera_contracts::core_install::InstallCoreBinaryError> {
            // Verify the lifecycle forwards the staged input to the upstream
            // contract without introducing unsupported companion assets.
            assert!(artifact.source.starts_with(artifact.staging.path()));
            assert!(artifact.companion.is_none());
            assert!(!artifact.destination.as_os_str().is_empty());
            self.events.lock().unwrap().push("install");
            Ok(())
        }
    }

    #[async_trait]
    impl ports::BinaryInstaller for FailingInstaller {
        async fn install(
            &self,
            artifact: &chimera_contracts::core_install::CoreBinaryInstallRequest,
        ) -> Result<(), chimera_contracts::core_install::InstallCoreBinaryError> {
            self.events.lock().unwrap().push("install");
            Err(
                chimera_contracts::core_install::InstallCoreBinaryError::ElevatedCopyFailed {
                    core: artifact.target,
                    destination: (&artifact.destination).into(),
                    exit_code: Some(1),
                },
            )
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
    impl CoreLifecyclePort for CoreUpdateRecordingCore {
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
            self.events.lock().unwrap().push("stop");
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select");
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: self.state.clone(),
                state_changed_at: self.state_changed_at,
                run_type: self.run_type,
            })
        }

        async fn active_runtime_receipt(
            &self,
        ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
            if self.receipt_error {
                anyhow::bail!("active core status is unavailable");
            }
            let Some(kind) = self.active_kind else {
                return Ok(None);
            };
            if !matches!(self.state, CoreState::Running) {
                return Ok(None);
            }
            let target_core = match kind {
                chimera_core_manager::CoreKind::Mihomo => ClashCore::Mihomo,
                chimera_core_manager::CoreKind::ClashRust => ClashCore::ClashRs,
                chimera_core_manager::CoreKind::ClashPremium => ClashCore::ClashPremium,
                chimera_core_manager::CoreKind::ChimeraClient => ClashCore::ChimeraClient,
                chimera_core_manager::CoreKind::Meow => return Ok(None),
            };
            let mut receipt = crate::client::ports::test_runtime_receipt_with_mixed_port(7892);
            receipt.target_core = target_core;
            receipt.core_spec.kind = kind;
            Ok(Some(Arc::new(receipt)))
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
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
                state_changed_at: if self.run_type == RunType::Service {
                    1
                } else {
                    0
                },
                run_type: self.run_type,
            })
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            Some(self.recovery_notify.clone())
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for ConfirmedStoppedCore {
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
            self.events.lock().unwrap().push("stop");
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
                state_changed_at: 1,
                run_type: self.run_type,
            })
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[async_trait]
    impl CoreLifecyclePort for ProfileTccCore {
        async fn reconcile(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn reconcile_profiles(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
            profiles: Arc<chimera_config::profile::Profiles>,
            _app: chimera_config::application::ChimeraAppConfig,
            _staged_content: std::collections::BTreeMap<String, String>,
            _target: Option<String>,
        ) -> anyhow::Result<()> {
            if self.fail_next_reconcile.swap(false, AtomicOrdering::AcqRel) {
                anyhow::bail!("injected current Profiles reconcile failure");
            }
            *self.active_profiles.lock().unwrap() = Some((*profiles).clone());
            Ok(())
        }

        async fn validate_profile_runtime(
            &self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
            _profiles: Arc<chimera_config::profile::Profiles>,
            _app: chimera_config::application::ChimeraAppConfig,
            _staged_content: std::collections::BTreeMap<String, String>,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
            self.validations.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(crate::client::application_workflow::mutation::CheckRecord::Passed)
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.stops.fetch_add(1, AtomicOrdering::SeqCst);
            *self.active_profiles.lock().unwrap() = None;
            Ok(())
        }

        async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 1,
                run_type: RunType::Normal,
            })
        }

        async fn observe_runtime_baseline(
            &self,
        ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState>
        {
            Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
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
            _target: Option<String>,
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
            chimera_config::application::ReleaseChannel::Stable,
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
            chimera_config::application::ReleaseChannel::Stable,
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
        let client = spawn_startup_client(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client
            .startup_reconcile()
            .await
            .expect("startup reconcile should complete through the lifecycle actor");
        completed
            .result
            .expect("startup reconcile should return its terminal operation result");
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        assert!(events.lock().unwrap().contains(&"rebuild"));
    }

    #[tokio::test]
    async fn startup_recovery_keeps_a_receipt_only_for_the_latest_committed_target() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let core = Arc::new(StartupReceiptCore {
            events: events.clone(),
            status: Mutex::new((CoreState::Running, 1)),
            receipt: Mutex::new(None),
            stop_confirms: true,
        });
        let application = ApplicationClient::legacy().unwrap();
        let clash = ClashConfigClient::legacy().unwrap();
        let service: Arc<dyn ServiceLifecyclePort> = Arc::new(RecordingService {
            events: events.clone(),
        });
        let target = startup_target_for(
            application.clone(),
            clash.clone(),
            core.clone(),
            service.clone(),
        )
        .await;
        *core.receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
        let client =
            spawn_startup_client_with_application_and_clash(core, service, application, clash)
                .await;

        client
            .recover_startup()
            .await
            .expect("matching recovery receipt should be accepted");

        let events = events.lock().unwrap();
        assert!(!events.contains(&"stop"), "events: {events:?}");
        assert!(!events.contains(&"rebuild"), "events: {events:?}");
        assert!(!client.status().uncertain);
    }

    #[tokio::test]
    async fn initial_startup_retires_a_matching_running_receipt_before_reconcile() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let core = Arc::new(StartupReceiptCore {
            events: events.clone(),
            status: Mutex::new((CoreState::Running, 1)),
            receipt: Mutex::new(None),
            stop_confirms: true,
        });
        let application = ApplicationClient::legacy().unwrap();
        let clash = ClashConfigClient::legacy().unwrap();
        let service: Arc<dyn ServiceLifecyclePort> = Arc::new(RecordingService {
            events: events.clone(),
        });
        let target = startup_target_for(
            application.clone(),
            clash.clone(),
            core.clone(),
            service.clone(),
        )
        .await;
        *core.receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
        let client =
            spawn_startup_client_with_application_and_clash(core, service, application, clash)
                .await;

        let completed = client
            .startup_reconcile()
            .await
            .expect("initial startup should stop and rebuild the live target");
        let events = events.lock().unwrap();
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        let stop = events.iter().position(|event| *event == "stop").unwrap();
        let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
        assert!(stop < rebuild);
    }

    /// ST-LIFE-01; isolated lifecycle integration using mock Local/Service ports.
    /// Given Service and Local both report a running Core, when startup resolves
    /// to Local, Service release and a confirmed Local stop must precede apply.
    /// This catches owner-plan branches that skip the shared S3/S4 path.
    #[tokio::test]
    async fn startup_retires_service_then_stops_running_local_owner_before_apply() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(true));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            service_running.clone(),
        ));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: None,
            runtime_owned: true,
        });
        let client = spawn_startup_client(core, service).await;

        let completed = client.startup_reconcile().await.unwrap();
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);

        let events = events.lock().unwrap();
        let retire_service = events
            .iter()
            .position(|event| *event == "service-core-retire")
            .unwrap();
        let stop_local = events
            .iter()
            .position(|event| *event == "stop-local")
            .unwrap();
        let apply_local = events
            .iter()
            .position(|event| *event == "rebuild-local")
            .unwrap();
        assert!(retire_service < stop_local && stop_local < apply_local);
    }

    /// ST-LIFE-02; isolated lifecycle integration with a matching Recovery receipt.
    /// Given Service release leaves the same Local runtime and exact target,
    /// recovery may keep that receipt; it must not stop or rebuild Local.
    #[tokio::test]
    async fn recovery_keeps_local_owner_after_service_retirement_only_with_matching_receipt() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(true));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            service_running.clone(),
        ));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: None,
            runtime_owned: true,
        });
        let application = ApplicationClient::legacy().unwrap();
        let clash = ClashConfigClient::legacy().unwrap();
        let target = startup_target_for(
            application.clone(),
            clash.clone(),
            core.clone(),
            service.clone(),
        )
        .await;
        *core.local_receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
        let client =
            spawn_startup_client_with_application_and_clash(core, service, application, clash)
                .await;

        client.recover_startup().await.unwrap();
        assert!(!client.status().uncertain);

        let events = events.lock().unwrap();
        assert!(events.contains(&"service-core-retire"));
        assert!(!events.contains(&"stop-local"));
        assert!(!events.contains(&"rebuild-local"));
    }

    /// ST-LIFE-03; isolated lifecycle integration using mock hosts.
    /// Given startup adopts a ready Service while Local is running, it must
    /// confirm Local stopped, select Service, retire any unreceipted Service
    /// runtime, and only then apply. Event order is the regression oracle.
    #[tokio::test]
    async fn startup_adopts_service_then_stops_running_service_owner_before_apply() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(true));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            service_running.clone(),
        ));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: None,
            runtime_owned: true,
        });
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = true;
        let client = spawn_startup_client_with_application(
            core,
            service,
            ApplicationClient::static_state(app),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);

        let events = events.lock().unwrap();
        let stop_local = events
            .iter()
            .position(|event| *event == "stop-local")
            .unwrap();
        let select_service = events
            .iter()
            .position(|event| *event == "select-service")
            .unwrap();
        let retire_service = events
            .iter()
            .position(|event| *event == "service-core-retire")
            .unwrap();
        let apply_service = events
            .iter()
            .position(|event| *event == "rebuild-service")
            .unwrap();
        assert!(stop_local < select_service);
        assert!(select_service < retire_service && retire_service < apply_service);
    }

    /// ST-LIFE-04; isolated lifecycle integration with a matching Service receipt.
    /// Given recovery adopts the ready Service and its live receipt matches the
    /// committed target, it keeps the owner without stop/rebuild side effects.
    #[tokio::test]
    async fn recovery_keeps_adopted_service_owner_only_with_matching_receipt() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(true));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Stopped(None), 0),
            service_running.clone(),
        ));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: None,
            runtime_owned: true,
        });
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = true;
        let application = ApplicationClient::static_state(app);
        let clash = ClashConfigClient::legacy().unwrap();
        let target = startup_target_for(
            application.clone(),
            clash.clone(),
            core.clone(),
            service.clone(),
        )
        .await;
        *core.service_receipt.lock().unwrap() = Some(startup_test_receipt_for(
            Some(target),
            crate::core::actor_v2::control_endpoint::ExecutionHost::Service,
        ));
        let client =
            spawn_startup_client_with_application_and_clash(core, service, application, clash)
                .await;

        client.recover_startup().await.unwrap();
        assert!(!client.status().uncertain);

        let events = events.lock().unwrap();
        assert!(events.contains(&"select-service"));
        assert!(!events.contains(&"service-core-retire"));
        assert!(!events.contains(&"rebuild-service"));
    }

    /// ST-LIFE-05; isolated receipt-adapter integration.
    /// The coarse status remains Running with the same timestamp while the
    /// adapter invalidates the rich receipt on re-read (the in-place Service
    /// revision case); recovery must stop and rebuild instead of accepting it.
    #[tokio::test]
    async fn recovery_rechecks_rich_receipt_after_coarse_running_status() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(false));
        let mut core = startup_owner_core(events.clone(), (CoreState::Running, 1), service_running);
        core.invalidate_receipt_after_first_query = true;
        let core = Arc::new(core);
        let application = ApplicationClient::legacy().unwrap();
        let clash = ClashConfigClient::legacy().unwrap();
        let target = startup_target_for(
            application.clone(),
            clash.clone(),
            core.clone(),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;
        *core.local_receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
        let client = spawn_startup_client_with_application_and_clash(
            core,
            Arc::new(RecordingService {
                events: events.clone(),
            }),
            application,
            clash,
        )
        .await;

        client.recover_startup().await.unwrap();
        assert!(!client.status().uncertain);

        let events = events.lock().unwrap();
        let stop = events
            .iter()
            .position(|event| *event == "stop-local")
            .unwrap();
        let apply = events
            .iter()
            .position(|event| *event == "rebuild-local")
            .unwrap();
        assert!(stop < apply);
    }

    /// ST-LIFE-10; isolated lifecycle integration for an uncertain Service.
    /// Given conditional stop returns StopUnconfirmed and the backend's later
    /// projection is error-stopped, startup must isolate the owner without
    /// trusting that projection; neither Local stop nor Local apply may run.
    #[tokio::test]
    async fn service_stop_unconfirmed_does_not_trust_error_stopped_projection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(true));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            service_running.clone(),
        );
        core.service_retire_error =
            Some((chimera_core_manager::CoreErrorKind::StopUnconfirmed, false));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: Some("quarantined: stop unconfirmed".into()),
            runtime_owned: true,
        });
        let workflow = CoreLifecycleWorkflow::new(
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            None,
            Arc::new(core),
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            service.clone(),
        );
        let mut startup_state = StartupWorkflowState::default();

        let report = workflow
            .startup_reconcile(
                1,
                Box::new(RecordingServiceTransition {
                    events: events.clone(),
                }),
                startup_state.retry_target_mut(),
            )
            .await;

        assert!(matches!(
            &report.outcome,
            StartupOutcome::RecoveryRequired { .. }
        ));
        {
            let events = events.lock().unwrap();
            let retirement = events
                .iter()
                .position(|event| *event == "service-core-retire")
                .unwrap();
            assert!(
                events[retirement + 1..]
                    .iter()
                    .all(|event| *event != "service-probe"),
                "startup must not reinterpret the post-stop projection as proof"
            );
            assert!(!events.contains(&"stop-local"));
            assert!(!events.contains(&"rebuild-local"));
        }
        let info = service.probe().await.unwrap();
        assert!(matches!(
            info.server
                .as_ref()
                .and_then(|server| server.core_infos.detail.as_ref()),
            Some(chimera_ipc::api::status::CoreStateDetail::Stopped {
                reason: Some(reason)
            }) if reason.contains("quarantined")
        ));
    }

    /// ST-LIFE-11; isolated lifecycle integration for deterministic Service stop failure.
    /// A fresh Service Stopped probe cannot turn InvalidConfig into success;
    /// startup blocks before touching the running Local owner.
    #[tokio::test]
    async fn deterministic_service_stop_error_with_stopped_probe_remains_blocked() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(true));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            service_running.clone(),
        );
        core.service_retire_error =
            Some((chimera_core_manager::CoreErrorKind::InvalidConfig, false));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: None,
            runtime_owned: true,
        });
        let client = spawn_startup_client(Arc::new(core), service).await;

        let completed = client.startup_reconcile().await.unwrap();

        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                ..
            }
        ));
        let events = events.lock().unwrap();
        assert!(events.contains(&"service-core-retire"));
        assert!(!events.contains(&"stop-local"));
        assert!(!events.contains(&"rebuild-local"));
    }

    /// ST-LIFE-12; isolated lifecycle integration for Local stop error handling.
    /// InvalidConfig remains a deterministic failure even when a fresh status
    /// says Stopped; only NotStarted has the idempotent exception.
    #[tokio::test]
    async fn deterministic_local_stop_error_with_fresh_stopped_state_remains_blocked() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            Arc::new(AtomicBool::new(false)),
        );
        core.stop_error = Some((chimera_core_manager::CoreErrorKind::InvalidConfig, false));
        let client = spawn_startup_client(
            Arc::new(core),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();

        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                ..
            }
        ));
        let events = events.lock().unwrap();
        assert!(events.contains(&"stop-local"));
        assert!(!events.contains(&"rebuild-local"));
    }

    /// ST-LIFE-13; startup recovery for a Service manager across app restarts.
    /// Given a stopped Service status carries a prior error reason, the manager
    /// quarantine recovery must finish before a new Local Core is applied.
    #[tokio::test]
    async fn startup_recovers_error_stopped_service_before_local_apply() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(false));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Stopped(None), 0),
            service_running.clone(),
        ));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: Some("service manager quarantine".into()),
            runtime_owned: true,
        });
        let workflow = CoreLifecycleWorkflow::new(
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            None,
            core,
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            service,
        );
        let mut startup_state = StartupWorkflowState::default();

        let report = workflow
            .startup_reconcile(
                1,
                Box::new(RecordingServiceTransition {
                    events: events.clone(),
                }),
                startup_state.retry_target_mut(),
            )
            .await;

        assert_eq!(report.outcome, StartupOutcome::Ready);
        let events = events.lock().unwrap();
        let recover = events
            .iter()
            .position(|event| *event == "service-core-recover")
            .unwrap();
        let apply = events
            .iter()
            .position(|event| *event == "rebuild-local")
            .unwrap();
        assert!(recover < apply);
    }

    /// ST-LIFE-15; startup must not recover another installation's Service manager.
    /// A foreign stopped status with an error reason remains unproven because
    /// manager recovery may terminate a quarantined process owned by that app.
    #[tokio::test]
    async fn foreign_error_stopped_service_is_not_recovered_or_replaced() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(false));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Stopped(None), 0),
            service_running.clone(),
        ));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: Some("foreign service manager quarantine".into()),
            runtime_owned: false,
        });
        let workflow = CoreLifecycleWorkflow::new(
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            None,
            core,
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            service,
        );
        let mut startup_state = StartupWorkflowState::default();

        let report = workflow
            .startup_reconcile(
                1,
                Box::new(RecordingServiceTransition {
                    events: events.clone(),
                }),
                startup_state.retry_target_mut(),
            )
            .await;

        assert!(
            matches!(
                &report.outcome,
                StartupOutcome::ReadyDegraded { reason, .. }
                    if reason.contains("may still hold a Core")
            ),
            "foreign Service should keep the owner unproven: {:?}",
            report.outcome
        );
        let events = events.lock().unwrap();
        assert!(!events.contains(&"service-core-recover"));
        assert!(!events.contains(&"service-core-retire"));
        assert!(!events.contains(&"stop-local"));
        assert!(!events.contains(&"rebuild-local"));
    }

    /// ST-LIFE-16; a Service replacement observed after recovery stays isolated.
    /// The fresh probe must still identify the same Service generation and
    /// this installation before startup treats the stopped reason as cleared.
    #[tokio::test]
    async fn service_owner_change_during_recovery_blocks_local_apply() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(false));
        let core = Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Stopped(None), 0),
            service_running.clone(),
        ));
        let service = Arc::new(ForeignAfterRecoveryServiceProbe {
            inner: MutableStartupServiceProbe {
                events: events.clone(),
                running: service_running,
                stopped_reason: Some("service manager quarantine".into()),
                runtime_owned: true,
            },
        });
        let workflow = CoreLifecycleWorkflow::new(
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            None,
            core,
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            service,
        );
        let mut startup_state = StartupWorkflowState::default();

        let report = workflow
            .startup_reconcile(
                1,
                Box::new(RecordingServiceTransition {
                    events: events.clone(),
                }),
                startup_state.retry_target_mut(),
            )
            .await;

        assert!(matches!(
            report.outcome,
            StartupOutcome::RecoveryRequired { .. }
        ));
        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "service-core-recover")
                .count(),
            1
        );
        assert!(!events.contains(&"service-core-retire"));
        assert!(!events.contains(&"stop-local"));
        assert!(!events.contains(&"rebuild-local"));
    }

    /// ST-LIFE-14; unresolved Service quarantine must fail closed.
    /// If manager recovery cannot prove the quarantined process dead, startup
    /// reports RecoveryRequired and does not apply a Local runtime.
    #[tokio::test]
    async fn unresolved_service_quarantine_prevents_local_apply() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service_running = Arc::new(AtomicBool::new(false));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Stopped(None), 0),
            service_running.clone(),
        );
        core.service_recover_error =
            Some((chimera_core_manager::CoreErrorKind::Quarantined, false));
        let service = Arc::new(MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: Some("service manager quarantine".into()),
            runtime_owned: true,
        });
        let workflow = CoreLifecycleWorkflow::new(
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            None,
            Arc::new(core),
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            service,
        );
        let mut startup_state = StartupWorkflowState::default();

        let report = workflow
            .startup_reconcile(
                1,
                Box::new(RecordingServiceTransition {
                    events: events.clone(),
                }),
                startup_state.retry_target_mut(),
            )
            .await;

        assert!(matches!(
            report.outcome,
            StartupOutcome::RecoveryRequired { .. }
        ));
        let events = events.lock().unwrap();
        assert!(events.contains(&"service-core-recover"));
        assert!(!events.contains(&"rebuild-local"));
    }

    /// ST-LIFE-06; isolated lifecycle integration with a typed deterministic stop error.
    /// Given the Local owner remains Running and reports InvalidConfig on stop,
    /// startup blocks and never applies over the still-running owner.
    #[tokio::test]
    async fn deterministic_local_stop_failure_blocks_startup_retry() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            Arc::new(AtomicBool::new(false)),
        );
        core.stop_confirms = false;
        core.stop_error = Some((chimera_core_manager::CoreErrorKind::InvalidConfig, false));
        let client = spawn_startup_client(
            Arc::new(core),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                ..
            }
        ));
        assert!(!events.lock().unwrap().contains(&"rebuild-local"));
    }

    /// ST-LIFE-07; isolated lifecycle integration with an uncertain stop result.
    /// Given the stop outcome cannot be established, startup requires explicit
    /// recovery and must not issue a second runtime mutation.
    #[tokio::test]
    async fn uncertain_local_stop_requires_explicit_recovery() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            Arc::new(AtomicBool::new(false)),
        );
        core.stop_confirms = false;
        core.stop_error = Some((chimera_core_manager::CoreErrorKind::StopUnconfirmed, false));
        let client = spawn_startup_client(
            Arc::new(core),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::RecoveryRequired { .. }
        ));
        assert!(!events.lock().unwrap().contains(&"rebuild-local"));
    }

    /// ST-LIFE-08; isolated lifecycle integration where stop is accepted but
    /// status still says Running. Startup waits for fresh owner evidence and
    /// must not apply until a later retry can prove the stop.
    #[tokio::test]
    async fn accepted_local_stop_waits_while_owner_is_still_running() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            Arc::new(AtomicBool::new(false)),
        );
        core.stop_confirms = false;
        let client = spawn_startup_client(
            Arc::new(core),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                ..
            }
        ));
        assert!(!events.lock().unwrap().contains(&"rebuild-local"));
    }

    /// ST-LIFE-09; isolated lifecycle integration for idempotent NotStarted.
    /// Given stop returns NotStarted but a fresh status and runtime baseline
    /// prove Stopped, startup accepts that evidence and applies afterwards.
    #[tokio::test]
    async fn not_started_stop_is_accepted_after_fresh_stopped_confirmation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut core = startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            Arc::new(AtomicBool::new(false)),
        );
        core.stop_error = Some((chimera_core_manager::CoreErrorKind::NotStarted, false));
        let client = spawn_startup_client(
            Arc::new(core),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        let events = events.lock().unwrap();
        let stop = events
            .iter()
            .position(|event| *event == "stop-local")
            .unwrap();
        let apply = events
            .iter()
            .position(|event| *event == "rebuild-local")
            .unwrap();
        assert!(stop < apply);
    }

    #[tokio::test]
    async fn recovery_retires_a_running_receipt_when_its_target_is_stale() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(StartupReceiptCore {
                events: events.clone(),
                status: Mutex::new((CoreState::Running, 1)),
                receipt: Mutex::new(Some(startup_test_receipt(Some("older-target".into())))),
                stop_confirms: true,
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        client
            .recover_startup()
            .await
            .expect("stale recovery receipt should be retired and reapplied");

        let events = events.lock().unwrap();
        let stop = events.iter().position(|event| *event == "stop").unwrap();
        let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
        assert!(stop < rebuild);
    }

    #[tokio::test]
    async fn startup_does_not_reconcile_when_a_local_stop_is_unconfirmed() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(StartupReceiptCore {
                events: events.clone(),
                status: Mutex::new((CoreState::Running, 1)),
                receipt: Mutex::new(None),
                stop_confirms: false,
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client
            .startup_reconcile()
            .await
            .expect("unconfirmed stop should return a startup report");

        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                ..
            }
        ));
        let events = events.lock().unwrap();
        assert!(events.contains(&"stop"));
        assert!(!events.contains(&"rebuild"));
    }

    #[tokio::test]
    async fn startup_retries_a_retryable_core_failure_through_the_lifecycle_actor() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovered = Arc::new(Notify::new());
        let service = Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        });
        let client = spawn_startup_client(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered: recovered.clone(),
                outcome_uncertain: false,
                stop_gate: None,
            }),
            service,
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));
        assert!(initial.result.is_err());
        assert!(
            events
                .lock()
                .unwrap()
                .contains(&"rebuild-transient-failure")
        );

        tokio::time::timeout(Duration::from_secs(5), recovered.notified())
            .await
            .expect("actor should retry a transient startup failure");

        let events = events.lock().unwrap();
        assert!(events.contains(&"rebuild-recovered"));
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "rebuild-transient-failure")
                .count(),
            1
        );
        let cached = client.startup_reconcile().await.unwrap();
        assert_eq!(cached.report.operation_id, initial.report.operation_id);
        assert!(matches!(
            cached.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));
        assert!(!client.status().uncertain);
    }

    #[tokio::test]
    async fn disconnected_service_mode_patch_refreshes_the_startup_retry_target() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovered = Arc::new(Notify::new());
        let application = ApplicationClient::static_state(
            chimera_config::application::ChimeraAppConfig::default(),
        );
        let client = spawn_startup_client_with_application(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered: recovered.clone(),
                outcome_uncertain: false,
                stop_gate: None,
            }),
            Arc::new(RestartBudgetService {
                events: events.clone(),
                attempts: AtomicU8::new(0),
                exhausted: AtomicBool::new(false),
            }),
            application,
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));

        client
            .patch_application_config(ChimeraAppConfigPatch {
                enable_service_mode: Some(true),
                ..Default::default()
            })
            .await
            .expect(
                "disconnected Service Mode preference should be saved without runtime reconcile",
            );

        tokio::time::timeout(Duration::from_millis(800), recovered.notified())
            .await
            .expect("the changed committed target should retry immediately");
        let events = events.lock().unwrap();
        assert!(events.contains(&"rebuild-recovered"));
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "rebuild-transient-failure")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn startup_retry_lease_failure_is_drained_after_actor_operation_returns() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovered = Arc::new(Notify::new());
        let retry_lease_started = Arc::new(Notify::new());
        let release_retry_lease = Arc::new(Notify::new());
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let service = Arc::new(StartupRetryLeaseService {
            inner: RestartBudgetService {
                events: events.clone(),
                attempts: AtomicU8::new(0),
                exhausted: AtomicBool::new(false),
            },
            transition_attempts: AtomicU8::new(0),
            retry_lease_gate: Some((retry_lease_started.clone(), release_retry_lease.clone())),
        });
        let client = spawn_startup_client(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered,
                outcome_uncertain: false,
                stop_gate: Some((stop_started.clone(), release_stop.clone())),
            }),
            service,
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));
        let (identity, next_attempt) = client
            .startup_retry_schedule_for_tests()
            .await
            .unwrap()
            .expect("startup failure should schedule a retry");
        assert!(next_attempt > tokio::time::Instant::now());
        let lease_failure_observed = client
            .observe_next_startup_retry_lease_failure()
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(8), retry_lease_started.notified())
            .await
            .expect("the retry should begin acquiring its Service lease");

        let stopping_client = client.clone();
        let stop = tokio::spawn(async move { stopping_client.stop_core().await });
        tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
            .await
            .expect("the unrelated stop should take and hold ApplicationWorkflow");

        release_retry_lease.notify_one();
        tokio::time::timeout(Duration::from_secs(1), lease_failure_observed)
            .await
            .expect("the lease failure should be handled while the workflow is busy")
            .expect("the actor should acknowledge the handled lease failure");
        assert!(!stop.is_finished());

        release_stop.notify_one();
        stop.await
            .expect("the stop request task should complete")
            .expect("the stop operation should succeed");

        let (rescheduled_identity, rescheduled_at) = client
            .startup_retry_schedule_for_tests()
            .await
            .unwrap()
            .expect("the completed actor operation should restore and reschedule the retry");
        assert_eq!(rescheduled_identity, identity);
        assert!(rescheduled_at > tokio::time::Instant::now());
        client.shutdown().await.unwrap();
    }

    #[test]
    fn startup_retry_lease_failure_is_retained_until_application_workflow_returns() {
        let now = tokio::time::Instant::now();
        let mut target = super::super::application_workflow::startup::StartupRetryTarget::new(
            "retry-target".into(),
            false,
        );
        target.schedule_dependency_at(now - Duration::from_secs(10), "initial retry".into());
        assert!(target.begin(now));

        let mut workflow = ApplicationWorkflow::for_startup_retry_tests();
        workflow.set_startup_retry_target_for_tests(target);
        let mut pending = VecDeque::new();

        record_startup_retry_lease_failure(
            None,
            &mut pending,
            "retry-target".into(),
            "Service transition lease is unavailable".into(),
        );
        assert_eq!(pending.len(), 1);
        assert!(workflow.startup_retry_schedule().is_none());

        apply_pending_startup_retry_lease_failures(Some(&mut workflow), &mut pending);
        let (identity, next_attempt) = workflow
            .startup_retry_schedule()
            .expect("restoring the workflow must reschedule its deferred retry");
        assert_eq!(identity, "retry-target");
        assert!(next_attempt > tokio::time::Instant::now());
        assert!(pending.is_empty());
        assert!(!workflow.startup_retry_due("retry-target"));
    }

    #[tokio::test]
    async fn startup_retry_lease_failure_waits_for_dependency_without_spending_attempts() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovered = Arc::new(Notify::new());
        let service = Arc::new(StartupRetryLeaseService {
            inner: RestartBudgetService {
                events: events.clone(),
                attempts: AtomicU8::new(0),
                exhausted: AtomicBool::new(false),
            },
            transition_attempts: AtomicU8::new(0),
            retry_lease_gate: None,
        });
        let client = spawn_startup_client(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered: recovered.clone(),
                outcome_uncertain: false,
                stop_gate: None,
            }),
            service.clone(),
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));
        tokio::time::timeout(Duration::from_secs(8), recovered.notified())
            .await
            .expect("startup retry should resume after the Service lease becomes available");

        assert_eq!(service.transition_attempts.load(AtomicOrdering::Acquire), 3);
        let events = events.lock().unwrap();
        assert!(events.contains(&"startup-retry-lease-failed"));
        assert!(events.contains(&"rebuild-recovered"));
    }

    #[tokio::test]
    async fn preexisting_uncertain_runtime_blocks_startup_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::static_state(
            chimera_config::application::ChimeraAppConfig::default(),
        );
        let client = spawn_startup_client_with_application(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered: Arc::new(Notify::new()),
                outcome_uncertain: true,
                stop_gate: None,
            }),
            Arc::new(RestartBudgetService {
                events: events.clone(),
                attempts: AtomicU8::new(0),
                exhausted: AtomicBool::new(false),
            }),
            application.clone(),
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::RecoveryRequired { .. }
        ));
        assert!(client.status().uncertain);
        client
            .patch_application_config(ChimeraAppConfigPatch {
                enable_service_mode: Some(true),
                ..Default::default()
            })
            .await
            .expect(
                "source-only Service Mode save must remain available after uncertain Core outcome",
            );
        assert!(application.get_typed().enable_service_mode);
        assert!(client.status().uncertain);
        tokio::time::sleep(Duration::from_millis(1_200)).await;

        assert!(
            !events
                .lock()
                .unwrap()
                .contains(&"rebuild-transient-failure")
        );
        assert!(!events.lock().unwrap().contains(&"rebuild-recovered"));
    }

    #[tokio::test]
    async fn unresolved_live_attempt_isolated_until_explicit_reconcile_recovery() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::static_state(
            chimera_config::application::ChimeraAppConfig::default(),
        );
        let client = spawn_startup_client_with_application(
            Arc::new(startup_owner_core(
                events.clone(),
                (CoreState::Running, 1),
                Arc::new(AtomicBool::new(false)),
            )),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
            application.clone(),
        )
        .await;
        client
            .startup_reconcile()
            .await
            .expect("the test actor must establish the startup owner first");
        let original_operation = client
            .seed_unresolved_application_attempt_for_tests()
            .await
            .expect("the production actor should retain an unresolved attempt");

        let error = client
            .select_core(ClashCore::Mihomo)
            .await
            .expect_err("runtime mutations must be isolated while the attempt is unresolved");
        assert!(error.to_string().contains("runtime attempt is unresolved"));
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .unwrap()
                .0,
            Some((original_operation.clone(), true))
        );

        client
            .patch_application_config(ChimeraAppConfigPatch {
                enable_auto_launch: Some(true),
                ..Default::default()
            })
            .await
            .expect("source-only preferences remain writable during runtime recovery");
        assert!(application.get_typed().enable_auto_launch);
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .unwrap()
                .0,
            Some((original_operation, true)),
            "a source-only patch must preserve the unresolved attempt that owns recovery"
        );

        client
            .reconcile()
            .await
            .expect("the regular reconcile entry point should perform explicit recovery");
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .unwrap()
                .0,
            None
        );
        assert!(events.lock().unwrap().contains(&"rebuild-local"));
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn reconcile_preserves_an_undecided_profile_source_attempt_until_commit() {
        let directory = tempfile::tempdir().expect("temporary Profiles directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
            .expect("UTF-8 temporary Profiles path");
        let mut manager = PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .expect("Profiles state manager");
        let before = manager.snapshot_handle().load();
        let version = before.version;
        let mut candidate = before.state.clone();
        candidate.valid.push("pending-source-decision-test".into());
        let profile_snapshot = manager.snapshot_handle();
        let core = Arc::new(ProfileTccCore {
            validations: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            fail_next_reconcile: AtomicBool::new(false),
            active_profiles: Mutex::new(None),
        });
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client_with_profiles(
            core.clone(),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
            profile_snapshot.clone(),
        )
        .await;

        let (decision_tx, decision_rx) = oneshot::channel();
        let (local_write_started_tx, local_write_started_rx) = oneshot::channel();
        let (continue_write_tx, continue_write_rx) = oneshot::channel();
        let source_writer = tokio::spawn(async move {
            manager
                .replace_if_version_with_participant(
                    version,
                    candidate.clone(),
                    move |decision| {
                        decision_tx
                            .send(decision.clone())
                            .expect("the test must receive the source decision");
                        Arc::new(AckOnlyProfileChange)
                    },
                    move || async move {
                        let _ = local_write_started_tx.send(());
                        continue_write_rx.await.map_err(|_| {
                            anyhow::anyhow!("the test must release the source persistence write")
                        })?;
                        Ok(())
                    },
                    || async { Ok(()) },
                )
                .await
        });
        let decision = decision_rx
            .await
            .expect("source transaction publishes its read-only decision handle");
        local_write_started_rx
            .await
            .expect("source persistence must be paused before its commit");
        assert_eq!(decision.decision(), StateDecision::Undecided);

        let unresolved_operation = client
            .seed_unresolved_source_decision_for_tests(decision.clone())
            .await
            .expect("seed the actor with the live source decision");
        let error = client
            .reconcile()
            .await
            .expect_err("reconcile cannot settle an undecided source transaction");
        assert!(error.to_string().contains("undecided"));
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .expect("read the retained source attempt")
                .0,
            Some((unresolved_operation.clone(), true)),
            "an undecided source transaction must retain its recovery owner"
        );
        assert_eq!(
            core.active_profiles
                .lock()
                .unwrap()
                .as_ref()
                .map(|profiles| profiles.valid.clone()),
            Some(before.state.valid.clone()),
            "the first reconcile only sees the currently persisted Profiles"
        );

        continue_write_tx
            .send(())
            .expect("release source persistence after checking unresolved recovery");
        assert!(matches!(
            source_writer.await.expect("source persistence task"),
            Ok(ReplaceIfVersionResult::Replaced)
        ));
        assert!(matches!(
            decision.decision(),
            StateDecision::Committed { .. }
        ));
        client
            .reconcile()
            .await
            .expect("reconcile after commit applies and settles the committed Profiles");
        assert_eq!(
            core.active_profiles
                .lock()
                .unwrap()
                .as_ref()
                .map(|profiles| profiles.valid.clone()),
            Some(profile_snapshot.load().state.valid.clone()),
            "the next reconcile applies the now-committed Profiles"
        );
        assert_eq!(
            client
                .application_recovery_state_for_tests()
                .await
                .expect("read the settled source attempt")
                .0,
            None
        );
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn reconcile_preserves_a_profile_source_attempt_after_resource_recovery_failure() {
        let directory = tempfile::tempdir().expect("temporary Profiles directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
            .expect("UTF-8 temporary Profiles path");
        let mut manager = PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .expect("Profiles state manager");
        let before = manager.snapshot_handle().load();
        let version = before.version;
        let mut candidate = before.state.clone();
        candidate
            .valid
            .push("resource-recovery-failure-test".into());
        let core = Arc::new(ProfileTccCore {
            validations: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            fail_next_reconcile: AtomicBool::new(false),
            active_profiles: Mutex::new(None),
        });
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client_with_profiles(
            core.clone(),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
            manager.snapshot_handle(),
        )
        .await;

        let (decision_tx, decision_rx) = oneshot::channel();
        let source_result = manager
            .replace_if_version_with_participant(
                version,
                candidate,
                move |decision| {
                    decision_tx
                        .send(decision.clone())
                        .expect("the test must receive the source decision");
                    Arc::new(AckOnlyProfileChange)
                },
                || async { Err(anyhow::anyhow!("injected Profile persistence failure")) },
                || async {
                    Err(anyhow::anyhow!(
                        "injected Profile resource recovery failure"
                    ))
                },
            )
            .await;
        assert!(source_result.is_err(), "the source transaction must fail");
        let decision = decision_rx
            .await
            .expect("source transaction must publish its decision handle");
        assert!(matches!(
            decision.decision(),
            StateDecision::Aborted {
                resources: chimera_core::state::AbortResourceState::NeedsRecovery(_),
            }
        ));

        let unresolved_operation = client
            .seed_unresolved_source_decision_for_tests(decision)
            .await
            .expect("seed the actor with the source recovery obligation");
        let error = client
            .reconcile()
            .await
            .expect_err("runtime reconcile cannot settle a source resource-recovery failure");
        assert!(error.to_string().contains("resource recovery"));
        let (attempt, _, _) = client
            .application_recovery_state_for_tests()
            .await
            .expect("read the retained source attempt");
        assert_eq!(
            attempt,
            Some((unresolved_operation, true)),
            "a terminal source recovery failure must retain its owner"
        );
        assert_eq!(
            core.active_profiles
                .lock()
                .unwrap()
                .as_ref()
                .map(|profiles| profiles.valid.clone()),
            Some(before.state.valid.clone()),
            "reconcile applies the currently persisted Profiles without clearing the source obligation"
        );
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn startup_recovery_preserves_the_first_startup_report() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovered = Arc::new(Notify::new());
        let service = Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        });
        let client = spawn_startup_client(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered,
                outcome_uncertain: false,
                stop_gate: None,
            }),
            service,
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));

        client
            .recover_startup()
            .await
            .expect("startup recovery should reconcile the current target");
        assert!(events.lock().unwrap().contains(&"rebuild-recovered"));

        let cached = client.startup_reconcile().await.unwrap();
        assert_eq!(cached.report.operation_id, initial.report.operation_id);
        assert_eq!(cached.report.outcome, initial.report.outcome);
    }

    #[tokio::test]
    async fn changed_core_target_invalidates_the_old_startup_retry_timer() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        });
        let client = spawn_startup_client(
            Arc::new(StartupTransientCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stops: AtomicU8::new(0),
                recovered: Arc::new(Notify::new()),
                outcome_uncertain: false,
                stop_gate: None,
            }),
            service,
        )
        .await;

        let initial = client.startup_reconcile().await.unwrap();
        assert!(matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ));
        client
            .select_core(ClashCore::ClashRs)
            .await
            .expect("selecting a new target should reconcile and commit it");
        tokio::time::sleep(Duration::from_millis(1_200)).await;

        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "rebuild-transient-failure")
                .count(),
            1
        );
        assert!(!events.lock().unwrap().contains(&"rebuild-recovered"));
    }

    #[tokio::test]
    async fn startup_reconcile_is_idempotent_and_returns_the_first_report() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

        let first = client.startup_reconcile().await.unwrap();
        let second = client.startup_reconcile().await.unwrap();

        assert_eq!(first.report.operation_id, second.report.operation_id);
        assert_eq!(first.report.outcome, StartupOutcome::Ready);
        assert_eq!(second.report.outcome, StartupOutcome::Ready);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "rebuild")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn startup_lease_failure_caches_an_unsettled_report_and_is_idempotent() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            Arc::new(TransitionUnavailableService {
                inner: RecordingService {
                    events: events.clone(),
                },
            }),
        )
        .await;

        let first = client.startup_reconcile().await.unwrap();
        let second = client.startup_reconcile().await.unwrap();

        assert_eq!(first.report.operation_id, second.report.operation_id);
        assert_eq!(
            first.report.outcome,
            StartupOutcome::Unsettled {
                reason: "could not acquire the Service transition lease: test Service transition lease is unavailable".into(),
            }
        );
        assert_eq!(second.report.outcome, first.report.outcome);
        assert!(first.result.is_err());
        assert!(client.status().uncertain);
        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == "service-begin-failed")
                .count(),
            1,
            "the cached report must prevent another lease acquisition"
        );
        assert!(!events.contains(&"service-probe"));
        assert!(!events.contains(&"rebuild"));
    }

    #[tokio::test]
    async fn startup_reconcile_refuses_local_start_when_service_owner_is_unreadable() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::static_state(
            chimera_config::application::ChimeraAppConfig::default(),
        );
        let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(UnreadableService {
            events: events.clone(),
        });
        let client = spawn_startup_client_with_application(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            service,
            application.clone(),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();

        assert!(completed.result.is_err());
        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                ..
            }
        ));
        assert!(!events.lock().unwrap().contains(&"rebuild"));
        assert!(events.lock().unwrap().contains(&"service-probe-failed"));
        assert!(client.status().uncertain);
        let mutation_error = client
            .select_core(ClashCore::Mihomo)
            .await
            .expect_err("unproven startup owner must block core mutation");
        assert!(
            mutation_error
                .to_string()
                .contains("startup has not established")
        );
        let retry_error = client
            .reconcile()
            .await
            .expect_err("normal reconcile must use a fresh startup owner check");
        assert!(retry_error.to_string().contains("may still hold a Core"));
        assert!(client.status().uncertain);
        assert!(!events.lock().unwrap().contains(&"rebuild"));

        client
            .patch_application_config(ChimeraAppConfigPatch {
                enable_service_mode: Some(true),
                ..Default::default()
            })
            .await
            .expect(
                "source-only Service Mode save must remain available with owner evidence pending",
            );
        assert!(application.get_typed().enable_service_mode);
        assert!(client.status().uncertain);
        assert!(!events.lock().unwrap().contains(&"rebuild"));
    }

    #[tokio::test]
    async fn ready_service_reconcile_failure_retires_owned_core_before_local_fallback() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = true;
        let service = Arc::new(StartupFallbackServiceProbe {
            inner: StartupServiceProbe {
                events: events.clone(),
                probes: AtomicU8::new(0),
                confirms_stopped: false,
                runtime_owned: true,
            },
        });
        let client = spawn_startup_client_with_application(
            Arc::new(StartupDegradedServiceCore {
                events: events.clone(),
                reconciles: AtomicU8::new(0),
                stopped: AtomicBool::new(false),
                selected: Mutex::new(RunType::Service),
            }),
            service,
            ApplicationClient::static_state(app),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        let events = events.lock().unwrap();
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        assert_eq!(
            completed.report.observation.as_ref().unwrap().desired,
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local
        );
        let failed_reconcile = events
            .iter()
            .position(|event| *event == "rebuild-service-failed")
            .unwrap();
        let retire = events
            .iter()
            .position(|event| *event == "service-core-retire")
            .unwrap();
        let fallback = events
            .iter()
            .position(|event| *event == "rebuild-after-service-release")
            .unwrap();
        let probes = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| (*event == "service-probe").then_some(index))
            .collect::<Vec<_>>();
        assert!(probes.len() >= 3);
        assert!(probes[0] < retire);
        assert!(retire < probes[1]);
        assert!(probes[1] < failed_reconcile);
        assert!(failed_reconcile < probes[2]);
        assert!(probes[2] < fallback);
        assert!(!client.status().uncertain);
    }

    #[tokio::test]
    async fn startup_adoption_falls_back_when_service_reconcile_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = true;
        let service = Arc::new(StartupFallbackServiceProbe {
            inner: StartupServiceProbe {
                events: events.clone(),
                probes: AtomicU8::new(0),
                confirms_stopped: false,
                runtime_owned: true,
            },
        });
        let core = Arc::new(StartupDegradedServiceCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stopped: AtomicBool::new(true),
            selected: Mutex::new(RunType::Normal),
        });
        let client = spawn_startup_client_with_application(
            core.clone(),
            service,
            ApplicationClient::static_state(app),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        assert_eq!(
            completed.report.observation.as_ref().unwrap().desired,
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local
        );
        assert_eq!(core.status().await.unwrap().run_type, RunType::Normal);

        let events = events.lock().unwrap();
        let select_service = events
            .iter()
            .position(|event| *event == "select-service")
            .unwrap();
        let failed_reconcile = events
            .iter()
            .position(|event| *event == "rebuild-service-failed")
            .unwrap();
        let fallback = events
            .iter()
            .position(|event| *event == "rebuild-after-service-release")
            .unwrap();
        assert!(select_service < failed_reconcile);
        assert!(failed_reconcile < fallback);
    }

    #[tokio::test]
    async fn startup_retires_service_core_and_confirms_stop_before_local_reconcile() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(RetiringCore {
                inner: RecordingCore {
                    events: events.clone(),
                    stop_started: Arc::new(Notify::new()),
                    release_stop: Arc::new(Notify::new()),
                    recovery_notify: Arc::new(Notify::new()),
                    run_type: RunType::Normal,
                },
                selected: Mutex::new(RunType::Normal),
            }),
            Arc::new(StartupServiceProbe {
                events: events.clone(),
                probes: AtomicU8::new(0),
                confirms_stopped: true,
                runtime_owned: true,
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        let events = events.lock().unwrap();

        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        let retire = events
            .iter()
            .position(|event| *event == "service-core-retire")
            .unwrap();
        let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
        assert!(retire < rebuild);
        // Service mutations also publish a completion-time status refresh, so
        // assert the proof/retire/confirm/rebuild order instead of a fixed
        // total probe count.
        let service_probes = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| (*event == "service-probe").then_some(index))
            .collect::<Vec<_>>();
        assert!(service_probes.iter().any(|index| *index < retire));
        assert!(
            service_probes
                .iter()
                .any(|index| *index > retire && *index < rebuild)
        );
    }

    #[tokio::test]
    async fn startup_hands_back_foreign_service_core_without_stopping_it() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(RetiringCore {
                inner: RecordingCore {
                    events: events.clone(),
                    stop_started: Arc::new(Notify::new()),
                    release_stop: Arc::new(Notify::new()),
                    recovery_notify: Arc::new(Notify::new()),
                    run_type: RunType::Service,
                },
                selected: Mutex::new(RunType::Service),
            }),
            Arc::new(StartupServiceProbe {
                events: events.clone(),
                probes: AtomicU8::new(0),
                confirms_stopped: false,
                runtime_owned: false,
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        let events = events.lock().unwrap();

        assert_eq!(completed.report.outcome, StartupOutcome::Ready);
        let select_local = events
            .iter()
            .position(|event| *event == "select-local")
            .unwrap();
        let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
        assert!(select_local < rebuild);
        assert!(events.contains(&"rebuild"));
        assert!(!events.contains(&"rebuild-after-service-release"));
        assert!(!events.contains(&"service-core-retire"));
    }

    #[tokio::test]
    async fn startup_does_not_mutate_or_handoff_unreadable_service_owner() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(RetiringCore {
                inner: RecordingCore {
                    events: events.clone(),
                    stop_started: Arc::new(Notify::new()),
                    release_stop: Arc::new(Notify::new()),
                    recovery_notify: Arc::new(Notify::new()),
                    run_type: RunType::Service,
                },
                selected: Mutex::new(RunType::Service),
            }),
            Arc::new(UnreadableService {
                events: events.clone(),
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();
        let events = events.lock().unwrap();

        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::RecoveryRequired { .. }
        ));
        assert!(!events.contains(&"service-core-retire"));
        assert!(!events.contains(&"rebuild"));
        assert!(!events.contains(&"rebuild-after-service-release"));
    }

    #[tokio::test]
    async fn startup_holds_service_transition_before_entering_lifecycle_actor() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let service = Arc::new(StartupTransitionRaceService {
            inner: StartupServiceProbe {
                events: events.clone(),
                probes: AtomicU8::new(0),
                confirms_stopped: true,
                runtime_owned: true,
            },
            transition_lock: Arc::new(tokio::sync::Mutex::new(())),
            transition_attempts: Arc::new(AtomicUsize::new(0)),
            probe_entered: Arc::new(Notify::new()),
            release_probe: Arc::new(Notify::new()),
        });
        let client = spawn_startup_client(
            Arc::new(RetiringCore {
                inner: RecordingCore {
                    events: events.clone(),
                    stop_started: Arc::new(Notify::new()),
                    release_stop: Arc::new(Notify::new()),
                    recovery_notify: Arc::new(Notify::new()),
                    run_type: RunType::Normal,
                },
                selected: Mutex::new(RunType::Normal),
            }),
            service.clone(),
        )
        .await;

        let startup_client = client.clone();
        let startup = tokio::spawn(async move { startup_client.startup_reconcile().await });
        tokio::time::timeout(Duration::from_secs(1), service.probe_entered.notified())
            .await
            .expect("startup should reach its Service probe while holding the lease");

        let service_client = client.clone();
        let service_command =
            tokio::spawn(async move { service_client.start_service(Duration::from_secs(1)).await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while service.transition_attempts.load(AtomicOrdering::Acquire) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the queued Service command should contend for the transition lease");

        service.release_probe.notify_one();
        let startup = tokio::time::timeout(Duration::from_secs(2), startup)
            .await
            .expect("startup and the queued Service command must not deadlock")
            .expect("startup task should not panic")
            .expect("startup should reach a terminal report");
        assert_eq!(startup.report.outcome, StartupOutcome::Ready);
        tokio::time::timeout(Duration::from_secs(2), service_command)
            .await
            .expect("queued Service command should run after startup releases the lease")
            .expect("Service task should not panic")
            .expect("Service command should succeed");

        let events = events.lock().unwrap();
        let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
        let service_start = events
            .iter()
            .position(|event| *event == "service-start")
            .unwrap();
        assert!(rebuild < service_start);
    }

    #[tokio::test]
    async fn startup_does_not_start_local_when_service_stop_is_unconfirmed() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = spawn_startup_client(
            Arc::new(RetiringCore {
                inner: RecordingCore {
                    events: events.clone(),
                    stop_started: Arc::new(Notify::new()),
                    release_stop: Arc::new(Notify::new()),
                    recovery_notify: Arc::new(Notify::new()),
                    run_type: RunType::Normal,
                },
                selected: Mutex::new(RunType::Normal),
            }),
            Arc::new(StartupServiceProbe {
                events: events.clone(),
                probes: AtomicU8::new(0),
                confirms_stopped: false,
                runtime_owned: true,
            }),
        )
        .await;

        let completed = client.startup_reconcile().await.unwrap();

        assert!(matches!(
            completed.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                ..
            }
        ));
        assert!(client.status().uncertain);
        assert!(events.lock().unwrap().contains(&"service-core-retire"));
        assert!(!events.lock().unwrap().contains(&"rebuild"));
        assert!(
            !events
                .lock()
                .unwrap()
                .contains(&"rebuild-after-service-release")
        );
    }

    #[tokio::test]
    async fn timed_out_startup_reconcile_has_no_terminal_completion() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = spawn_startup_client(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            }),
            Arc::new(RecordingService {
                events: events.clone(),
            }),
        )
        .await;

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

        let eventual = client
            .cached_startup_report()
            .await
            .expect("startup report query should succeed")
            .expect("queued startup should cache a terminal report");
        assert_eq!(eventual.outcome, StartupOutcome::Ready);
        let repeated = client
            .startup_reconcile()
            .await
            .expect("a repeated startup request should return the cached report");
        assert_eq!(repeated.report.operation_id, eventual.operation_id);
        assert_eq!(repeated.report.outcome, eventual.outcome);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == "rebuild")
                .count(),
            1,
            "the eventual report must remain idempotent after the original caller timed out"
        );
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

    async fn run_binary_update(
        desired_core: ClashCore,
        state: CoreState,
        active_kind: Option<chimera_core_manager::CoreKind>,
        target: ClashCore,
        state_changed_at: i64,
        receipt_error: bool,
    ) -> Vec<&'static str> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.core = desired_core;
        let core = Arc::new(CoreUpdateRecordingCore {
            events: events.clone(),
            state,
            state_changed_at,
            run_type: RunType::Normal,
            active_kind,
            receipt_error,
        });
        let client = CoreLifecycleClient::spawn_with_installer(
            core,
            ApplicationClient::static_state(app),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(RecordingInstaller {
                events: events.clone(),
            }),
        )
        .await
        .unwrap();
        let staging = Arc::new(tempfile::tempdir().unwrap());
        client
            .replace_core_binary(PreparedCoreBinary {
                target,
                source: staging.path().join("staged-core"),
                destination: staging.path().join("installed-core"),
                staging,
                progress: Arc::new(RecordingProgress {
                    events: events.clone(),
                }),
            })
            .await
            .unwrap();
        events.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn core_binary_workflow_copies_through_upstream_os_installer_without_elevation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.core = ClashCore::Mihomo;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(CoreUpdateRecordingCore {
                events: events.clone(),
                state: CoreState::Stopped(None),
                state_changed_at: 1,
                run_type: RunType::Normal,
                active_kind: None,
                receipt_error: false,
            }),
            ApplicationClient::static_state(app),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
        )
        .await
        .unwrap();
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("staged-mihomo");
        std::fs::write(&source, b"new-mihomo-binary").unwrap();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination = destination_dir.path().join("mihomo");
        std::fs::write(&destination, b"old-mihomo-binary").unwrap();

        client
            .replace_core_binary(PreparedCoreBinary {
                target: ClashCore::Mihomo,
                source,
                destination: destination.clone(),
                staging,
                progress: Arc::new(RecordingProgress {
                    events: events.clone(),
                }),
            })
            .await
            .unwrap();

        assert_eq!(std::fs::read(destination).unwrap(), b"new-mihomo-binary");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["restarting", "rebuild", "finished"]
        );
    }

    #[tokio::test]
    async fn refused_os_core_copy_keeps_target_and_does_not_restart() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.core = ClashCore::Mihomo;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(CoreUpdateRecordingCore {
                events: events.clone(),
                state: CoreState::Stopped(None),
                state_changed_at: 1,
                run_type: RunType::Normal,
                active_kind: None,
                receipt_error: false,
            }),
            ApplicationClient::static_state(app),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
        )
        .await
        .unwrap();

        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("staged-mihomo");
        std::fs::write(&source, b"new-mihomo-binary").unwrap();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination = destination_dir.path().join("mihomo");
        // A directory is a deterministic copy failure on all supported OSes;
        // no permissions are changed and no real elevation is attempted.
        std::fs::create_dir(&destination).unwrap();
        let marker = destination.join("existing-core-marker");
        std::fs::write(&marker, b"preserved").unwrap();

        let error = client
            .replace_core_binary(PreparedCoreBinary {
                target: ClashCore::Mihomo,
                source: source.clone(),
                destination: destination.clone(),
                staging,
                progress: Arc::new(RecordingProgress {
                    events: events.clone(),
                }),
            })
            .await
            .expect_err("refused elevated fallback must fail the update");

        assert!(
            error.to_string().contains("could not authorize copying"),
            "unexpected failure: {error:#}"
        );
        assert_eq!(std::fs::read(&marker).unwrap(), b"preserved");
        // The staged source is owned by the update request. Once the failed
        // operation completes, its temporary directory is intentionally gone.
        assert!(!source.exists());
        assert!(destination.is_dir());
        assert_eq!(events.lock().unwrap().as_slice(), ["finished-error"]);
    }

    #[tokio::test]
    async fn updating_an_inactive_core_does_not_stop_or_restart_the_confirmed_active_core() {
        let events = run_binary_update(
            ClashCore::Mihomo,
            CoreState::Running,
            Some(chimera_core_manager::CoreKind::Mihomo),
            ClashCore::ClashRs,
            1,
            false,
        )
        .await;

        assert_eq!(events, ["install", "finished"]);
    }

    #[tokio::test]
    async fn updating_the_running_core_stops_before_install_even_when_it_is_not_selected() {
        let events = run_binary_update(
            ClashCore::Mihomo,
            CoreState::Running,
            Some(chimera_core_manager::CoreKind::ClashRust),
            ClashCore::ClashRs,
            1,
            false,
        )
        .await;

        assert_eq!(
            events,
            ["stop", "install", "restarting", "rebuild", "finished"]
        );
    }

    #[tokio::test]
    async fn an_unidentified_running_core_is_stopped_before_binary_replacement() {
        let events = run_binary_update(
            ClashCore::Mihomo,
            CoreState::Running,
            None,
            ClashCore::ClashRs,
            1,
            false,
        )
        .await;

        assert_eq!(
            events,
            ["stop", "install", "restarting", "rebuild", "finished"]
        );
    }

    #[tokio::test]
    async fn unavailable_service_status_is_not_treated_as_proven_stopped() {
        let events = run_binary_update(
            ClashCore::Mihomo,
            CoreState::Stopped(None),
            None,
            ClashCore::ClashRs,
            0,
            true,
        )
        .await;

        assert_eq!(
            events,
            ["stop", "install", "restarting", "rebuild", "finished"]
        );
    }

    #[tokio::test]
    async fn binary_replacement_of_a_stopped_selected_core_installs_then_restarts() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = application.get_typed().core;
        let runtime_paths =
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root"));
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(ConfirmedStoppedCore {
                events: events.clone(),
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
            ["install", "restarting", "rebuild", "finished"]
        );
    }

    #[tokio::test]
    async fn binary_install_failure_does_not_stop_or_restart_a_stopped_core() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = application.get_typed().core;
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(ConfirmedStoppedCore {
                events: events.clone(),
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
            ["install", "finished-error"]
        );
    }
}

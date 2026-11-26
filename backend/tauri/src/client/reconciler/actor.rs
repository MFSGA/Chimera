//! Runtime application and lifecycle operations share one actor mailbox.
//! The desktop compatibility client is re-exported by core_lifecycle.
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

#[cfg(test)]
use crate::client::reconciler::apply::checked_mode_effect_can_finish;
#[cfg(test)]
use crate::client::reconciler::checked::{
    CheckedRuntimeCandidate, CheckedRuntimeInbox, CheckedSelection, MAX_CHECKED_PENDING,
};
#[cfg(test)]
use crate::client::reconciler::committed::SourceVersions;
use crate::client::reconciler::{
    committed::SourceDomain, status::ReconcilerStatus, workflow::RuntimeReconcilerState,
};
#[cfg(test)]
use chimera_core::client::reconciler::{
    completion::refresh_effect_ports,
    worker::{WorkerResult, run_lifecycle},
};
use chimera_core::client::reconciler::{
    completion::{
        HandoffContinuation, RebuildAction, checked_clash_notice, rebuild_after_completion,
    },
    dispatch::{RequestAdmission, RoundAdmission, request_kind},
    plan::RunIntent,
    source_dispatch::{SourceAction, SourceDispatch, SourceDispatchFacts},
    worker::{
        WorkerCompletion, WorkerEvidence, WorkerWorkflow, schedule_startup_recovery,
        spawn_service_recovery_lease, spawn_startup_retry_lease,
    },
};
use ractor::{Actor, ActorProcessingErr, ActorRef};
#[cfg(test)]
use ractor::{RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[cfg(test)]
use crate::client::application_workflow::{
    startup::{StartupOutcome, StartupWorkflowState},
    workflow::ProfileRuntime,
};
#[cfg(test)]
use crate::client::core_lifecycle::{
    BinaryInstallProgress, LegacyServiceBridge, ServiceTransitionLease,
};
use crate::client::core_lifecycle::{
    Command, CoreLifecyclePort, CoreLifecycleWorkflow, CoreStatusSnapshot, PreparedCoreBinary,
    RuntimeTransformDiagnostics, ServiceLifecyclePort, ports, workflow,
};
use crate::client::{
    ChimeraClient, application::ApplicationClient, application_workflow::startup::StartupReport,
    application_workflow::workflow::ApplicationWorkflow, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};

#[cfg(test)]
use chimera_core::client::reconciler::{
    dispatch::is_source_only_application_patch,
    mailbox::{MAX_PENDING, OperationId, Response},
    service_evidence::{service_followup_evidence_changed, startup_recovery_evidence_changed},
};

use chimera_core::client::core_lifecycle::RECOVERY_INTERVAL;
use chimera_core::client::reconciler::worker::CALL_WAIT;

use chimera_core::client::reconciler::mailbox::{
    AdmissionFailure, DIRTY_WINDOW, DirtyTick, Mailbox, MailboxTimers,
};

pub use chimera_core::client::app_lifecycle::CompletedLifecycleOperation;

fn completed_from_startup_report(report: StartupReport) -> CompletedLifecycleOperation {
    let result = report
        .outcome
        .failure_reason()
        .map_or(Ok(()), |reason| Err(anyhow::anyhow!(reason.to_owned())));
    CompletedLifecycleOperation { result, report }
}

#[async_trait::async_trait]
impl WorkerWorkflow<Command> for ApplicationWorkflow {
    async fn execute_lifecycle_command(&mut self, command: Command) -> anyhow::Result<bool> {
        self.execute_lifecycle(command).await
    }

    async fn recover_worker_panic(&mut self, message: String) {
        self.mark_recovery_required(message).await;
    }

    fn observe_worker(&self) -> WorkerEvidence {
        WorkerEvidence {
            lower_outcome_uncertain: self.lifecycle().outcome_uncertain(),
            startup_owner_unproven: self.startup_owner_unproven(),
            service_restart_policy: self.lifecycle().service_restart_policy(),
        }
    }

    async fn probe_worker_service(
        &self,
    ) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.lifecycle().probe_service().await
    }

    #[cfg(test)]
    async fn run_command(&mut self, command: Command) -> WorkerResult {
        match command {
            Command::ProfileMutation(mut mutation) => {
                // Ref's recoverable panic flag must cover each future
                // poll, not remain set across a Tokio .await. Preserve
                // the uncertainty/recovery settlement on panic.
                let run = chimera_panics::catch_recoverable_future(
                    self.run_profile_mutation(&mut mutation),
                )
                .await;
                let (result, panicked, rebuild) = match run {
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
                        self.mark_recovery_required(message.clone()).await;
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
                        (Err::<(), _>(anyhow::anyhow!(message)), true, false)
                    }
                };
                WorkerResult {
                    result,
                    workflow_panicked: panicked,
                    request_runtime_rebuild: rebuild,
                }
            }
            command => run_lifecycle(self, command).await,
        }
    }
}

pub use chimera_core::client::reconciler::mailbox::{
    CoreLifecycleOperationResult, CoreLifecycleStatus,
};

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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
type ProxyHandle = crate::client::system_proxy::SystemProxyClient;
#[cfg(any(target_os = "android", target_os = "ios"))]
type ProxyHandle = ();
type Message = chimera_core::client::reconciler::client::Message<ApplicationWorkflow, ProxyHandle>;
type ActorClient =
    chimera_core::client::reconciler::client::ActorClient<ApplicationWorkflow, ProxyHandle>;

struct RuntimeReconcilerActor;

struct RuntimeReconcilerMailbox {
    mailbox: Mailbox<Command>,
    timers: MailboxTimers<Message>,
    reconciler: tokio::sync::watch::Sender<ReconcilerStatus>,
    workflow: Option<ApplicationWorkflow>,
    #[cfg(test)]
    pending_mutation_connection: Option<(ApplicationWorkflow, RpcReplyPort<anyhow::Result<()>>)>,
    pending_startup_retry_lease_failures: VecDeque<(String, String)>,
    #[cfg(test)]
    startup_retry_lease_failure_observer: Option<tokio::sync::oneshot::Sender<()>>,
    service_status: ServiceHostWatch,
    service: Arc<dyn ServiceLifecyclePort>,
    runtime: RuntimeReconcilerState,
    source_authority: Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    service_recovery_inflight: bool,
    startup_recovery_pending: Arc<AtomicBool>,
}

fn record_startup_retry_lease_failure(
    workflow: Option<&mut ApplicationWorkflow>,
    pending: &mut VecDeque<(String, String)>,
    identity: String,
    reason: String,
) {
    chimera_core::client::reconciler::dispatch::record_startup_retry_lease_failure(
        workflow.map(ApplicationWorkflow::progress_mut),
        pending,
        identity,
        reason,
    );
}

fn apply_pending_startup_retry_lease_failures(
    workflow: Option<&mut ApplicationWorkflow>,
    pending: &mut VecDeque<(String, String)>,
) {
    chimera_core::client::reconciler::dispatch::apply_pending_startup_retry_lease_failures(
        workflow.map(ApplicationWorkflow::progress_mut),
        pending,
    );
}

struct RuntimeReconcilerArgs {
    reconciler: tokio::sync::watch::Sender<ReconcilerStatus>,
    workflow: ApplicationWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    service_status: ServiceHostWatch,
    service: Arc<dyn ServiceLifecyclePort>,
    source_authority: Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    startup_owner_unproven_shared: Arc<AtomicBool>,
    startup_recovery_pending: Arc<AtomicBool>,
}

type Request = chimera_core::client::reconciler::mailbox::Request<Command>;

impl RuntimeReconcilerMailbox {
    fn source_dispatch_facts(&self) -> SourceDispatchFacts {
        SourceDispatchFacts {
            shutting_down: self.mailbox.shutting_down,
            uncertain: self.mailbox.uncertain,
            startup_owner_unproven: self.mailbox.startup_owner_unproven
                || self
                    .mailbox
                    .startup_owner_unproven_shared
                    .load(Ordering::Acquire),
            owner_transition_pending: self.owner_transition_pending(),
            queue_full: self.mailbox.queue_full(),
            authority_available: self.source_authority.is_some(),
        }
    }

    fn apply_source_dispatch(
        &mut self,
        myself: &ActorRef<Message>,
        domain: SourceDomain,
        version: u64,
        dispatch: SourceDispatch,
    ) {
        if dispatch.observe_source_only {
            self.reconciler
                .send_modify(|status| status.observe_source_only(domain, version));
        }
        match dispatch.action {
            SourceAction::None => {}
            SourceAction::MarkDirty => self.mark_runtime_dirty(myself),
            SourceAction::RecoverStartup => self.request_startup_recovery(myself),
            SourceAction::ReplanAfterHandoff => {
                self.replan_retained_source_on_service_change(myself)
            }
            SourceAction::ApplyChecked(checked) => {
                // Source dispatch and enqueue are synchronous in the same
                // mailbox turn: the authority cannot disappear between them.
                let authority = self
                    .source_authority
                    .as_ref()
                    .expect("dispatch requires source authority")
                    .clone();
                self.mailbox
                    .enqueue_checked_source(checked, domain, version, authority, None);
            }
        }
        if let Some(degradation) = dispatch.degradation {
            self.reconciler
                .send_modify(|status| status.record_system_degradation(degradation));
            self.publish_maintenance();
        }
    }

    /// Ref ServiceActor is the sole production publisher of daemon phase,
    /// retry attempts, and readiness generation. The legacy watch is updated
    /// only for injected compatibility ports that have no actor owner.
    fn publishes_legacy_service_status(&self) -> bool {
        self.service.actor_status().is_none()
    }

    fn publish_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        if self.publishes_legacy_service_status() {
            let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
            self.service_status.observe_probe(info, runtime_owned, None);
        }
    }

    fn publish_service_status_with_policy(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
        policy: ServiceRestartPolicySnapshot,
    ) {
        if self.publishes_legacy_service_status() {
            let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
            self.service_status
                .observe_probe(info, runtime_owned, Some(policy));
        }
    }

    fn publish_service_probe_failure(&self) {
        if self.publishes_legacy_service_status() {
            self.service_status.probe_failed();
        }
    }

    #[cfg(test)]
    async fn refresh_service_status(&mut self) -> anyhow::Result<ServiceHostStatus> {
        if let Some(actor) = self.service.actor_status() {
            return Ok(actor);
        }
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
        if self
            .runtime
            .request_dirty_tick(self.mailbox.uncertain, self.mailbox.shutting_down)
        {
            self.timers
                .after(myself, DIRTY_WINDOW, || Message::DirtyTick);
        }
    }

    fn schedule_deferred_retry(
        &mut self,
        myself: &ActorRef<Message>,
        digest: String,
        next_attempt: tokio::time::Instant,
    ) {
        self.timers
            .at(myself, next_attempt, move || Message::DeferredRetry {
                digest,
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
        &mut self,
        myself: &ActorRef<Message>,
        identity: String,
        next_attempt: tokio::time::Instant,
    ) {
        self.timers
            .at(myself, next_attempt, move || Message::StartupRetryDue {
                identity,
            });
    }

    fn request_startup_recovery(&self, myself: &ActorRef<Message>) {
        let startup_owner_unproven = self.mailbox.startup_owner_unproven_shared.clone();
        schedule_startup_recovery(
            myself.clone(),
            self.mailbox.next_id.clone(),
            self.service.clone(),
            self.startup_recovery_pending.clone(),
            move || startup_owner_unproven.load(Ordering::Acquire),
            |operation_id, transition| Command::RecoverStartup {
                operation_id,
                transition,
            },
            Message::Request,
            |succeeded| Message::StartupRecoveryFinished { succeeded },
        );
    }

    /// A handoff failure and a degraded Service endpoint are independent
    /// maintenance debts; keep both even when a Core round converged.
    fn publish_maintenance(&self) {
        chimera_core::client::reconciler::dispatch::publish_maintenance(
            &self.runtime,
            &self.reconciler,
        );
    }

    /// Shared S2 admission barrier. A Startup recovery worker may hold the
    /// Service transition lease *outside* the CoreLifecycle mailbox; startup
    /// and follow-up owner commands may also be queued behind another action.
    /// Neither a readiness event nor a source notification can infer an
    /// established owner before those transitions have settled.
    fn owner_transition_pending(&self) -> bool {
        self.mailbox
            .owner_transition_pending(&self.startup_recovery_pending)
    }

    /// Evaluate release debt after a proven runtime round or a Service
    /// observation. This never starts a second Core controller: the actual
    /// daemon stop is admitted as its own serialized lifecycle command.
    async fn schedule_service_release(&mut self, explicit_retry: bool) {
        if !self.mailbox.release_allowed(
            &self.runtime,
            &self.startup_recovery_pending,
            &self.reconciler.borrow().conclusion,
        ) {
            return;
        }
        let Some(workflow) = self.workflow.as_ref() else {
            return;
        };
        let observation = workflow.lifecycle().service_release_observation().await;
        self.mailbox
            .observe_release(&mut self.runtime, observation, explicit_retry);
        self.publish_maintenance();
    }

    /// A Service readiness update can itself resolve a previously blocked
    /// S2 owner step (for example, the other daemon stopped externally). If
    /// no leased handoff is queued, ask the *existing* source planner to check
    /// the latest committed target again. Keep the continuation latched until
    /// a verified round succeeds. Unreadable or still-owned Service evidence
    /// is rejected by S2; this is not permission to start Core.
    fn replan_retained_source_on_service_change(&mut self, myself: &ActorRef<Message>) {
        if !self.runtime.source_after_handoff
            || !self
                .mailbox
                .owner_followup_allowed(&self.runtime, &self.startup_recovery_pending)
        {
            return;
        }
        self.runtime.source_dirty = true;
        self.mark_runtime_dirty(myself);
    }

    /// DirtyTick may have fired while an independently acquired Startup
    /// transition lease was still outstanding. Keep its latest-target intent
    /// rather than dropping it or submitting Core early; a successful owner
    /// recovery or later independent Service evidence rearms one coalesced tick.
    fn rearm_deferred_dirty_after_owner(&mut self, myself: &ActorRef<Message>) {
        if self.runtime.dirty
            && self
                .mailbox
                .owner_followup_allowed(&self.runtime, &self.startup_recovery_pending)
        {
            self.mark_runtime_dirty(myself);
        }
    }

    async fn follow_service(&mut self) {
        if chimera_core::client::reconciler::dispatch::follow_service(
            &mut self.mailbox,
            &mut self.runtime,
            &self.startup_recovery_pending,
            self.workflow
                .as_ref()
                .map(|workflow| (workflow.progress(), workflow.lifecycle())),
            self.service.as_ref(),
        )
        .await
        {
            self.publish_maintenance();
        }
    }

    fn reject(&mut self, request: Request, error: anyhow::Error) {
        #[cfg(test)]
        let mut request = request;
        if matches!(&request.command, Command::ServiceEndpointDown(_)) {
            self.service_recovery_inflight = false;
        }
        #[cfg(test)]
        if let Command::ProfileMutation(mutation) = &mut request.command {
            let message = error.to_string();
            mutation.answer(
                crate::client::application_workflow::mutation::TryAck::Failed(message.clone()),
            );
            mutation.settle(Err(message));
        }
        if let Command::ReplaceCoreBinary(artifact) = &request.command {
            let message = error.to_string();
            chimera_core::client::reconciler::worker::finish_binary_progress(
                artifact.progress.as_ref(),
                Some(&message),
            );
        }
        self.mailbox.settle(request.response, Err(error));
    }

    fn reject_pending(&mut self, myself: &ActorRef<Message>) {
        if self.mailbox.uncertain && !self.mailbox.shutting_down {
            self.runtime.dirty = false;
        }
        let workflow = self.workflow.as_ref();
        let refused = self.mailbox.drain_refused(request_kind, |command| {
            workflow.is_none_or(|workflow| {
                !workflow.has_unresolved_attempt()
                    || workflow.recovery_command_allowed(command)
                    || matches!(command, Command::Shutdown)
            })
        });
        for (request, reason) in refused {
            if matches!(
                reason,
                AdmissionFailure::StartupUnproven | AdmissionFailure::ApplicationRecovery
            ) {
                if let Some(digest) = request.deferred_retry.as_deref() {
                    self.reschedule_deferred_retry(myself, digest, Duration::from_secs(5));
                }
            }
            self.reject(request, anyhow::anyhow!(reason.message()));
        }
    }

    fn close(&mut self) {
        self.timers.cancel();
        self.runtime.checked.clear();
        self.runtime.clear_dirty();
        for request in self.mailbox.close() {
            self.reject(
                request,
                anyhow::anyhow!(AdmissionFailure::Closing.message()),
            );
        }
        self.mailbox.publish();
    }

    fn push_background(&mut self, command: Command) {
        self.mailbox.push_background(command);
    }

    fn drive(&mut self, myself: &ActorRef<Message>) {
        if self.mailbox.active.is_some() {
            self.mailbox.publish();
            return;
        }
        self.reject_pending(myself);
        let Some(request) = self.mailbox.take_next(|| Command::Shutdown) else {
            self.mailbox.publish();
            return;
        };

        let request = match self.mailbox.admit_round(&mut self.runtime, request) {
            RoundAdmission::Run(request) => request,
            RoundAdmission::Rejected(request, error) => {
                self.reject(request, error);
                self.drive(myself);
                return;
            }
            RoundAdmission::Skipped => {
                self.drive(myself);
                return;
            }
        };

        let Some(mut workflow) = self.workflow.take() else {
            tracing::error!("core lifecycle workflow missing without an active operation");
            self.reject(
                request,
                anyhow::anyhow!("core lifecycle workflow unavailable"),
            );
            self.mailbox.publish();
            return;
        };

        #[cfg(test)]
        let profile_mutation = matches!(&request.command, Command::ProfileMutation(_));
        #[cfg(test)]
        if profile_mutation && !workflow.mutation_connected() {
            self.workflow = Some(workflow);
            self.reject(
                request,
                anyhow::anyhow!("application workflow is not connected"),
            );
            self.mailbox.publish();
            return;
        }

        let publish_legacy_service = self.publishes_legacy_service_status();
        let service_status = &self.service_status;
        self.mailbox.launch_round(
            myself,
            workflow,
            request,
            &self.runtime,
            &self.reconciler,
            publish_legacy_service,
            |phase| {
                if publish_legacy_service {
                    service_status.publish_phase(phase);
                }
            },
            |completion| Message::Completed { completion },
        );
    }
}

// The Windows desktop regression needs to distinguish a slow Core operation
// from a synchronous Actor mailbox handler blocking a Checked ACK. This probe
// is compiled only into the isolated E2E executable; it never changes admission.
#[cfg(feature = "e2e")]
struct E2eCoreMailboxProbe {
    kind: &'static str,
    started: std::time::Instant,
}

#[cfg(feature = "e2e")]
impl E2eCoreMailboxProbe {
    fn begin(kind: &'static str) -> Self {
        if matches!(
            kind,
            "checked" | "completed" | "service_readiness" | "service_recovery_tick"
        ) {
            tracing::warn!(target: "chimera::e2e_core_mailbox", kind, stage = "entered", "CoreLifecycle mailbox stage");
        }
        Self {
            kind,
            started: std::time::Instant::now(),
        }
    }
}

#[cfg(feature = "e2e")]
impl Drop for E2eCoreMailboxProbe {
    fn drop(&mut self) {
        let elapsed_ms = self.started.elapsed().as_millis();
        if elapsed_ms >= 100 || matches!(self.kind, "checked" | "completed") {
            tracing::warn!(target: "chimera::e2e_core_mailbox", kind = self.kind, stage = "finished", elapsed_ms = elapsed_ms as u64, "CoreLifecycle mailbox stage");
        }
    }
}

impl Actor for RuntimeReconcilerActor {
    type Msg = Message;
    type State = RuntimeReconcilerMailbox;
    type Arguments = RuntimeReconcilerArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(RuntimeReconcilerMailbox {
            mailbox: Mailbox::new(
                args.next_id,
                args.status,
                args.startup_owner_unproven_shared,
            ),
            timers: MailboxTimers::new(&myself, RECOVERY_INTERVAL, || Message::ServiceRecoveryTick),
            reconciler: args.reconciler,
            workflow: Some(args.workflow),
            #[cfg(test)]
            pending_mutation_connection: None,
            pending_startup_retry_lease_failures: VecDeque::new(),
            #[cfg(test)]
            startup_retry_lease_failure_observer: None,
            service_status: args.service_status,
            service: args.service,
            runtime: RuntimeReconcilerState::default(),
            source_authority: args.source_authority,
            service_recovery_inflight: false,
            startup_recovery_pending: args.startup_recovery_pending,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        #[cfg(feature = "e2e")]
        let _mailbox_probe = E2eCoreMailboxProbe::begin(match &message {
            Message::Checked { .. } => "checked",
            Message::Completed { .. } => "completed",
            Message::ObserveService { .. } => "observe_service",
            Message::ServiceReadiness => "service_readiness",
            Message::ServiceProbeFailed => "service_probe_failed",
            Message::ServiceRecoveryTick => "service_recovery_tick",
            Message::Request(_) => "request",
            _ => "other",
        });
        match message {
            Message::AbortChecked { domain, request } => {
                let count = state
                    .runtime
                    .abort_checked_candidate(domain, request.as_deref());
                tracing::debug!(
                    ?domain,
                    ?request,
                    count,
                    "discarded uncommitted checked candidates"
                );
            }
            Message::InstallCandidateGate { gate, reply } => {
                let result = if state.mailbox.shutting_down {
                    Err(anyhow::anyhow!("core lifecycle is shutting down"))
                } else {
                    state
                        .workflow
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("core lifecycle workflow is busy"))
                        .and_then(|workflow| workflow.lifecycle().install_checked_gate(gate))
                };
                let _ = reply.send(result);
            }
            Message::Checked { checked, reply } => {
                // Unlike a bare cast, this acknowledgement proves that the
                // owner actually retained the exact document and digest.
                // Do not let a closed or uncertain owner authorize a commit.
                let facts = state.source_dispatch_facts();
                let result = state.runtime.admit_checked_candidate(checked, facts);
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Message::ConnectApplicationWorkflow {
                workflow: incoming,
                reply,
                registered,
            } => {
                let validation = if state.mailbox.shutting_down {
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
            Message::Request(request) => {
                if matches!(&request.command, Command::ServiceEndpointDown(_)) {
                    state.service_recovery_inflight = true;
                }
                let service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
                match state.mailbox.receive_request(request, service_connected) {
                    RequestAdmission::Accepted => {}
                    RequestAdmission::Close => state.close(),
                    RequestAdmission::Reject(request, reason) => {
                        state.reject(request, anyhow::anyhow!(reason.message()));
                    }
                }
            }

            Message::Completed { completion } => {
                let WorkerCompletion {
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
                    service_endpoint_recovery,
                    shutdown,
                    request_runtime_rebuild,
                    deferred_retry,
                    reconciled,
                } = completion;
                if state
                    .mailbox
                    .active
                    .as_ref()
                    .is_some_and(|active| active.response.id == id)
                {
                    let lifecycle = workflow.lifecycle();
                    let chimera_core::client::reconciler::completion::CompletedRound {
                        active,
                        result,
                        source_advanced,
                        round,
                        runtime_binding,
                    } = chimera_core::client::reconciler::completion::observe_completed(
                        &mut state.mailbox,
                        &state.runtime,
                        lifecycle,
                        id,
                        result,
                        workflow_panicked,
                        lower_outcome_uncertain,
                        reconciled,
                        service_probe.is_some(),
                        shutdown,
                        || lifecycle.take_prepared_outcome(),
                        || lifecycle.take_checked_connection_effect(),
                        || lifecycle.effect_port_binding(),
                    )
                    .await
                    .expect("matched active lifecycle operation");
                    let verified_running = round.verified_running;
                    state.runtime.observe_round_connection(&round);
                    // Legacy SetConfig UI messages describe the outcome of
                    // Core refresh, not source persistence. Checked Clash
                    // follows the same semantics once the Actor settles it.
                    if let Some(notice) =
                        checked_clash_notice(active.checked_clash, &result, verified_running)
                    {
                        #[cfg(not(test))]
                        crate::core::handle::Handle::notice_message(
                            &crate::core::handle::Message::SetConfig(notice),
                        );
                        #[cfg(test)]
                        let _ = notice;
                    }
                    // This belongs to the existing LocalRuntimeHost. The
                    // status projection only records outstanding publication
                    // debt and cannot mutate or duplicate the runtime product.
                    let pending_product = workflow.lifecycle().pending_runtime_product_target();
                    state.reconciler.send_modify(|status| {
                        status.observe_product_publication(pending_product.as_deref());
                    });
                    state.publish_maintenance();
                    state.workflow = Some(workflow);
                    if service_endpoint_recovery {
                        state.service_recovery_inflight = false;
                    }
                    if state.runtime.observe_host_completion(
                        active.trigger,
                        service_endpoint_recovery,
                        &result,
                    ) {
                        state.publish_maintenance();
                    }
                    state.mailbox.set_owner_unproven(startup_owner_unproven);
                    #[cfg(test)]
                    if let Some((incoming, reply)) = state.pending_mutation_connection.take() {
                        let validation = if state.mailbox.shutting_down {
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
                    state.mailbox.uncertain |= workflow_panicked || lower_outcome_uncertain;

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

                    let (failed, retry) =
                        chimera_core::client::reconciler::completion::settle_completed(
                            &mut state.mailbox,
                            &mut state.runtime,
                            state
                                .workflow
                                .as_ref()
                                .expect("completed workflow was restored")
                                .lifecycle()
                                .convergence_progress(),
                            &state.reconciler,
                            round,
                            active.response,
                            active.trigger,
                            &active.observed_sources,
                            reconciled,
                            shutdown,
                            result,
                        );
                    if let Some((digest, next_attempt)) =
                        chimera_core::client::reconciler::completion::settle_deferred_retry(
                            state
                                .workflow
                                .as_mut()
                                .map(ApplicationWorkflow::progress_mut),
                            deferred_retry,
                            reconciled,
                            failed,
                            state.mailbox.uncertain,
                            retry,
                        )
                        .await
                    {
                        state.schedule_deferred_retry(&myself, digest, next_attempt);
                    }

                    #[cfg(feature = "e2e")]
                    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_retry_settled", "CoreLifecycle completion stage");
                    if state.runtime.retry_completed_source(
                        retry_reconcile_on_failure,
                        failed,
                        source_advanced,
                        state.mailbox.uncertain,
                        state.mailbox.shutting_down,
                    ) {
                        state.mark_runtime_dirty(&myself);
                    }
                    match rebuild_after_completion(
                        request_runtime_rebuild,
                        state.mailbox.shutting_down,
                        state.mailbox.startup_owner_unproven
                            || state
                                .mailbox
                                .startup_owner_unproven_shared
                                .load(Ordering::Acquire),
                        state.mailbox.uncertain,
                    ) {
                        RebuildAction::None => {}
                        RebuildAction::RecoverStartup => state.request_startup_recovery(&myself),
                        RebuildAction::ScheduleRuntime => {
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
                    }
                    if recover && failed && !state.mailbox.uncertain && !state.mailbox.shutting_down
                    {
                        tracing::error!("failed to recover core; scheduling retry");
                        state
                            .timers
                            .after(&myself, Duration::from_secs(5), || Message::RecoverCore);
                    }
                    state.follow_service().await;
                    state.rearm_deferred_dirty_after_owner(&myself);
                    match state.runtime.complete_handoff(
                        active.trigger,
                        reconciled,
                        failed,
                        verified_running,
                        state.mailbox.uncertain,
                        state.mailbox.startup_owner_unproven
                            || state
                                .mailbox
                                .startup_owner_unproven_shared
                                .load(Ordering::Acquire),
                        state.mailbox.shutting_down,
                    ) {
                        HandoffContinuation::None => {}
                        HandoffContinuation::ClearMaintenance => state.publish_maintenance(),
                        HandoffContinuation::MarkDirty => state.mark_runtime_dirty(&myself),
                    }
                    state
                        .schedule_service_release(active.trigger == "Retry")
                        .await;
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
                state.follow_service().await;
                state.replan_retained_source_on_service_change(&myself);
                state.rearm_deferred_dirty_after_owner(&myself);
                state.schedule_service_release(false).await;
            }
            Message::ServiceReadiness => {
                state.follow_service().await;
                state.replan_retained_source_on_service_change(&myself);
                state.rearm_deferred_dirty_after_owner(&myself);
                state.schedule_service_release(false).await;
            }
            Message::ServiceProbeFailed => {
                state.publish_service_probe_failure();
                state.follow_service().await;
                state.schedule_service_release(false).await;
            }
            Message::ServiceRecoveryTick => {
                let can_recover = !state.mailbox.uncertain
                    && !state.mailbox.shutting_down
                    && state.mailbox.idle()
                    && !state.service_recovery_inflight;
                let recovery_due = if can_recover {
                    match state.workflow.as_ref() {
                        Some(workflow) => workflow.service_endpoint_recovery_due().await,
                        None => false,
                    }
                } else {
                    false
                };
                if recovery_due {
                    state.service_recovery_inflight = true;
                    state.runtime.service_recovery_maintenance =
                        Some("Service endpoint recovery is in progress".into());
                    state.publish_maintenance();
                    let service = state.service.clone();
                    let actor = myself.clone();
                    let operation_id = state.mailbox.allocate_operation_id();
                    spawn_service_recovery_lease(
                        actor,
                        service,
                        operation_id,
                        Message::Request,
                        || Message::ServiceRecoveryLeaseFailed,
                    );
                }
            }
            Message::ServiceRecoveryLeaseFailed => {
                state.service_recovery_inflight = false;
                state.runtime.service_recovery_maintenance =
                    Some("Service endpoint recovery is waiting for a transition lease".into());
                state.publish_maintenance();
            }
            Message::RecoverCore => {
                if state.mailbox.uncertain || state.mailbox.shutting_down {
                    tracing::warn!(
                        uncertain = state.mailbox.uncertain,
                        shutting_down = state.mailbox.shutting_down,
                        "ignoring crash recovery because lifecycle is unavailable"
                    );
                } else {
                    tracing::info!("queueing core recovery through lifecycle actor");
                    state.push_background(Command::RecoverCore);
                }
            }
            Message::SourceCommitted {
                domain,
                version,
                reconcile_requested,
                startup_recovery_required,
            } => {
                let facts = state.source_dispatch_facts();
                let dispatch = state.runtime.dispatch_source_committed(
                    domain,
                    version,
                    reconcile_requested,
                    startup_recovery_required,
                    facts,
                );
                state.apply_source_dispatch(&myself, domain, version, dispatch);
            }
            Message::CheckedSourceCommitted {
                domain,
                version,
                request,
            } => {
                let facts = state.source_dispatch_facts();
                let dispatch = state.runtime.dispatch_checked_source(
                    domain,
                    version,
                    request.as_deref(),
                    facts,
                );
                state.apply_source_dispatch(&myself, domain, version, dispatch);
            }
            Message::ApplyCheckedCommitted {
                domain,
                version,
                target,
                authority,
                reply,
            } => {
                let facts = state.source_dispatch_facts();
                if let Some(checked) = state
                    .runtime
                    .take_explicit_checked(domain, version, &target, facts)
                {
                    state.mailbox.enqueue_checked_source(
                        checked,
                        domain,
                        version,
                        authority,
                        Some(reply),
                    );
                } else {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "checked candidate has no matching committed source or Core owner"
                    )));
                }
            }
            Message::RuntimeDirty => {
                if state.runtime.run_intent == RunIntent::Stopped
                    || state.mailbox.uncertain
                    || state.mailbox.shutting_down
                {
                    tracing::debug!(
                        ?state.runtime.run_intent,
                        uncertain = state.mailbox.uncertain,
                        shutting_down = state.mailbox.shutting_down,
                        "ignoring runtime dirty signal while stopped or unavailable"
                    );
                } else {
                    state.mark_runtime_dirty(&myself);
                }
            }
            #[cfg(test)]
            Message::ReadSourceVersionsForTest { reply } => {
                let _ = reply.send(state.runtime.seen_sources.clone());
            }
            #[cfg(test)]
            Message::ReadAppliedSelectionForTest { reply } => {
                let selection = state
                    .workflow
                    .as_ref()
                    .and_then(|workflow| workflow.lifecycle().applied_selection_for_tests());
                let _ = reply.send(selection);
            }
            #[cfg(test)]
            Message::ReadCheckedForTest { reply } => {
                let _ = reply.send(state.runtime.checked.first().map(|checked| {
                    (
                        checked.prepared.target.clone(),
                        checked.prepared.intent.digest.clone(),
                        checked.prepared.snapshot.product_bytes().to_vec(),
                    )
                }));
            }
            Message::DirtyTick => {
                let owner_transition = state.owner_transition_pending();
                match state.runtime.dirty_tick(
                    state.mailbox.uncertain,
                    state.mailbox.shutting_down,
                    owner_transition,
                    state.mailbox.queue_full(),
                ) {
                    DirtyTick::None => {}
                    DirtyTick::Reconcile => state.push_background(Command::Reconcile),
                    DirtyTick::SourceCommitted => state.push_background(Command::ReconcileSource),
                }
            }
            Message::StartupRecoveryFinished { succeeded } => {
                if succeeded {
                    state.replan_retained_source_on_service_change(&myself);
                    state.rearm_deferred_dirty_after_owner(&myself);
                }
            }
            Message::StartupRetryDue { identity } => {
                use chimera_core::client::reconciler::dispatch::{
                    StartupRetryAdmission, admit_startup_retry,
                };
                match admit_startup_retry(
                    &state.mailbox,
                    &state.runtime,
                    state
                        .workflow
                        .as_mut()
                        .map(ApplicationWorkflow::progress_mut),
                    &identity,
                ) {
                    StartupRetryAdmission::Ignore => {}
                    StartupRetryAdmission::Reschedule(deadline) => {
                        state.schedule_startup_retry(&myself, identity, deadline)
                    }
                    StartupRetryAdmission::AcquireLease => {
                        let service = state.service.clone();
                        let actor = myself.clone();
                        let operation_id = state.mailbox.allocate_operation_id();
                        #[cfg(test)]
                        let observer = state.startup_retry_lease_failure_observer.take();
                        spawn_startup_retry_lease(
                            actor,
                            service,
                            operation_id,
                            identity,
                            Message::Request,
                            move |identity, reason| Message::StartupRetryLeaseFailed {
                                identity,
                                reason,
                                #[cfg(test)]
                                observer,
                                #[cfg(not(test))]
                                observer: None,
                            },
                        );
                    }
                }
            }
            Message::StartupRetryLeaseFailed {
                identity,
                reason,
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
                    let id = state.mailbox.allocate_operation_id();
                    state.mailbox.pending.push_back(Request {
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
                // Take the same lifecycle Arc before borrowing its progress
                // mutably. The worker owns it whenever workflow is absent.
                let lifecycle = state
                    .workflow
                    .as_ref()
                    .map(ApplicationWorkflow::lifecycle_handle);
                let workflow =
                    state
                        .workflow
                        .as_mut()
                        .zip(lifecycle.as_ref())
                        .map(|(workflow, lifecycle)| {
                            (workflow.progress_mut(), lifecycle.convergence_core())
                        });
                if let Some(next_attempt) =
                    chimera_core::client::reconciler::dispatch::dispatch_deferred_retry(
                        &mut state.mailbox,
                        workflow,
                        &digest,
                    )
                    .await
                {
                    state.schedule_deferred_retry(&myself, digest, next_attempt);
                }
            }
            #[cfg(not(test))]
            #[allow(unreachable_patterns)]
            _ => unreachable!("test-only reconciler message in production"),
        }
        state.drive(&myself);
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.timers.cancel();
        // The workflow (and its transaction/Service lease) is moved into the
        // active task. An unexpected Actor stop cannot drop that owner while
        // it might still be mutating Core or committing a profile decision.
        // The outcome is unproven because its Completed message was not
        // handled; do not publish a successful runtime conclusion.
        state.mailbox.join_lost_worker().await;
        state.runtime.checked.clear();
        state.reconciler.send_modify(ReconcilerStatus::close);
        Ok(())
    }
}

enum ClientInner {
    Actor(ActorClient),
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
pub(crate) struct RuntimeReconcilerClient(Arc<ClientInner>);

/// A direct, core-only test has no ServiceActor. Its Service endpoint is
/// explicitly **not installed**, not an ambiguous OS-global legacy probe.
/// This makes the fixture model a real no-daemon environment while keeping
/// production's fail-closed S2 ownership checks fully enabled.
#[cfg(test)]
struct DirectNoDaemonService {
    inner: Arc<dyn ports::ServiceLifecyclePort>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl ports::ServiceLifecyclePort for DirectNoDaemonService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        Ok(chimera_ipc::types::StatusInfo {
            name: "chimera-test-service".into(),
            version: "0".into(),
            status: chimera_ipc::types::ServiceStatus::NotInstalled,
            server: None,
        })
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.begin_transition().await
    }

    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.try_begin_transition().await
    }
}

impl RuntimeReconcilerClient {
    #[cfg(test)]
    pub(in crate::client) async fn spawn(
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

    pub(in crate::client) async fn spawn_with_service(
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

    pub(in crate::client) async fn spawn_with_profiles(
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
        Self::spawn_with_profiles_and_content(
            core,
            application,
            clash,
            profiles,
            None,
            None,
            runtime_paths,
            installer,
            service,
            shutdown,
            tasks,
        )
        .await
    }

    /// Production passes the read fence owned by the Profiles source actor;
    /// test adapters without a ProfilesActor may keep the old snapshot path.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::client) async fn spawn_with_profiles_and_content(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
        content_reader: Option<crate::state::mutation::ProfileContentReader>,
        content_source: Option<
            Arc<dyn chimera_config::runtime::executor::ProfileContentSource + Send + Sync>,
        >,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let source_authority =
            content_reader
                .clone()
                .zip(content_source)
                .map(|(reader, content)| {
                    Arc::new(
                        crate::client::reconciler::authority::LiveSourceAuthority::new(
                            application.clone(),
                            (*clash).clone(),
                            reader,
                            content,
                        ),
                    )
                        as Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>
                });
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
        .with_shutdown(shutdown.child_token())
        .with_service_status(service_status_rx.clone())
        .with_profile_content_reader(content_reader)
        .with_source_authority(source_authority.clone());
        let startup_owner_unproven = lifecycle.startup_owner_unproven_handle();
        #[cfg(not(test))]
        let startup_owner_unproven_signal = startup_owner_unproven.clone();
        let service_failed_generation = lifecycle.service_failed_generation_handle();
        let workflow = ApplicationWorkflow::with_lifecycle(lifecycle);
        let startup_recovery_pending = Arc::new(AtomicBool::new(false));
        let status = Arc::new(parking_lot::Mutex::new(CoreLifecycleStatus::default()));
        let (reconciler_tx, reconciler) = tokio::sync::watch::channel(ReconcilerStatus::default());
        let next_id = Arc::new(AtomicU64::new(1));
        let (actor_ref, _handle) = Actor::spawn(
            None,
            RuntimeReconcilerActor,
            RuntimeReconcilerArgs {
                reconciler: reconciler_tx,
                workflow,
                next_id: next_id.clone(),
                status: status.clone(),
                service_status,
                service: service.clone(),
                source_authority: source_authority.clone(),
                startup_owner_unproven_shared: startup_owner_unproven.clone(),
                startup_recovery_pending: startup_recovery_pending.clone(),
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
        #[cfg(not(test))]
        let service_actor_updates = service.actor_status_updates();
        #[cfg(not(test))]
        let service_followup_actor = actor_ref.clone();
        let client = Self(Arc::new(ClientInner::Actor(ActorClient {
            actor_ref,
            next_id,
            status,
            reconciler,
            service_status: service_status_rx.clone(),
            service,
            core,
            startup_owner_unproven,
            service_failed_generation,
            startup_recovery_pending,
            source_authority,
        })));
        #[cfg(not(test))]
        {
            let recovery_client = client.clone();
            let recovery_shutdown = shutdown.child_token();
            tasks.spawn(
                chimera_core::client::reconciler::service_observer::observe_service(
                    service_actor_updates,
                    service_status_rx,
                    startup_owner_unproven_signal,
                    recovery_shutdown,
                    crate::client::service_status::project_actor_service_status,
                    move || {
                        service_followup_actor
                            .cast(Message::ServiceReadiness)
                            .is_ok()
                    },
                    move || {
                        if recovery_client.startup_recovery_needed() {
                            recovery_client.request_startup_recovery();
                        }
                    },
                ),
            );
        }
        // Ref ServiceActor already published its initial daemon probe during
        // startup. No second CoreLifecycle probe/cache refresh is needed.
        Ok(client)
    }

    #[cfg(test)]
    pub(in crate::client) fn direct(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> Self {
        let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(DirectNoDaemonService {
            inner: Arc::new(LegacyServiceBridge::new(Arc::new(
                crate::core::actor_v2::CoreFacade::new_local(),
            ))),
        });
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
        Self(Arc::new(ClientInner::Direct {
            workflow: tokio::sync::Mutex::new(workflow),
            service_status: service_status_rx,
            service,
            core,
            startup_state: tokio::sync::Mutex::new(startup_state),
        }))
    }

    pub(in crate::client) async fn install_candidate_gate(
        &self,
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.install_candidate_gate(gate).await,
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                anyhow::bail!("production CandidateCheck requires the lifecycle actor")
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn connect_application_workflow(
        &self,
        workflow: ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        self.connect_application_workflow_with_registration(workflow, None)
            .await
    }

    #[cfg(test)]
    pub(in crate::client) async fn connect_application_workflow_with_registration(
        &self,
        workflow: ApplicationWorkflow,
        registered: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { actor_ref, .. }) => {
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
            ClientInner::Direct { .. } => {
                anyhow::bail!("application workflow requires the lifecycle actor")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn begin_profile_mutation(
        &self,
        request: crate::client::application_workflow::mutation::MutationRequest,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient {
                actor_ref, next_id, ..
            }) => {
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
            ClientInner::Direct { .. } => {
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
            ClientInner::Actor(client) => client.execute_with_timeout(command, timeout).await,
            #[cfg(test)]
            ClientInner::Direct {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
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

    pub(in crate::client) async fn startup_reconcile(
        &self,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        self.startup_reconcile_with_timeout(None).await
    }

    async fn cached_startup_report(&self) -> anyhow::Result<Option<StartupReport>> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.cached_startup_report().await,
            #[cfg(test)]
            ClientInner::Direct { workflow, .. } => Ok(workflow.lock().await.startup_report()),
        }
    }

    async fn startup_reconcile_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.startup_reconcile_with_timeout(timeout).await,
            #[cfg(test)]
            ClientInner::Direct {
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
            ClientInner::Actor(client) => client.startup_recovery_needed(),
            #[cfg(test)]
            ClientInner::Direct { .. } => false,
        }
    }

    /// The ref RuntimePreparation must share both its revision sequence and
    /// its confirmed port baseline with this same mutating Core owner.
    /// No new allocator or resolver is synthesized for compatibility ports.
    pub(in crate::client) fn runtime_preparation_session(
        &self,
    ) -> Option<crate::client::ports::RuntimePreparationSession> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.runtime_preparation_session(),
            #[cfg(test)]
            ClientInner::Direct { core, .. } => core.runtime_preparation_session(),
        }
    }

    pub(crate) fn status(&self) -> CoreLifecycleStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.status(),
            #[cfg(test)]
            ClientInner::Direct { .. } => CoreLifecycleStatus::default(),
        }
    }

    pub(crate) fn subscribe_reconciler(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<ReconcilerStatus>> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { reconciler, .. }) => Some(reconciler.clone()),
            #[cfg(test)]
            ClientInner::Direct { .. } => None,
        }
    }

    /// Ref-shaped outcome projection; unlike the legacy lifecycle status,
    /// this carries the last sampled source versions and runtime conclusion.
    pub(crate) fn reconciler_status(&self) -> ReconcilerStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.reconciler_status(),
            #[cfg(test)]
            ClientInner::Direct { .. } => ReconcilerStatus::default(),
        }
    }

    /// Explicitly dispatch an acknowledged, committed checked value through
    /// the same lifecycle queue used by the Profiles production gate. The
    /// canonical authority verifies the latest source content before and
    /// after Core apply; the composition root owns that reader. An ACK is
    /// proof of candidate ownership, not Core apply success.
    pub(in crate::client) async fn apply_checked_committed(
        &self,
        domain: SourceDomain,
        version: u64,
        target: String,
    ) -> anyhow::Result<()> {
        let authority = match self.0.as_ref() {
            ClientInner::Actor(ActorClient {
                source_authority: Some(port),
                ..
            }) => port.clone(),
            _ => anyhow::bail!("authoritative source verification is not connected"),
        };
        self.submit_checked_with_authority(domain, version, target, authority)
            .await
    }

    async fn submit_checked_with_authority(
        &self,
        domain: SourceDomain,
        version: u64,
        target: String,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
    ) -> anyhow::Result<()> {
        let ClientInner::Actor(client) = self.0.as_ref() else {
            anyhow::bail!("checked runtime apply requires the lifecycle actor");
        };
        client
            .submit_checked_with_authority(domain, version, target, authority)
            .await
    }

    /// The ref Source Gate always tags an ACKed candidate with its own
    /// durable commit domain; multiple domains can check concurrently.
    pub(in crate::client) async fn checked_candidate_for_domain(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_inner(domain, prepared, core_spec, None, false)
            .await
    }

    pub(in crate::client) async fn checked_candidate_for_domain_with_request(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: String,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_inner(domain, prepared, core_spec, Some(request), false)
            .await
    }

    pub(in crate::client) async fn checked_candidate_for_domain_with_request_effect(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: String,
        interrupt_mode: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_inner(domain, prepared, core_spec, Some(request), interrupt_mode)
            .await
    }

    async fn checked_candidate_inner(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: Option<String>,
        interrupt_mode: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        #[cfg(test)]
        use chimera_contracts::runtime_error::RuntimeError;
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client
                    .checked_candidate_inner(domain, prepared, core_spec, request, interrupt_mode)
                    .await
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => Err(Arc::new(RuntimeError::ShuttingDown)),
        }
    }

    #[cfg(test)]
    async fn checked_candidate(
        &self,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_for_domain(SourceDomain::Profiles, prepared, core_spec)
            .await
    }

    #[cfg(test)]
    async fn applied_selection_for_tests(
        &self,
    ) -> Option<crate::client::reconciler::committed::Selection> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            return None;
        };
        match actor_ref
            .call(|reply| Message::ReadAppliedSelectionForTest { reply }, None)
            .await
        {
            Ok(CallResult::Success(selection)) => selection,
            _ => None,
        }
    }

    #[cfg(test)]
    async fn checked_for_tests(&self) -> Option<(Option<String>, String, Vec<u8>)> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            return None;
        };
        match actor_ref
            .call(|reply| Message::ReadCheckedForTest { reply }, None)
            .await
        {
            Ok(CallResult::Success(snapshot)) => snapshot,
            _ => None,
        }
    }

    pub(crate) async fn settled(&self, domain: SourceDomain, version: u64) -> ReconcilerStatus {
        let ClientInner::Actor(ActorClient { reconciler, .. }) = self.0.as_ref() else {
            return ReconcilerStatus::default();
        };
        chimera_core::client::reconciler::status::settled(reconciler.clone(), domain, version).await
    }

    /// Conservative rollback for a failed source CAS after the Core owner
    /// acknowledged check admission. The abort and source notices share its
    /// one ordered mailbox; on cross-request same-domain ambiguity, the next
    /// successful commit falls back to authoritative latest-source recovery.
    pub(in crate::client) fn abort_uncommitted_checked(&self, domain: SourceDomain) {
        self.abort_checked_inner(domain, None);
    }

    pub(in crate::client) fn abort_uncommitted_checked_request(
        &self,
        domain: SourceDomain,
        request: String,
    ) {
        self.abort_checked_inner(domain, Some(request));
    }

    fn abort_checked_inner(&self, domain: SourceDomain, request: Option<String>) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.abort_checked_inner(domain, request),
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    /// Ref checked source notification. The single Actor consumes the cached
    /// checked value through ApplyChecked; it does not enqueue the old
    /// SourceCommitted rebuild when a checked value was available.
    pub(in crate::client) fn checked_source_committed(&self, domain: SourceDomain, version: u64) {
        self.checked_source_committed_inner(domain, version, None);
    }

    pub(in crate::client) fn checked_source_committed_for_request(
        &self,
        domain: SourceDomain,
        version: u64,
        request: String,
    ) {
        self.checked_source_committed_inner(domain, version, Some(request));
    }

    fn checked_source_committed_inner(
        &self,
        domain: SourceDomain,
        version: u64,
        request: Option<String>,
    ) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client.checked_source_committed_inner(domain, version, request)
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    /// Observe a source-owned durable commit in the single lifecycle mailbox.
    /// `reconcile_requested` asks the one runtime owner to apply the latest
    /// committed snapshots. Already-applied legacy domains pass false.
    pub(crate) fn source_committed(
        &self,
        domain: SourceDomain,
        version: u64,
        reconcile_requested: bool,
    ) {
        // Preserve request_runtime_rebuild's Service-generation fence: a
        // failed startup generation may require owner recovery even when the
        // generic "owner unproven" flag has already cleared. Reconcile must
        // not race that host transition.
        let recover_startup = reconcile_requested && self.startup_recovery_needed();
        if recover_startup {
            self.request_startup_recovery();
        }
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { actor_ref, .. }) => {
                if actor_ref
                    .cast(Message::SourceCommitted {
                        domain,
                        version,
                        reconcile_requested,
                        startup_recovery_required: recover_startup,
                    })
                    .is_err()
                {
                    tracing::warn!(
                        ?domain,
                        version,
                        "failed to notify lifecycle of source commit"
                    );
                }
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    pub(crate) fn request_runtime_rebuild(&self) {
        if self.startup_recovery_needed() {
            self.request_startup_recovery();
            return;
        }
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { actor_ref, .. }) => {
                if actor_ref.cast(Message::RuntimeDirty).is_err() {
                    tracing::warn!("failed to enqueue background runtime rebuild");
                }
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    fn request_startup_recovery(&self) {
        let (actor_ref, next_id, service, startup_recovery_pending) = match self.0.as_ref() {
            ClientInner::Actor(ActorClient {
                actor_ref,
                next_id,
                service,
                startup_recovery_pending,
                ..
            }) => (actor_ref, next_id, service, startup_recovery_pending),
            #[cfg(test)]
            ClientInner::Direct { .. } => return,
        };
        let client = self.clone();
        schedule_startup_recovery(
            actor_ref.clone(),
            next_id.clone(),
            service.clone(),
            startup_recovery_pending.clone(),
            move || client.startup_recovery_needed(),
            |operation_id, transition| Command::RecoverStartup {
                operation_id,
                transition,
            },
            Message::Request,
            |succeeded| Message::StartupRecoveryFinished { succeeded },
        );
    }

    pub(in crate::client) async fn request_service_endpoint_down(&self) {
        self.request_service_endpoint_down_if(|| true).await;
    }

    pub(in crate::client) async fn request_service_endpoint_down_if<F>(&self, still_current: F)
    where
        F: Fn() -> bool + Send + 'static,
    {
        let (actor_ref, next_id, service) = match self.0.as_ref() {
            ClientInner::Actor(ActorClient {
                actor_ref,
                next_id,
                service,
                ..
            }) => (actor_ref.clone(), next_id.clone(), service.clone()),
            #[cfg(test)]
            ClientInner::Direct { .. } => return,
        };

        chimera_core::client::reconciler::worker::request_service_endpoint_down_if(
            actor_ref,
            next_id,
            service,
            still_current,
            Command::ServiceEndpointDown,
            Message::Request,
        )
        .await;
    }

    pub(in crate::client) fn service_status(&self) -> ServiceHostStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.service_status(),
            #[cfg(test)]
            ClientInner::Direct { service_status, .. } => service_status.borrow().clone(),
        }
    }

    pub(in crate::client) fn observe_service_status(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
    ) {
        if let ClientInner::Actor(ActorClient {
            actor_ref, service, ..
        }) = self.0.as_ref()
            && service.actor_status().is_none()
            && actor_ref.cast(Message::ObserveService { info }).is_err()
        {
            tracing::debug!("failed to publish legacy service status observation");
        }
    }

    pub(in crate::client) fn observe_service_probe_failure(&self) {
        if let ClientInner::Actor(ActorClient {
            actor_ref, service, ..
        }) = self.0.as_ref()
            && service.actor_status().is_none()
            && actor_ref.cast(Message::ServiceProbeFailed).is_err()
        {
            tracing::debug!("failed to publish legacy service probe failure");
        }
    }

    #[cfg(test)]
    pub(in crate::client) async fn probe_service(&self) -> anyhow::Result<ServiceHostStatus> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { actor_ref, .. }) => {
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
            ClientInner::Direct { workflow, .. } => workflow
                .lock()
                .await
                .probe_service()
                .await
                .map(classify_service_probe),
        }
    }

    /// Explicitly resume core execution; retrying a source while stopped
    /// never implicitly changes this intent.
    pub(in crate::client) async fn start_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StartCore).await
    }

    pub(in crate::client) async fn stop_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StopCore).await
    }

    pub(in crate::client) async fn shutdown(&self) -> anyhow::Result<()> {
        self.execute(Command::Shutdown).await
    }

    pub(in crate::client) async fn reconcile(&self) -> anyhow::Result<()> {
        if self.startup_recovery_needed() {
            return self.recover_startup().await;
        }
        self.execute(Command::Reconcile).await
    }

    async fn recover_startup(&self) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.recover_startup().await,
            #[cfg(test)]
            ClientInner::Direct {
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
            ClientInner::Actor(client) => client.core_status().await,
            #[cfg(test)]
            ClientInner::Direct { core, .. } => core.status().await,
        }
    }

    // Only legacy actor tests exercise the original direct-core adapter.
    // Production selects through ChimeraClient::patch_app_config instead.
    #[cfg(test)]
    pub(in crate::client) async fn select_core(
        &self,
        core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.execute(Command::SelectCore(core)).await
    }

    pub(in crate::client) async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<workflow::ApplicationPatchResult> {
        let (degradations, settled) = tokio::sync::oneshot::channel();
        self.execute(Command::PatchApplicationConfig {
            patch,
            service_connected: false,
            degradations,
            runtime_converged: Arc::new(AtomicBool::new(true)),
        })
        .await?;
        settled
            .await
            .map_err(|_| anyhow::anyhow!("application runtime result was not delivered"))
    }

    pub(in crate::client) async fn replace_core_binary(
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
            ClientInner::Actor(ActorClient { service, .. }) => service.clone(),
            #[cfg(test)]
            ClientInner::Direct { service, .. } => service.clone(),
        };
        let transition = service.begin_transition().await?;
        self.execute(command(transition)).await
    }

    pub(in crate::client) async fn install_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::InstallService).await
    }

    pub(in crate::client) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UninstallService)
            .await
    }

    pub(in crate::client) async fn update_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UpdateService).await
    }

    pub(in crate::client) async fn start_service(
        &self,
        ready_timeout: Duration,
    ) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::StartService {
            transition,
            ready_timeout,
        })
        .await
    }

    pub(in crate::client) async fn restart_service(
        &self,
        ready_timeout: Duration,
    ) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::RestartService {
            transition,
            ready_timeout,
        })
        .await
    }

    pub(in crate::client) async fn stop_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::StopService).await
    }
}

impl ChimeraClient {
    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        // Legacy frontend IPC only has Running/Stopped. Do not collapse ref's
        // Starting/Switching/Degraded projections into a fake Stopped result;
        // keep this compatibility endpoint until the UI consumes the typed
        // CoreActor projection directly.
        self.inner.shared.core().status().await
    }

    pub(crate) fn service_status(&self) -> ServiceHostStatus {
        if let Some(service) = self.inner.shared.service_actor() {
            return crate::client::service_status::project_actor_service_status(&service.status());
        }
        // Injected legacy lifecycle fixtures deliberately omit ServiceActor.
        // Production always owns the ref ServiceClient from Tauri setup().
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
        self.inner.shared.core().runtime_transform_diagnostics()
    }

    pub(crate) fn promoted_runtime_snapshot(
        &self,
    ) -> Option<Arc<crate::client::runtime::RuntimeSnapshot>> {
        self.inner.shared.core().promoted_runtime_snapshot()
    }

    /// Ref: selecting a core is an Application source transaction, not a
    /// second mutating CoreLifecycle command. Preserve the commit receipt
    /// and post-commit runtime degradations for the UI.
    pub(crate) async fn change_core(
        &self,
        clash_core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<crate::client::runtime::MutationOutcome<()>> {
        self.patch_app_config(chimera_config::application::ChimeraAppConfigPatch {
            core: Some(clash_core),
            ..Default::default()
        })
        .await
    }

    pub(crate) async fn start_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.start_core().await
    }

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
#[path = "tests.rs"]
mod tests;

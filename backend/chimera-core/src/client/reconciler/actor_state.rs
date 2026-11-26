//! The sole RuntimeReconciler mailbox's state and execution methods.
use super::{
    client::Message,
    committed::SourceDomain,
    dispatch::{RoundAdmission, request_kind},
    mailbox::{AdmissionFailure, DIRTY_WINDOW, Mailbox, MailboxTimers, Request},
    source_dispatch::{SourceAction, SourceDispatch, SourceDispatchFacts},
    startup_transaction::StartupTransactionHost,
    status::ReconcilerStatus,
    worker::{WorkerWorkflow, schedule_startup_recovery},
    workflow::RuntimeReconcilerState,
    workflow_progress::WorkflowProgress,
};
use crate::{
    actor_v2::{
        service_policy::ServiceRestartPolicySnapshot,
        service_status::{ServiceHostStatus, ServiceHostWatch},
    },
    client::core_lifecycle::{command::Command, ports::ServiceLifecyclePort},
};
use ractor::{ActorRef, RpcReplyPort};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Host workflow adapter: borrow the original progress and lifecycle, and
/// delegate only the existing desktop ownership/legacy fixture operations.
#[async_trait::async_trait]
pub trait MailboxWorkflow<P: Send + 'static>:
    WorkerWorkflow<Command<P>> + Sized + Send + 'static
{
    type Lifecycle: StartupTransactionHost<Core = dyn crate::client::core_lifecycle::ports::CoreLifecyclePort>
        + super::apply_executor::ApplyHost
        + 'static;
    fn lifecycle(&self) -> &Self::Lifecycle;
    fn progress(&self) -> &WorkflowProgress;
    fn progress_mut(&mut self) -> &mut WorkflowProgress;
    fn recovery_command_allowed(&self, command: &Command<P>) -> bool;
    fn legacy_runtime_owned(info: &chimera_ipc::types::StatusInfo<'_>) -> bool;
    async fn service_release_observation(&self) -> super::release::ReleaseObservation;
    fn reject_workflow_command(command: &mut Command<P>, error: &anyhow::Error);
    fn admit_workflow_command(&self, command: &Command<P>) -> anyhow::Result<()>;
    fn lifecycle_handle(&self) -> Arc<Self::Lifecycle>;
    fn install_candidate_gate(&self, gate: Arc<super::check::CandidateCheck>)
    -> anyhow::Result<()>;
    fn legacy_service_connected() -> bool;
    fn checked_clash_notice(notice: Result<(), String>);
    fn take_prepared_outcome(&self) -> Option<super::mutation::ApplyOutcome>;
    fn take_checked_connection_effect(&self) -> Option<super::apply::CheckedConnectionEffect>;
    async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings>;
    fn runtime_bound(&self, ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>);
    async fn service_endpoint_recovery_due(&self) -> bool;
    // Production refuses fixture messages even when Cargo unifies test-support.
    async fn handle_test_message(
        _message: Message<Self, P>,
        _state: &mut RuntimeReconcilerMailbox<Self, P>,
    ) -> Result<bool, ractor::ActorProcessingErr> {
        Err(anyhow::anyhow!("test-only reconciler message in production").into())
    }
    fn restore_pending_connection(_state: &mut RuntimeReconcilerMailbox<Self, P>) {}
}

pub struct RuntimeReconcilerMailbox<W: Send + 'static, P: Send + 'static> {
    pub mailbox: Mailbox<Command<P>>,
    pub timers: MailboxTimers<Message<W, P>>,
    pub reconciler: tokio::sync::watch::Sender<ReconcilerStatus>,
    pub workflow: Option<W>,
    pub pending_mutation_connection: Option<(W, RpcReplyPort<anyhow::Result<()>>)>,
    pub pending_startup_retry_lease_failures: VecDeque<(String, String)>,
    pub startup_retry_lease_failure_observer: Option<tokio::sync::oneshot::Sender<()>>,
    pub service_status: ServiceHostWatch,
    pub service: Arc<dyn ServiceLifecyclePort>,
    pub runtime: RuntimeReconcilerState,
    pub source_authority:
        Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    pub service_recovery_inflight: bool,
    pub startup_recovery_pending: Arc<AtomicBool>,
}

impl<W: MailboxWorkflow<P>, P: Send + 'static> RuntimeReconcilerMailbox<W, P> {
    pub fn source_dispatch_facts(&self) -> SourceDispatchFacts {
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

    pub fn apply_source_dispatch(
        &mut self,
        myself: &ActorRef<Message<W, P>>,
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
    pub fn publishes_legacy_service_status(&self) -> bool {
        self.service.actor_status().is_none()
    }

    pub fn publish_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        if self.publishes_legacy_service_status() {
            let runtime_owned = W::legacy_runtime_owned(&info);
            self.service_status.observe_probe(info, runtime_owned, None);
        }
    }

    pub fn publish_service_status_with_policy(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
        policy: ServiceRestartPolicySnapshot,
    ) {
        if self.publishes_legacy_service_status() {
            let runtime_owned = W::legacy_runtime_owned(&info);
            self.service_status
                .observe_probe(info, runtime_owned, Some(policy));
        }
    }

    pub fn publish_service_probe_failure(&self) {
        if self.publishes_legacy_service_status() {
            self.service_status.probe_failed();
        }
    }

    pub async fn refresh_service_status(&mut self) -> anyhow::Result<ServiceHostStatus> {
        if let Some(actor) = self.service.actor_status() {
            return Ok(actor);
        }
        let workflow = self
            .workflow
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("core lifecycle workflow is busy"))?;
        let policy = workflow.observe_worker().service_restart_policy;
        match workflow.probe_worker_service().await {
            Ok(info) => {
                let runtime_owned = W::legacy_runtime_owned(&info);
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

    pub fn mark_runtime_dirty(&mut self, myself: &ActorRef<Message<W, P>>) {
        if self
            .runtime
            .request_dirty_tick(self.mailbox.uncertain, self.mailbox.shutting_down)
        {
            self.timers
                .after(myself, DIRTY_WINDOW, || Message::DirtyTick);
        }
    }

    pub fn schedule_deferred_retry(
        &mut self,
        myself: &ActorRef<Message<W, P>>,
        digest: String,
        next_attempt: tokio::time::Instant,
    ) {
        self.timers
            .at(myself, next_attempt, move || Message::DeferredRetry {
                digest,
            });
    }

    pub fn reschedule_deferred_retry(
        &mut self,
        myself: &ActorRef<Message<W, P>>,
        digest: &str,
        delay: Duration,
    ) {
        let next_attempt = self.workflow.as_mut().and_then(|workflow| {
            workflow
                .progress_mut()
                .reschedule_deferred_retry(digest, delay)
        });
        if let Some(next_attempt) = next_attempt {
            self.schedule_deferred_retry(myself, digest.to_owned(), next_attempt);
        }
    }

    pub fn schedule_startup_retry(
        &mut self,
        myself: &ActorRef<Message<W, P>>,
        identity: String,
        next_attempt: tokio::time::Instant,
    ) {
        self.timers
            .at(myself, next_attempt, move || Message::StartupRetryDue {
                identity,
            });
    }

    pub fn request_startup_recovery(&self, myself: &ActorRef<Message<W, P>>) {
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
    pub fn publish_maintenance(&self) {
        super::dispatch::publish_maintenance(&self.runtime, &self.reconciler);
    }

    /// Shared S2 admission barrier. A Startup recovery worker may hold the
    /// Service transition lease *outside* the CoreLifecycle mailbox; startup
    /// and follow-up owner commands may also be queued behind another action.
    /// Neither a readiness event nor a source notification can infer an
    /// established owner before those transitions have settled.
    pub fn owner_transition_pending(&self) -> bool {
        self.mailbox
            .owner_transition_pending(&self.startup_recovery_pending)
    }

    /// Evaluate release debt after a proven runtime round or a Service
    /// observation. This never starts a second Core controller: the actual
    /// daemon stop is admitted as its own serialized lifecycle command.
    pub async fn schedule_service_release(&mut self, explicit_retry: bool) {
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
        let observation = workflow.service_release_observation().await;
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
    pub fn replan_retained_source_on_service_change(&mut self, myself: &ActorRef<Message<W, P>>) {
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
    pub fn rearm_deferred_dirty_after_owner(&mut self, myself: &ActorRef<Message<W, P>>) {
        if self.runtime.dirty
            && self
                .mailbox
                .owner_followup_allowed(&self.runtime, &self.startup_recovery_pending)
        {
            self.mark_runtime_dirty(myself);
        }
    }

    pub async fn follow_service(&mut self) {
        if super::dispatch::follow_service(
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

    pub fn reject(&mut self, request: Request<Command<P>>, error: anyhow::Error) {
        let mut request = request;
        if matches!(&request.command, Command::ServiceEndpointDown(_)) {
            self.service_recovery_inflight = false;
        }
        W::reject_workflow_command(&mut request.command, &error);
        if let Command::ReplaceCoreBinary(artifact) = &request.command {
            let message = error.to_string();
            super::worker::finish_binary_progress(artifact.progress.as_ref(), Some(&message));
        }
        self.mailbox.settle(request.response, Err(error));
    }

    pub fn reject_pending(&mut self, myself: &ActorRef<Message<W, P>>) {
        if self.mailbox.uncertain && !self.mailbox.shutting_down {
            self.runtime.dirty = false;
        }
        let workflow = self.workflow.as_ref();
        let refused = self.mailbox.drain_refused(request_kind, |command| {
            workflow.is_none_or(|workflow| {
                !workflow.progress().has_unresolved_attempt()
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

    pub fn close(&mut self) {
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

    pub fn push_background(&mut self, command: Command<P>) {
        self.mailbox.push_background(command);
    }

    pub fn drive(&mut self, myself: &ActorRef<Message<W, P>>) {
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

        let Some(workflow) = self.workflow.take() else {
            tracing::error!("core lifecycle workflow missing without an active operation");
            self.reject(
                request,
                anyhow::anyhow!("core lifecycle workflow unavailable"),
            );
            self.mailbox.publish();
            return;
        };

        if let Err(error) = workflow.admit_workflow_command(&request.command) {
            self.workflow = Some(workflow);
            self.reject(request, error);
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

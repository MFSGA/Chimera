//! Round admission and dispatch for the single runtime mailbox.
use super::{
    committed::SourceDomain,
    mailbox::{
        ActiveOperation, AdmissionFailure, Mailbox, Request, RequestKind, Response,
        ShutdownAdmission, replay_shutdown,
    },
    plan::{Conclusion, RunIntent},
    release::ReleaseObservation,
    worker::{WorkerCompletion, WorkerPlan, WorkerWorkflow, spawn_worker},
    workflow::{RuntimeAdmission, RuntimeReconcilerState},
};
use crate::{
    actor_v2::service_policy::ServicePhase,
    client::core_lifecycle::{
        command::{Command, runtime_trigger},
        ports::ServiceTransitionLease,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub fn is_source_only_application_patch<P>(command: &Command<P>) -> bool {
    matches!(
        command,
        Command::PatchApplicationConfig {
            patch,
            service_connected,
            ..
        }
            if !crate::state::mutation::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            )
    )
}

pub fn request_kind<P>(command: &Command<P>) -> RequestKind {
    if matches!(command, Command::CacheStartupUnsettled { .. })
        || is_source_only_application_patch(command)
    {
        RequestKind::SourceOnly
    } else if matches!(
        command,
        Command::StartupReconcile { .. }
            | Command::RecoverStartup { .. }
            | Command::RetryStartup { .. }
            | Command::ServiceEndpointDown(_)
    ) {
        RequestKind::StartupOwner
    } else {
        RequestKind::Other
    }
}

/// Ref's sole request admission, retaining the staged Chimera shutdown waiters
/// and actual host connectivity sampled in the same mailbox turn.
pub enum RequestAdmission<P> {
    Accepted,
    Close,
    Reject(Request<Command<P>>, AdmissionFailure),
}

impl<P> Mailbox<Command<P>> {
    pub fn receive_request(
        &mut self,
        mut request: Request<Command<P>>,
        service_connected: bool,
    ) -> RequestAdmission<P> {
        if let Command::PatchApplicationConfig {
            service_connected: patch_service_connected,
            ..
        } = &mut request.command
        {
            *patch_service_connected = service_connected;
        }
        if matches!(&request.command, Command::Shutdown) {
            match self.shutdown_admission() {
                ShutdownAdmission::Replay(error) => {
                    self.settle(request.response, replay_shutdown(&error));
                }
                ShutdownAdmission::Full => {
                    return RequestAdmission::Reject(
                        request,
                        AdmissionFailure::TooManyShutdownWaiters,
                    );
                }
                ShutdownAdmission::Wait { first } => {
                    self.enqueue_shutdown_waiter(request.response);
                    if first {
                        return RequestAdmission::Close;
                    }
                }
            }
        } else if let Err(reason) = self.admit_request(request_kind(&request.command)) {
            return RequestAdmission::Reject(request, reason);
        } else {
            self.pending.push_back(request);
        }
        RequestAdmission::Accepted
    }
}

/// Check run intent and the current committed version when a queued request
/// reaches execution, not merely when the original caller enqueues it.
pub enum RoundAdmission<P> {
    Run(Request<Command<P>>),
    Skipped,
    Rejected(Request<Command<P>>, anyhow::Error),
}

impl<P> Mailbox<Command<P>> {
    pub fn admit_round(
        &self,
        runtime: &mut RuntimeReconcilerState,
        request: Request<Command<P>>,
    ) -> RoundAdmission<P> {
        use crate::client::core_lifecycle::command::admission_command;
        match runtime.admit(admission_command(&request.command)) {
            RuntimeAdmission::Run => RoundAdmission::Run(request),
            RuntimeAdmission::SkipInactive => {
                self.settle(request.response, Ok(()));
                RoundAdmission::Skipped
            }
            RuntimeAdmission::RejectStoppedChecked => RoundAdmission::Rejected(
                request,
                anyhow::anyhow!("checked runtime cannot apply while stopped"),
            ),
            RuntimeAdmission::RejectStaleChecked => RoundAdmission::Rejected(
                request,
                anyhow::anyhow!("checked runtime source is no longer the latest committed version"),
            ),
        }
    }

    /// Start a round on the existing worker and retain its JoinHandle in the
    /// same mailbox. A host phase observer is the only side effect supplied
    /// by the composition root; no separate command execution path exists.
    pub fn launch_round<W, M>(
        &mut self,
        actor: &ractor::ActorRef<M>,
        workflow: W,
        request: Request<Command<P>>,
        runtime: &RuntimeReconcilerState,
        reconciler: &tokio::sync::watch::Sender<super::status::ReconcilerStatus>,
        publish_legacy_service: bool,
        publish_phase: impl FnOnce(ServicePhase),
        completed: impl FnOnce(WorkerCompletion<W>) -> M + Send + 'static,
    ) where
        P: Send + 'static,
        W: WorkerWorkflow<Command<P>>,
        M: ractor::Message,
    {
        let observed_sources = runtime.seen_sources.clone();
        reconciler.send_modify(|status| status.begin(request.response.id));
        let RoundDispatch {
            plan,
            trigger,
            checked_apply,
            checked_clash,
            service_phase,
        } = prepare_round(&request, publish_legacy_service);
        let shutdown = plan.shutdown;
        if let Some(phase) = service_phase {
            publish_phase(phase);
        }
        let task = spawn_worker(actor, workflow, request.command, plan, completed);
        self.active = Some(ActiveOperation {
            response: request.response,
            task,
            shutdown,
            trigger,
            checked_clash,
            checked_apply,
            observed_sources,
        });
        self.publish();
    }
}

pub struct RoundDispatch {
    pub plan: WorkerPlan,
    pub trigger: &'static str,
    pub checked_apply: bool,
    pub checked_clash: bool,
    pub service_phase: Option<ServicePhase>,
}

pub fn prepare_round<P>(
    request: &Request<Command<P>>,
    publish_legacy_service: bool,
) -> RoundDispatch {
    let id = request.response.id;
    let trigger = runtime_trigger(&request.command);
    let checked_apply = matches!(&request.command, Command::ApplyChecked { .. });
    let checked_clash = matches!(
        &request.command,
        Command::ApplyChecked {
            domain: SourceDomain::Clash,
            ..
        }
    );
    let shutdown = matches!(&request.command, Command::Shutdown);
    let deferred_retry = request.deferred_retry.clone();
    let runtime_converged = match &request.command {
        Command::PatchApplicationConfig {
            runtime_converged, ..
        } => Some(Arc::clone(runtime_converged)),
        _ => None,
    };
    let application_runtime_reconcile = matches!(
        &request.command,
        Command::PatchApplicationConfig {
            patch,
            service_connected,
            ..
        }
            if crate::state::mutation::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            )
    );
    let reconciled = matches!(
        &request.command,
        Command::Reconcile
            | Command::ReconcileSource
            | Command::ApplyChecked { .. }
            | Command::StartCore
            | Command::StartupReconcile { .. }
            | Command::FollowService { .. }
            | Command::RecoverStartup { .. }
            | Command::RetryStartup { .. }
            | Command::RetryDeferredProfiles { .. }
            | Command::ReconcileProfiles { .. }
            | Command::RecoverCore
            | Command::ReplaceCoreBinary(_)
    ) || application_runtime_reconcile;
    #[cfg(any(test, feature = "test-support"))]
    let reconciled = reconciled || matches!(&request.command, Command::SelectCore(_));
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
            | Command::FollowService { .. }
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
    let service_endpoint_recovery = matches!(&request.command, Command::ServiceEndpointDown(_));
    let progress = match &request.command {
        Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
        _ => None,
    };

    let plan = WorkerPlan {
        id,
        shutdown,
        deferred_retry,
        reconciled,
        runtime_converged,
        service_mutation,
        publish_legacy_service,
        retry_reconcile_on_failure,
        recover,
        service_endpoint_recovery,
        progress,
    };
    RoundDispatch {
        plan,
        trigger,
        checked_apply,
        checked_clash,
        service_phase,
    }
}

impl<P> Mailbox<Command<P>> {
    pub fn enqueue_checked_source(
        &mut self,
        checked: super::checked::CheckedRuntimeCandidate,
        domain: SourceDomain,
        version: u64,
        authority: Arc<dyn super::authority::RuntimeSourceAuthority>,
        reply: Option<ractor::RpcReplyPort<anyhow::Result<()>>>,
    ) {
        let id = self.allocate_operation_id();
        self.pending.push_back(Request {
            command: Command::ApplyChecked {
                prepared: checked.prepared,
                core_spec: checked.core_spec,
                domain,
                version,
                authority,
                interrupt_mode: checked.interrupt_mode,
            },
            response: Response { id, reply },
            deferred_retry: None,
        });
    }

    /// S2 owner transition may already hold a Service lease outside the queue.
    pub fn owner_transition_pending(&self, startup_recovery_pending: &AtomicBool) -> bool {
        startup_recovery_pending.load(Ordering::Acquire)
            || self.pending.iter().any(|request| {
                matches!(
                    &request.command,
                    Command::FollowService { .. }
                        | Command::StartupReconcile { .. }
                        | Command::RecoverStartup { .. }
                        | Command::RetryStartup { .. }
                )
            })
    }

    pub fn owner_followup_allowed(
        &self,
        runtime: &RuntimeReconcilerState,
        startup_recovery_pending: &AtomicBool,
    ) -> bool {
        runtime.run_intent == RunIntent::Running
            && !self.uncertain
            && !self.shutting_down
            && !self.owner_unproven()
            && self.active.is_none()
            && !self.owner_transition_pending(startup_recovery_pending)
    }

    pub fn push_background(&mut self, command: Command<P>) -> bool {
        if self.queue_full() {
            tracing::warn!("dropping background lifecycle operation because the queue is full");
            return false;
        }
        self.pending.push_back(Request {
            command,
            response: Response {
                id: self.allocate_operation_id(),
                reply: None,
            },
            deferred_retry: None,
        });
        true
    }

    pub fn push_deferred_retry(&mut self, digest: String) -> bool {
        if self.queue_full() {
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

    pub fn enqueue_follow_service(
        &mut self,
        runtime: &mut RuntimeReconcilerState,
        transition: Box<dyn ServiceTransitionLease>,
    ) {
        if self.queue_full() {
            tracing::warn!("dropping Service owner follow-up because the lifecycle queue is full");
            runtime.service_handoff_maintenance =
                Some("Service owner handoff is waiting for lifecycle queue capacity".into());
            return;
        }
        runtime.service_handoff_maintenance = Some("Service owner handoff is pending".into());
        let operation_id = self.allocate_operation_id();
        // The lease is already held; run before later Core mutations.
        self.pending.push_front(Request {
            command: Command::FollowService {
                operation_id,
                transition,
            },
            response: Response {
                id: operation_id,
                reply: None,
            },
            deferred_retry: None,
        });
    }

    pub fn release_allowed(
        &self,
        runtime: &RuntimeReconcilerState,
        startup_recovery_pending: &AtomicBool,
        conclusion: &Conclusion,
    ) -> bool {
        self.owner_followup_allowed(runtime, startup_recovery_pending)
            && matches!(conclusion, Conclusion::Converged)
    }

    pub fn observe_release(
        &mut self,
        runtime: &mut RuntimeReconcilerState,
        observation: ReleaseObservation,
        explicit_retry: bool,
    ) {
        runtime.service_release_debt.observe(observation);
        if observation != ReleaseObservation::Owed
            || !runtime.service_release_debt.begin(explicit_retry)
        {
            return;
        }
        if self.queue_full() {
            runtime
                .service_release_debt
                .deferred("lifecycle queue is full".into());
            return;
        }
        self.push_background(Command::MaintainServiceRelease);
    }
}

/// Service follow-up holds the existing host lease before enqueueing a
/// transition. Recheck the exact owner evidence after acquiring it.
pub async fn follow_service<P>(
    mailbox: &mut Mailbox<Command<P>>,
    runtime: &mut RuntimeReconcilerState,
    startup_recovery_pending: &AtomicBool,
    workflow: Option<(
        &super::workflow_progress::WorkflowProgress,
        &impl super::startup_transaction::StartupTransactionHost,
    )>,
    service: &dyn crate::client::core_lifecycle::ports::ServiceLifecyclePort,
) -> bool {
    if !mailbox.owner_followup_allowed(runtime, startup_recovery_pending) {
        return false;
    }
    let Some((progress, lifecycle)) = workflow else {
        return false;
    };
    if !progress.service_host_change_due(lifecycle).await {
        return false;
    }
    let transition = match service.try_begin_transition().await {
        Ok(transition) => transition,
        Err(error) => {
            tracing::debug!(%error, "Service owner follow-up deferred while a transition is active");
            runtime.service_handoff_maintenance = Some(format!(
                "Service ownership transition is waiting for its lease: {error:#}"
            ));
            return true;
        }
    };
    if !progress.service_host_change_due(lifecycle).await {
        return false;
    }
    mailbox.enqueue_follow_service(runtime, transition);
    true
}

/// Keep deferred Profile recovery on the original mailbox and progress owner.
/// No timer or retry spends a budget before the current Core state is known.
pub async fn dispatch_deferred_retry<P>(
    mailbox: &mut Mailbox<Command<P>>,
    workflow: Option<(
        &mut super::workflow_progress::WorkflowProgress,
        &dyn crate::client::core_lifecycle::ports::CoreLifecyclePort,
    )>,
    digest: &str,
) -> Option<tokio::time::Instant> {
    use std::time::Duration;
    if mailbox.uncertain || mailbox.shutting_down {
        if let Some((progress, _)) = workflow {
            progress
                .finish_deferred_retry(digest, false, false, false, mailbox.uncertain, None)
                .await;
        }
        return None;
    }
    let Some((progress, core)) = workflow else {
        return Some(tokio::time::Instant::now() + super::mailbox::DIRTY_WINDOW);
    };
    if !mailbox.idle() || progress.has_unresolved_attempt() {
        return progress.reschedule_deferred_retry(digest, Duration::from_secs(5));
    }
    match core.status().await.ok() {
        Some(status) if matches!(status.state, chimera_ipc::api::status::CoreState::Running) => {
            if progress.begin_deferred_retry(digest)
                && !mailbox.push_deferred_retry(digest.to_owned())
            {
                return progress.reschedule_deferred_retry(digest, Duration::from_secs(1));
            }
        }
        Some(_) => progress.wait_for_deferred_dependency(digest),
        None => return progress.reschedule_deferred_retry(digest, Duration::from_secs(5)),
    }
    None
}

/// Startup timers must not acquire a new host lease after StopCore.
pub enum StartupRetryAdmission {
    Ignore,
    Reschedule(tokio::time::Instant),
    AcquireLease,
}

pub fn admit_startup_retry<P>(
    mailbox: &Mailbox<Command<P>>,
    runtime: &RuntimeReconcilerState,
    progress: Option<&mut super::workflow_progress::WorkflowProgress>,
    identity: &str,
) -> StartupRetryAdmission {
    if runtime.run_intent != RunIntent::Running || mailbox.uncertain || mailbox.shutting_down {
        return StartupRetryAdmission::Ignore;
    }
    let Some(progress) = progress else {
        return StartupRetryAdmission::Reschedule(
            tokio::time::Instant::now() + super::mailbox::DIRTY_WINDOW,
        );
    };
    if progress.startup_retry_due(identity) && progress.begin_startup_retry(identity) {
        StartupRetryAdmission::AcquireLease
    } else {
        StartupRetryAdmission::Ignore
    }
}

pub fn publish_maintenance(
    runtime: &RuntimeReconcilerState,
    reconciler: &tokio::sync::watch::Sender<super::status::ReconcilerStatus>,
) {
    let product = reconciler.borrow().product_publication_maintenance();
    let reason = runtime.maintenance(product.as_deref());
    reconciler.send_modify(|status| status.set_maintenance(reason));
}

pub fn record_startup_retry_lease_failure(
    workflow: Option<&mut super::workflow_progress::WorkflowProgress>,
    pending: &mut std::collections::VecDeque<(String, String)>,
    identity: String,
    reason: String,
) {
    if let Some(workflow) = workflow {
        workflow.defer_startup_retry(&identity, reason);
    } else {
        pending.push_back((identity, reason));
    }
}

pub fn apply_pending_startup_retry_lease_failures(
    workflow: Option<&mut super::workflow_progress::WorkflowProgress>,
    pending: &mut std::collections::VecDeque<(String, String)>,
) {
    let Some(workflow) = workflow else {
        return;
    };
    while let Some((identity, reason)) = pending.pop_front() {
        workflow.defer_startup_retry(&identity, reason);
    }
}

#[cfg(test)]
mod tests {
    use super::super::mailbox::{CoreLifecycleStatus, MAX_PENDING};
    use super::*;
    use parking_lot::Mutex;
    use std::sync::atomic::AtomicU64;

    fn mailbox() -> Mailbox<Command<()>> {
        Mailbox::new(
            Arc::new(AtomicU64::new(1)),
            Arc::new(Mutex::new(CoreLifecycleStatus::default())),
            Arc::new(AtomicBool::new(false)),
        )
    }

    fn request(command: Command<()>) -> Request<Command<()>> {
        Request {
            command,
            response: Response {
                id: 42,
                reply: None,
            },
            deferred_retry: None,
        }
    }

    #[test]
    fn startup_timer_never_acquires_lease_when_stopped_or_uncertain() {
        let mut mailbox = mailbox();
        let mut runtime = RuntimeReconcilerState::default();
        assert!(matches!(
            admit_startup_retry(&mailbox, &runtime, None, "target"),
            StartupRetryAdmission::Reschedule(_)
        ));
        runtime.run_intent = RunIntent::Stopped;
        assert!(matches!(
            admit_startup_retry(&mailbox, &runtime, None, "target"),
            StartupRetryAdmission::Ignore
        ));
        runtime.run_intent = RunIntent::Running;
        mailbox.uncertain = true;
        assert!(matches!(
            admit_startup_retry(&mailbox, &runtime, None, "target"),
            StartupRetryAdmission::Ignore
        ));
        mailbox.uncertain = false;
        mailbox.close();
        assert!(matches!(
            admit_startup_retry(&mailbox, &runtime, None, "target"),
            StartupRetryAdmission::Ignore
        ));
        assert!(mailbox.pending.is_empty());
    }

    #[tokio::test]
    async fn deferred_timer_preserves_target_while_worker_owns_workflow_but_not_after_close() {
        let mut mailbox = mailbox();
        assert!(
            dispatch_deferred_retry(&mut mailbox, None, "target")
                .await
                .is_some()
        );
        mailbox.uncertain = true;
        assert!(
            dispatch_deferred_retry(&mut mailbox, None, "target")
                .await
                .is_none()
        );
        mailbox.uncertain = false;
        mailbox.close();
        assert!(
            dispatch_deferred_retry(&mut mailbox, None, "target")
                .await
                .is_none()
        );
        assert!(mailbox.pending.is_empty());
    }

    #[test]
    fn received_request_and_shutdown_use_same_admission_and_terminal_history() {
        let mut mailbox = mailbox();
        assert!(matches!(
            mailbox.receive_request(request(Command::Reconcile), false),
            RequestAdmission::Accepted
        ));
        assert_eq!(mailbox.pending.len(), 1);
        assert!(matches!(
            mailbox.receive_request(request(Command::Shutdown), false),
            RequestAdmission::Close
        ));
        assert_eq!(mailbox.close().len(), 1);
        assert!(matches!(
            mailbox.receive_request(request(Command::Reconcile), false),
            RequestAdmission::Reject(_, AdmissionFailure::Closing)
        ));
        let next = mailbox.take_next(|| Command::Shutdown).unwrap();
        mailbox.finish_operation(
            next.response,
            true,
            Err(anyhow::anyhow!("unconfirmed shutdown")),
        );
        assert!(matches!(
            mailbox.receive_request(request(Command::Shutdown), false),
            RequestAdmission::Accepted
        ));
        let status = mailbox.status.lock();
        assert_eq!(status.completed.len(), 2);
        assert!(
            status
                .completed
                .iter()
                .all(|result| result.error.as_deref() == Some("unconfirmed shutdown"))
        );
    }

    #[test]
    fn queued_reconcile_rechecks_stopped_intent_and_settles_without_worker() {
        let mut mailbox = mailbox();
        let mut runtime = RuntimeReconcilerState::default();
        assert!(matches!(
            mailbox.receive_request(request(Command::Reconcile), false),
            RequestAdmission::Accepted
        ));
        runtime.run_intent = RunIntent::Stopped;
        let next = mailbox.take_next(|| Command::Shutdown).unwrap();
        assert!(matches!(
            mailbox.admit_round(&mut runtime, next),
            RoundAdmission::Skipped
        ));
        assert!(mailbox.active.is_none());
        assert_eq!(mailbox.status.lock().completed.len(), 1);
        assert!(mailbox.status.lock().completed[0].error.is_none());
    }

    #[test]
    fn release_is_only_admitted_for_converged_running_owner() {
        let mut mailbox = mailbox();
        let mut runtime = RuntimeReconcilerState::default();
        let pending = AtomicBool::new(false);
        assert!(mailbox.release_allowed(&runtime, &pending, &Conclusion::Converged));
        assert!(!mailbox.release_allowed(&runtime, &pending, &Conclusion::Inactive));
        runtime.run_intent = RunIntent::Stopped;
        assert!(!mailbox.release_allowed(&runtime, &pending, &Conclusion::Converged));
        runtime.run_intent = RunIntent::Running;
        pending.store(true, Ordering::Release);
        assert!(!mailbox.owner_followup_allowed(&runtime, &pending));
        pending.store(false, Ordering::Release);
        mailbox
            .startup_owner_unproven_shared
            .store(true, Ordering::Release);
        assert!(!mailbox.owner_followup_allowed(&runtime, &pending));
        mailbox.set_owner_unproven(false);
        assert!(mailbox.owner_followup_allowed(&runtime, &pending));
        mailbox.uncertain = true;
        assert!(!mailbox.owner_followup_allowed(&runtime, &pending));
    }

    #[test]
    fn release_debt_waits_for_explicit_retry_after_queue_capacity_failure() {
        let mut mailbox = mailbox();
        let mut runtime = RuntimeReconcilerState::default();
        for _ in 0..MAX_PENDING {
            assert!(mailbox.push_background(Command::Reconcile));
        }
        mailbox.observe_release(&mut runtime, ReleaseObservation::Owed, false);
        assert_eq!(mailbox.pending.len(), MAX_PENDING);
        assert!(
            runtime
                .service_release_debt
                .maintenance()
                .unwrap()
                .contains("queue is full")
        );
        mailbox.pending.clear();
        mailbox.observe_release(&mut runtime, ReleaseObservation::Owed, false);
        assert!(mailbox.pending.is_empty());
        mailbox.observe_release(&mut runtime, ReleaseObservation::Owed, true);
        assert!(matches!(
            &mailbox.pending.front().unwrap().command,
            Command::MaintainServiceRelease
        ));
        mailbox.observe_release(&mut runtime, ReleaseObservation::Owed, true);
        assert_eq!(mailbox.pending.len(), 1);
    }

    #[test]
    fn deferred_retry_retains_exact_target_and_operation_identity() {
        let mut mailbox = mailbox();
        assert!(mailbox.push_deferred_retry("digest-a".into()));
        let request = mailbox.pending.pop_front().unwrap();
        assert!(
            matches!(&request.command, Command::RetryDeferredProfiles { digest } if digest == "digest-a")
        );
        let dispatch = prepare_round(&request, false);
        assert_eq!(dispatch.plan.id, request.response.id);
        assert_eq!(dispatch.plan.deferred_retry.as_deref(), Some("digest-a"));
        assert_eq!(dispatch.trigger, "Tick");
        assert!(dispatch.plan.reconciled);
        assert!(!dispatch.plan.service_mutation);
    }

    #[test]
    fn stopped_and_source_only_commands_do_not_claim_runtime_reconciliation() {
        let shutdown = prepare_round(&request(Command::Shutdown), false);
        assert!(shutdown.plan.shutdown);
        assert!(!shutdown.plan.reconciled);
        let stop = prepare_round(&request(Command::StopCore), false);
        assert!(!stop.plan.reconciled);
        let source = request(Command::CacheStartupUnsettled {
            operation_id: 42,
            reason: "owner unknown".into(),
        });
        assert_eq!(request_kind(&source.command), RequestKind::SourceOnly);
        let dispatch = prepare_round(&source, false);
        assert!(!dispatch.plan.reconciled);
        assert!(!dispatch.plan.service_mutation);
    }
}

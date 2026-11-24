//! Runtime application and lifecycle operations share one actor mailbox.
//! The desktop compatibility client is re-exported by core_lifecycle.
use std::{
    collections::VecDeque,
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

#[cfg(test)]
use crate::client::reconciler::apply::checked_mode_effect_can_finish;
#[cfg(test)]
use crate::client::reconciler::checked::{CheckedRuntimeInbox, MAX_CHECKED_PENDING};
use crate::client::reconciler::{
    checked::{CheckedRuntimeCandidate, CheckedSelection},
    committed::{SourceDomain, SourceVersions},
    release::ReleaseObservation,
    status::ReconcilerStatus,
    workflow::{
        CheckedSourceNoticeKind, RuntimeAdmission, RuntimeReconcilerState,
        checked_mode_effect_deferred, committed_target_advanced, deferred_mode_effect_degradation,
        runtime_trigger, source_committed_during_round,
    },
};
use chimera_core::client::reconciler::plan::RunIntent;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[cfg(test)]
use crate::client::application_workflow::{
    startup::{StartupOutcome, StartupWorkflowState},
    workflow::ProfileRuntime,
};
#[cfg(test)]
use crate::client::core_lifecycle::{BinaryInstallProgress, LegacyServiceBridge};
use crate::client::core_lifecycle::{
    Command, CoreLifecyclePort, CoreLifecycleWorkflow, CoreStatusSnapshot, PreparedCoreBinary,
    RuntimeTransformDiagnostics, ServiceLifecyclePort, ServiceTransitionLease, ports, workflow,
};
use crate::client::{
    ChimeraClient, application::ApplicationClient, application_workflow::startup::StartupReport,
    application_workflow::workflow::ApplicationWorkflow, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};

const CALL_WAIT: Duration = Duration::from_secs(180);
const DIRTY_WINDOW: Duration = Duration::from_millis(500);
use chimera_core::client::core_lifecycle::RECOVERY_INTERVAL;
const STARTUP_RECOVERY_LEASE_ATTEMPTS: usize = 3;
const MAX_PENDING: usize = 32;
const COMPLETED_LIMIT: usize = 32;

type OperationId = u64;

pub(in crate::client) struct CompletedLifecycleOperation {
    pub(in crate::client) result: anyhow::Result<()>,
    pub(in crate::client) report: StartupReport,
}

/// An obsolete Checked apply can have changed the actual Core process while
/// another Source committed. Do not publish its stale port binding as the
/// current desired Effects state; the sole Actor must first reconcile latest.
/// Service/Shutdown observations remain independent of runtime convergence.
fn refresh_effect_ports(
    reconciled: bool,
    service_observed: bool,
    shutdown: bool,
    source_advanced: bool,
) -> bool {
    (service_observed || shutdown) || (reconciled && !source_advanced)
}

fn checked_clash_notice(
    checked_clash: bool,
    result: &anyhow::Result<()>,
    verified_running: bool,
) -> Option<Result<(), String>> {
    checked_clash.then(|| {
        result
            .as_ref()
            .map_err(|error| format!("{error:#}"))
            .and_then(|_| {
                verified_running.then_some(()).ok_or_else(|| {
                    "Clash source committed, but active Core apply was not verified".into()
                })
            })
    })
}

fn completed_from_startup_report(report: StartupReport) -> CompletedLifecycleOperation {
    let result = report
        .outcome
        .failure_reason()
        .map_or(Ok(()), |reason| Err(anyhow::anyhow!(reason.to_owned())));
    CompletedLifecycleOperation { result, report }
}

fn schedule_startup_recovery(
    actor_ref: ActorRef<Message>,
    next_id: Arc<AtomicU64>,
    service: Arc<dyn ServiceLifecyclePort>,
    startup_recovery_pending: Arc<AtomicBool>,
    is_needed: impl Fn() -> bool + Send + Sync + 'static,
) {
    if startup_recovery_pending
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    tokio::spawn(async move {
        let result = async {
            for attempt in 0..STARTUP_RECOVERY_LEASE_ATTEMPTS {
                // Acquire the host lease before asking the serialized actor to
                // run recovery, preserving the global host-lock/mailbox order.
                let transition = match service.begin_transition().await {
                    Ok(transition) => transition,
                    Err(error) if attempt + 1 < STARTUP_RECOVERY_LEASE_ATTEMPTS => {
                        tracing::debug!(%error, "startup owner recovery lease busy; retrying");
                        tokio::time::sleep(RECOVERY_INTERVAL).await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if !is_needed() {
                    return Ok(());
                }
                let operation_id = next_id.fetch_add(1, Ordering::Relaxed);
                return match actor_ref
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
                    CallResult::Timeout => Err(anyhow::anyhow!("startup owner recovery timed out")),
                    CallResult::SenderError => {
                        Err(anyhow::anyhow!("startup owner recovery reply was dropped"))
                    }
                };
            }
            unreachable!("startup recovery lease retry loop always returns or exhausts")
        }
        .await;
        startup_recovery_pending.store(false, Ordering::Release);
        // A source commit may reach the mailbox after RecoverStartup has
        // settled but before the caller's source observation. Only a
        // successfully resolved Startup dependency can wake its retained
        // source: a lease failure or timed-out recovery grants no authority
        // to proceed, even if the shared owner flag changed meanwhile.
        let _ = actor_ref.cast(Message::StartupRecoveryFinished {
            succeeded: result.is_ok(),
        });
        if let Err(error) = result {
            tracing::warn!(%error, "automatic startup owner recovery did not complete");
        }
    });
}

fn is_source_only_application_patch(command: &Command) -> bool {
    matches!(
        command,
        Command::PatchApplicationConfig {
            patch,
            service_connected,
            ..
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
    let readiness_changed = previous.phase.readiness() != current.phase.readiness()
        || current.ready_generation != previous.ready_generation;
    let owner_evidence_changed = owner_unproven
        && crate::client::application_workflow::startup::ServiceEvidence::from_status(previous)
            != crate::client::application_workflow::startup::ServiceEvidence::from_status(current);
    readiness_changed || owner_evidence_changed
}

/// A service watch can republish an unchanged daemon observation on each poll.
/// Every redundant message otherwise enters the sole CoreLifecycle mailbox,
/// where service follow-up performs async Core/source reads and can starve
/// Checked admission. Match ref's evidence-driven Service ownership planning:
/// only changes that can affect a host handoff, generation, or retry are events.
///
/// Deliberately compare projected *semantic evidence*, not the raw server DTO
/// (which can contain volatile reporting fields unrelated to Core ownership).
fn service_followup_evidence_changed(
    previous: &ServiceHostStatus,
    current: &ServiceHostStatus,
) -> bool {
    previous.phase != current.phase
        || previous.ready_generation != current.ready_generation
        || previous.settled_ready != current.settled_ready
        || previous.status != current.status
        || previous.runtime_owned != current.runtime_owned
        || previous.compat != current.compat
        || previous.restart_attempts != current.restart_attempts
        || crate::client::application_workflow::startup::ServiceEvidence::from_status(previous)
            != crate::client::application_workflow::startup::ServiceEvidence::from_status(current)
}

enum Message {
    /// Pinned ref RuntimeReconciler::Message::Checked, kept in the existing
    /// single CoreLifecycle mailbox until its prepared-apply path is ported.
    Checked {
        checked: CheckedRuntimeCandidate,
        reply: RpcReplyPort<Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>>>,
    },
    /// A checked candidate was never persisted; revoke its domain before
    /// the next durable SourceCommitted can consume stale uncommitted bytes.
    AbortChecked {
        domain: SourceDomain,
        request: Option<String>,
    },
    /// Install the single production ref CandidateCheck on the existing
    /// lifecycle workflow before any Application source mutation can run.
    InstallCandidateGate {
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
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
        service_endpoint_recovery: bool,
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
    ServiceReadiness,
    ServiceProbeFailed,
    ServiceRecoveryTick,
    ServiceRecoveryLeaseFailed,
    RecoverCore,
    /// Ref RuntimeGate::committed: observe the durable source version.
    /// Profiles requests run a latest-target reconciliation here, while
    /// already-applied legacy domains may only record their source version.
    SourceCommitted {
        domain: SourceDomain,
        version: u64,
        reconcile_requested: bool,
        /// The caller already observed a ref Service generation / owner that
        /// requires Startup recovery. Retain the durable runtime intent while
        /// that single owner is being re-established.
        startup_recovery_required: bool,
    },
    /// Ref RuntimeGate::committed for an already Checked product. Unlike the
    /// old SourceCommitted route, this must NEVER schedule an independent
    /// legacy build for a candidate it successfully consumes.
    CheckedSourceCommitted {
        domain: SourceDomain,
        version: u64,
        request: Option<String>,
    },
    RuntimeDirty,
    DirtyTick,
    /// A committed checked value can only be dispatched from the sole Owner
    /// mailbox after observing the matching source version. Not connected to
    /// the production Source Gate until the authoritative latest-target read
    /// is also integrated.
    ApplyCheckedCommitted {
        domain: SourceDomain,
        version: u64,
        target: String,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    #[cfg(test)]
    ReadSourceVersionsForTest {
        reply: RpcReplyPort<SourceVersions>,
    },
    #[cfg(test)]
    ReadCheckedForTest {
        reply: RpcReplyPort<Option<(Option<String>, String, Vec<u8>)>>,
    },
    #[cfg(test)]
    ReadAppliedSelectionForTest {
        reply: RpcReplyPort<Option<crate::client::reconciler::committed::Selection>>,
    },
    StartupRecoveryFinished {
        succeeded: bool,
    },
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

struct RuntimeReconcilerActor;

struct RuntimeReconcilerMailbox {
    reconciler: tokio::sync::watch::Sender<ReconcilerStatus>,
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
    runtime: RuntimeReconcilerState,
    source_authority: Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    service_recovery_inflight: bool,
    service_recovery_timer: tokio::task::JoinHandle<()>,
    startup_recovery_pending: Arc<AtomicBool>,
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
    trigger: &'static str,
    checked_clash: bool,
    /// Checked Apply already promotes its own exact verified selection.
    /// Legacy startup/reconcile must recover it from the confirmed receipt.
    checked_apply: bool,
    observed_sources: SourceVersions,
}

impl RuntimeReconcilerMailbox {
    fn allocate_operation_id(&self) -> OperationId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
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

    fn publish_service_phase(&self, phase: ServicePhase) {
        if self.publishes_legacy_service_status() {
            self.service_status.publish_phase(phase);
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
        if self.runtime.run_intent == RunIntent::Stopped || self.uncertain || self.shutting_down {
            return;
        }
        self.runtime.dirty = true;
        if !self.runtime.dirty_tick_scheduled {
            self.runtime.dirty_tick_scheduled = true;
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

    fn request_startup_recovery(&self, myself: &ActorRef<Message>) {
        let startup_owner_unproven = self.startup_owner_unproven_shared.clone();
        schedule_startup_recovery(
            myself.clone(),
            self.next_id.clone(),
            self.service.clone(),
            self.startup_recovery_pending.clone(),
            move || startup_owner_unproven.load(Ordering::Acquire),
        );
    }

    /// A handoff failure and a degraded Service endpoint are independent
    /// maintenance debts; keep both even when a Core round converged.
    fn publish_maintenance(&self) {
        let product = self.reconciler.borrow().product_publication_maintenance();
        let reason = self.runtime.maintenance(product.as_deref());
        self.reconciler
            .send_modify(|status| status.set_maintenance(reason));
    }

    /// Shared S2 admission barrier. A Startup recovery worker may hold the
    /// Service transition lease *outside* the CoreLifecycle mailbox; startup
    /// and follow-up owner commands may also be queued behind another action.
    /// Neither a readiness event nor a source notification can infer an
    /// established owner before those transitions have settled.
    fn owner_transition_pending(&self) -> bool {
        self.startup_recovery_pending.load(Ordering::Acquire)
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

    /// Evaluate release debt after a proven runtime round or a Service
    /// observation. This never starts a second Core controller: the actual
    /// daemon stop is admitted as its own serialized lifecycle command.
    async fn schedule_service_release(&mut self, explicit_retry: bool) {
        if self.runtime.run_intent == RunIntent::Stopped
            || self.uncertain
            || self.shutting_down
            || self.active.is_some()
            || self.startup_owner_unproven
            || self.startup_owner_unproven_shared.load(Ordering::Acquire)
            || self.owner_transition_pending()
            || !matches!(
                self.reconciler.borrow().conclusion,
                chimera_core::client::reconciler::plan::Conclusion::Converged
            )
        {
            return;
        }
        let Some(workflow) = self.workflow.as_ref() else {
            return;
        };
        let observation = workflow.lifecycle().service_release_observation().await;
        self.runtime.service_release_debt.observe(observation);
        if observation != ReleaseObservation::Owed {
            self.publish_maintenance();
            return;
        }
        if !self.runtime.service_release_debt.begin(explicit_retry) {
            self.publish_maintenance();
            return;
        }
        if self.pending.len() >= MAX_PENDING {
            self.runtime
                .service_release_debt
                .deferred("lifecycle queue is full".into());
            self.publish_maintenance();
            return;
        }
        self.push_background(Command::MaintainServiceRelease);
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
            || self.runtime.run_intent != RunIntent::Running
            || self.uncertain
            || self.shutting_down
            || self.startup_owner_unproven
            || self.startup_owner_unproven_shared.load(Ordering::Acquire)
            || self.active.is_some()
            || self.owner_transition_pending()
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
            && self.runtime.run_intent == RunIntent::Running
            && !self.uncertain
            && !self.shutting_down
            && !self.startup_owner_unproven
            && !self.startup_owner_unproven_shared.load(Ordering::Acquire)
            && self.active.is_none()
            && !self.owner_transition_pending()
        {
            self.mark_runtime_dirty(myself);
        }
    }

    async fn follow_service(&mut self) {
        if self.runtime.run_intent == RunIntent::Stopped
            || self.active.is_some()
            || self.uncertain
            || self.shutting_down
            || self.startup_owner_unproven
            || self.startup_owner_unproven_shared.load(Ordering::Acquire)
            || self.owner_transition_pending()
        {
            return;
        }
        let Some(workflow) = self.workflow.as_ref() else {
            return;
        };
        if !workflow.service_host_change_due().await {
            return;
        }
        let transition = match workflow.lifecycle().try_begin_service_transition().await {
            Ok(transition) => transition,
            Err(error) => {
                tracing::debug!(%error, "Service owner follow-up deferred while a transition is active");
                self.runtime.service_handoff_maintenance = Some(format!(
                    "Service ownership transition is waiting for its lease: {error:#}"
                ));
                self.publish_maintenance();
                return;
            }
        };
        if !workflow.service_host_change_due().await {
            return;
        }
        if self.pending.len() >= MAX_PENDING {
            tracing::warn!("dropping Service owner follow-up because the lifecycle queue is full");
            self.runtime.service_handoff_maintenance =
                Some("Service owner handoff is waiting for lifecycle queue capacity".into());
            self.publish_maintenance();
            return;
        }
        self.runtime.service_handoff_maintenance = Some("Service owner handoff is pending".into());
        self.publish_maintenance();
        let operation_id = self.allocate_operation_id();
        // Run this post-command owner reconciliation before requests already
        // queued behind the event that observed the readiness change. The
        // lease is held across the request, so allowing another queued Core
        // mutation to run first would invert the reference actor's order.
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

    fn reject(&mut self, mut request: Request, error: anyhow::Error) {
        if matches!(&request.command, Command::ServiceEndpointDown(_)) {
            self.service_recovery_inflight = false;
        }
        if let Command::ProfileMutation(mutation) = &mut request.command {
            let message = error.to_string();
            mutation.answer(
                crate::client::application_workflow::mutation::TryAck::Failed(message.clone()),
            );
            mutation.settle(Err(message));
        }
        if let Command::ReplaceCoreBinary(artifact) = &request.command {
            let message = error.to_string();
            // A UI progress observer is allowed to fail without making the
            // core mutation's result uncertain. This closure is synchronous.
            if chimera_panics::catch_recoverable(AssertUnwindSafe(|| {
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
        self.runtime.dirty = false;
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
                    | Command::ServiceEndpointDown(_)
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
        // A checked document that was never committed is not a running Core
        // receipt; discard it instead of carrying it past shutdown.
        self.runtime.checked.clear();
        self.runtime.clear_dirty();
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

        match self.runtime.admit(&request.command) {
            RuntimeAdmission::Run => {}
            RuntimeAdmission::RejectStoppedChecked => {
                self.reject(
                    request,
                    anyhow::anyhow!("checked runtime cannot apply while stopped"),
                );
                self.drive(myself);
                return;
            }
            RuntimeAdmission::SkipInactive => {
                self.settle(request.response, Ok(()));
                self.drive(myself);
                return;
            }
            RuntimeAdmission::RejectStaleChecked => {
                self.reject(
                    request,
                    anyhow::anyhow!(
                        "checked runtime source is no longer the latest committed version"
                    ),
                );
                self.drive(myself);
                return;
            }
        }

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
        let trigger = runtime_trigger(&request.command);
        let checked_apply = matches!(&request.command, Command::ApplyChecked { .. });
        let checked_clash = matches!(
            &request.command,
            Command::ApplyChecked {
                domain: SourceDomain::Clash,
                ..
            }
        );
        let observed_sources = self.runtime.seen_sources.clone();
        self.reconciler.send_modify(|status| status.begin(id));
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
                if crate::client::application_workflow::impact::application_patch_requires_reconcile(
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
        #[cfg(test)]
        let reconciled = reconciled || matches!(&request.command, Command::SelectCore(_));
        let service_phase = match &request.command {
            Command::InstallService(_) => Some(ServicePhase::Installing),
            Command::StartService { .. } => Some(ServicePhase::StartingDaemon),
            Command::RestartService { .. } => Some(ServicePhase::Restarting),
            Command::UninstallService(_) => Some(ServicePhase::Uninstalling),
            _ => None,
        };
        let publish_legacy_service = self.publishes_legacy_service_status();
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
        if let Some(phase) = service_phase {
            self.publish_service_phase(phase);
        }

        let command = request.command;
        let actor = myself.clone();
        let task = tokio::spawn(async move {
            let (result, workflow_panicked, request_runtime_rebuild) = match command {
                Command::ProfileMutation(mut mutation) => {
                    // Ref's recoverable panic flag must cover each future
                    // poll, not remain set across a Tokio .await. Preserve
                    // the uncertainty/recovery settlement on panic.
                    let run = chimera_panics::catch_recoverable_future(
                        workflow.run_profile_mutation(&mut mutation),
                    )
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
                            (Err::<(), _>(anyhow::anyhow!(message)), true, false)
                        }
                    }
                }
                command => {
                    match chimera_panics::catch_recoverable_future(
                        workflow.execute_lifecycle(command),
                    )
                    .await
                    {
                        Ok(Ok(request_runtime_rebuild)) => {
                            (Ok::<(), anyhow::Error>(()), false, request_runtime_rebuild)
                        }
                        Ok(Err(error)) => (Err(error), false, false),
                        Err(_) => {
                            let message =
                                "core lifecycle workflow panicked; execution state is uncertain";
                            workflow.mark_recovery_required(message.into()).await;
                            (
                                Err::<(), anyhow::Error>(anyhow::anyhow!(message)),
                                true,
                                false,
                            )
                        }
                    }
                }
            };
            if let Some(progress) = progress {
                let error = result.as_ref().err().map(ToString::to_string);
                // Do not let a synchronous notification callback override
                // the actual lifecycle outcome or trigger fatal panic UI.
                if chimera_panics::catch_recoverable(AssertUnwindSafe(|| {
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
            // Ref ServiceActor performs its own bounded probe and publishes
            // its watch after Service commands. Do not issue a second probe
            // merely to refresh CoreLifecycle's unused compatibility cache.
            let service_probe = if service_mutation && !workflow_panicked && publish_legacy_service
            {
                Some(
                    match chimera_panics::catch_recoverable_future(lifecycle.probe_service()).await
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
                    service_endpoint_recovery,
                    shutdown,
                    request_runtime_rebuild,
                    deferred_retry,
                    reconciled: reconciled
                        && runtime_converged
                            .as_ref()
                            .is_none_or(|converged| converged.load(Ordering::Acquire)),
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
            trigger,
            checked_clash,
            checked_apply,
            observed_sources,
        });
        self.publish();
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
        let service_recovery_timer =
            myself.send_interval(RECOVERY_INTERVAL, || Message::ServiceRecoveryTick);
        Ok(RuntimeReconcilerMailbox {
            reconciler: args.reconciler,
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
            runtime: RuntimeReconcilerState::default(),
            source_authority: args.source_authority,
            service_recovery_inflight: false,
            service_recovery_timer,
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
                let count = if let Some(request) = request.as_deref() {
                    state.runtime.checked.discard_request(domain, request)
                } else {
                    state.runtime.checked.discard_domain(domain)
                };
                tracing::debug!(
                    ?domain,
                    ?request,
                    count,
                    "discarded uncommitted checked candidates"
                );
            }
            Message::InstallCandidateGate { gate, reply } => {
                let result = if state.shutting_down {
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
                let result = if state.shutting_down || state.uncertain {
                    Err(Arc::new(
                        chimera_contracts::runtime_error::RuntimeError::ShuttingDown,
                    ))
                } else {
                    state.runtime.checked.accept(checked)
                };
                let _ = reply.send(result);
            }
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
                if matches!(&request.command, Command::ServiceEndpointDown(_)) {
                    state.service_recovery_inflight = true;
                }
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
                            | Command::ServiceEndpointDown(_)
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
                service_endpoint_recovery,
                shutdown,
                request_runtime_rebuild,
                deferred_retry,
                reconciled,
            } => {
                if state.active.as_ref().map(|operation| operation.response.id) == Some(id) {
                    // Source messages can be handled between the worker's
                    // final authoritative read and this completion. Detect
                    // that queued notification race *before* settling Effects
                    // or claiming latest-source convergence.
                    let superseded_in_mailbox = state.active.as_ref().is_some_and(|active| {
                        source_committed_during_round(
                            active.trigger,
                            &active.observed_sources,
                            &state.runtime.seen_sources,
                        )
                    });
                    let result = if superseded_in_mailbox && result.is_ok() {
                        Err(anyhow::Error::new(
                            crate::client::reconciler::attempt::CommittedTargetAdvanced,
                        )
                        .context("new committed source notice arrived during the Core operation"))
                    } else {
                        result
                    };
                    // Mailbox source advancement wins even if the earlier
                    // Core command already failed. Otherwise its refusal
                    // could poison a newer committed source's next retry.
                    let source_advanced =
                        superseded_in_mailbox || committed_target_advanced(&result);
                    // Consume the Core's genuine terminal once. The source
                    // notification fence below still decides whether this
                    // observed result can settle the current target.
                    let prepared_outcome = workflow.lifecycle().take_prepared_outcome();
                    // The lower Core can become uncertain independently of
                    // Result<()>; it must override any apparent Core success
                    // in both Attempt and Conclusion terminal classification.
                    let final_uncertain =
                        state.uncertain || workflow_panicked || lower_outcome_uncertain;
                    // A Checked worker may have promoted applied_selection
                    // after its second authoritative read, while the Actor
                    // already accepted a newer source notice. Completed is
                    // the final source-version fence: revoke that selection
                    // before subsequent mode changes inspect the baseline.
                    if source_advanced
                        && state
                            .active
                            .as_ref()
                            .is_some_and(|active| active.checked_apply)
                    {
                        workflow.lifecycle().invalidate_applied_selection();
                    }
                    let refresh_runtime_binding = refresh_effect_ports(
                        reconciled,
                        service_probe.is_some(),
                        shutdown,
                        source_advanced,
                    );
                    let runtime_binding = if refresh_runtime_binding {
                        Some(workflow.lifecycle().effect_port_binding().await)
                    } else {
                        None
                    };
                    #[cfg(feature = "e2e")]
                    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_binding_observed", "CoreLifecycle completion stage");
                    let active = state
                        .active
                        .take()
                        .expect("matched active lifecycle operation");
                    debug_assert_eq!(active.shutdown, shutdown);
                    // A Completed message precedes the task's return. Keep
                    // the workflow task owned until its JoinHandle settles;
                    // unexpected task panics must reach Actor supervision.
                    if let Err(error) = active.task.await
                        && let Ok(panic) = error.try_into_panic()
                    {
                        std::panic::resume_unwind(panic);
                    }
                    #[cfg(feature = "e2e")]
                    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_worker_joined", "CoreLifecycle completion stage");
                    let round = crate::client::reconciler::workflow::Round::observe(
                        workflow.lifecycle(),
                        &result,
                        active.trigger,
                        reconciled,
                        active.checked_apply,
                        source_advanced,
                        final_uncertain,
                        prepared_outcome.as_ref(),
                    )
                    .await;
                    let verified_running = round.verified_running;
                    if round.connection_effect_observed {
                        state.runtime.connection_interruption_maintenance = round
                            .connection_degradation
                            .as_ref()
                            .map(|degradation| degradation.message.clone());
                    }
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
                        state.runtime.service_recovery_maintenance =
                            result.as_ref().err().map(|error| {
                                format!("Service endpoint recovery is pending: {error:#}")
                            });
                    }
                    if active.trigger == "ServiceReadiness" {
                        state.runtime.service_handoff_maintenance = result
                            .as_ref()
                            .err()
                            .map(|error| format!("Service owner handoff is pending: {error:#}"));
                    }
                    if service_endpoint_recovery || active.trigger == "ServiceReadiness" {
                        state.publish_maintenance();
                    }
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
                    let needs_owner_handoff = result
                        .as_ref()
                        .err()
                        .and_then(|error| {
                            error.downcast_ref::<crate::client::reconciler::attempt::OwnershipTransitionPending>()
                        })
                        .is_some_and(|pending| pending.plan.is_some());
                    if matches!(
                        active.trigger,
                        "SourceCommitted" | "Retry" | "Start" | "CoreChanged"
                    ) && needs_owner_handoff
                    {
                        // The S2 decision was made inside the latest-source
                        // round. The single Actor queues its existing leased
                        // FollowService command; retain a one-shot S3/S4
                        // continuation even if no new source notification arrives.
                        state.runtime.source_after_handoff = true;
                    }

                    if let Some(error) = result.as_ref().err().and_then(|error| {
                        error.downcast_ref::<crate::client::reconciler::attempt::ServiceFallbackDeferred>()
                    }) {
                        state.runtime.service_handoff_maintenance = Some(error.to_string());
                        state.publish_maintenance();
                    } else if reconciled
                        && result.is_ok()
                        && state.runtime.service_handoff_maintenance
                            .as_deref()
                            .is_some_and(|reason| reason.starts_with("Service-to-Local fallback"))
                    {
                        state.runtime.service_handoff_maintenance = None;
                        state.publish_maintenance();
                    }
                    if active.trigger == "ServiceRelease" {
                        state.runtime.service_release_debt.finish(
                            result
                                .as_ref()
                                .map(|_| ())
                                .map_err(|error| format!("{error:#}")),
                        );
                        state.publish_maintenance();
                    }
                    round.settle(
                        state
                            .workflow
                            .as_ref()
                            .expect("completed workflow was restored")
                            .lifecycle(),
                        &state.reconciler,
                        active.response.id,
                        active.trigger,
                        &active.observed_sources,
                        state.runtime.run_intent,
                        reconciled,
                    );
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

                    #[cfg(feature = "e2e")]
                    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_retry_settled", "CoreLifecycle completion stage");
                    if (retry_reconcile_on_failure && failed || source_advanced)
                        && !state.uncertain
                        && !state.shutting_down
                    {
                        // A source superseded in flight is a *source*
                        // reconciliation, not a user Retry of obsolete bytes.
                        // Preserve the intent across an outstanding S2 lease
                        // and use the existing one-owner ReconcileSource path.
                        if source_advanced {
                            state.runtime.source_dirty = true;
                        }
                        state.mark_runtime_dirty(&myself);
                    }
                    if request_runtime_rebuild && !state.shutting_down {
                        if state.startup_owner_unproven
                            || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                        {
                            if !state.uncertain {
                                state.request_startup_recovery(&myself);
                            }
                        } else {
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
                    if recover && failed && !state.uncertain && !state.shutting_down {
                        tracing::error!("failed to recover core; scheduling retry");
                        let actor = myself.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            let _ = actor.cast(Message::RecoverCore);
                        });
                    }
                    state.follow_service().await;
                    state.rearm_deferred_dirty_after_owner(&myself);
                    // A failed S2 owner move retains the committed-source
                    // continuation. It may be repaired by a later startup
                    // recovery or explicit Start, not only FollowService.
                    // Consume it only after a successful verified runtime
                    // round, with a re-established and certain owner; the
                    // fresh S2/S3/S4 round must re-read durable sources.
                    if state.runtime.source_after_handoff
                        && reconciled
                        && !failed
                        && verified_running
                        && !state.uncertain
                        && !state.startup_owner_unproven
                        && !state.startup_owner_unproven_shared.load(Ordering::Acquire)
                        && state.runtime.run_intent == RunIntent::Running
                        && !state.shutting_down
                    {
                        state.runtime.source_after_handoff = false;
                        if active.trigger == "SourceCommitted" {
                            // A new latest-source round has *already* passed
                            // S2 and proved the current target. Do not enqueue
                            // an identical third round or leave a stale lease
                            // maintenance warning after external recovery.
                            state.runtime.service_handoff_maintenance = None;
                            state.publish_maintenance();
                        } else {
                            // A successful Start, FollowService or startup
                            // recovery needs one new committed-source round.
                            state.runtime.source_dirty = true;
                            state.mark_runtime_dirty(&myself);
                        }
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
                let can_recover = !state.uncertain
                    && !state.shutting_down
                    && state.active.is_none()
                    && state.pending.is_empty()
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
                    let operation_id = state.allocate_operation_id();
                    tokio::spawn(async move {
                        match service.begin_transition().await {
                            Ok(transition) => {
                                let request = Request {
                                    command: Command::ServiceEndpointDown(transition),
                                    response: Response {
                                        id: operation_id,
                                        reply: None,
                                    },
                                    deferred_retry: None,
                                };
                                if actor.cast(Message::Request(request)).is_err() {
                                    let _ = actor.cast(Message::ServiceRecoveryLeaseFailed);
                                }
                            }
                            Err(error) => {
                                tracing::debug!(%error, "Service recovery retry deferred while acquiring its transition lease");
                                let _ = actor.cast(Message::ServiceRecoveryLeaseFailed);
                            }
                        }
                    });
                }
            }
            Message::ServiceRecoveryLeaseFailed => {
                state.service_recovery_inflight = false;
                state.runtime.service_recovery_maintenance =
                    Some("Service endpoint recovery is waiting for a transition lease".into());
                state.publish_maintenance();
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
            Message::SourceCommitted {
                domain,
                version,
                reconcile_requested,
                startup_recovery_required,
            } => {
                // Domain owners have already published their source versions.
                // Profiles source commits request the one CoreLifecycle runtime
                // round; legacy inline domains pass false to avoid a second apply.
                // Always load the latest source, not the notification payload.
                state.runtime.seen_sources.observe(domain, version);
                if !reconcile_requested || state.runtime.run_intent == RunIntent::Stopped {
                    state.reconciler.send_modify(|status| {
                        status.observe_source_only(domain, version);
                    });
                }
                if reconcile_requested
                    && state.runtime.run_intent == RunIntent::Running
                    && !state.uncertain
                    && !state.shutting_down
                {
                    if startup_recovery_required
                        || state.startup_owner_unproven
                        || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                        || state.owner_transition_pending()
                    {
                        // Ref's source commit stays durable even when S2 must
                        // recover the initial owner first. Coalesce concurrent
                        // commits behind ONE latest-source continuation;
                        // successful verified Startup recovery will consume it.
                        state.runtime.source_after_handoff = true;
                        if state.startup_owner_unproven
                            || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                        {
                            state.request_startup_recovery(&myself);
                        } else if !state.owner_transition_pending() {
                            // The externally requested recovery may have
                            // settled before this source message was read.
                            // Replan through S2 rather than dropping its
                            // durable apply intent.
                            state.replan_retained_source_on_service_change(&myself);
                        }
                    } else {
                        state.runtime.source_dirty = true;
                        state.mark_runtime_dirty(&myself);
                    }
                }
            }
            Message::CheckedSourceCommitted {
                domain,
                version,
                request,
            } => {
                // This is a distinct source notification from the legacy
                // SourceCommitted variant: a checked receipt must either be
                // consumed by this single command queue or explicitly
                // deferred; never run its prepared bytes *and* a legacy build.
                let notice =
                    state
                        .runtime
                        .observe_checked_source(domain, version, request.as_deref());
                let previous = notice.previous;
                let mode_requested = notice.mode_requested;
                let mut checked_dispatched = false;
                state.reconciler.send_modify(|status| {
                    status.observe_source_only(domain, version);
                });
                if notice.kind == CheckedSourceNoticeKind::Duplicate {
                    tracing::debug!(?domain, version, "duplicate checked commit ignored");
                } else if notice.kind == CheckedSourceNoticeKind::Stale {
                    if state.runtime.run_intent == RunIntent::Running
                        && !state.uncertain
                        && !state.shutting_down
                    {
                        if state.startup_owner_unproven
                            || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                            || state.owner_transition_pending()
                        {
                            state.runtime.source_after_handoff = true;
                            if state.startup_owner_unproven
                                || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                            {
                                state.request_startup_recovery(&myself);
                            } else {
                                state.replan_retained_source_on_service_change(&myself);
                            }
                        } else {
                            state.runtime.source_dirty = true;
                            state.mark_runtime_dirty(&myself);
                        }
                    }
                    tracing::debug!(
                        ?domain,
                        version,
                        ?previous,
                        "stale checked receipt requires latest source recovery"
                    );
                } else if notice.kind == CheckedSourceNoticeKind::Superseded {
                    tracing::debug!(
                        ?domain,
                        version,
                        "ignoring superseded checked source notice"
                    );
                } else if state.runtime.run_intent == RunIntent::Stopped || state.shutting_down {
                    tracing::debug!(
                        ?domain,
                        version,
                        "checked commit observed while runtime stopped"
                    );
                } else if state.uncertain {
                    tracing::warn!(
                        ?domain,
                        version,
                        "checked commit deferred: Core outcome uncertain"
                    );
                } else if state.startup_owner_unproven
                    || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                    || state.owner_transition_pending()
                {
                    // A durable source is not discarded while S2 owns the
                    // Core; let the existing single-owner recovery replan the
                    // latest committed target after the host is proven.
                    state.runtime.source_after_handoff = true;
                    if state.startup_owner_unproven
                        || state.startup_owner_unproven_shared.load(Ordering::Acquire)
                    {
                        state.request_startup_recovery(&myself);
                    } else {
                        state.replan_retained_source_on_service_change(&myself);
                    }
                } else if state.pending.len() >= MAX_PENDING {
                    // No lossy background cast: keep a latest-source dirty
                    // intent with the only Core owner until queue capacity
                    // frees up. Never submit a candidate outside this Actor.
                    state.runtime.source_dirty = true;
                    state.mark_runtime_dirty(&myself);
                } else if let Some(authority) = state.source_authority.as_ref().cloned() {
                    let (selection, checked) = state
                        .runtime
                        .checked
                        .select_committed(domain, request.as_deref());
                    match (selection, checked) {
                        (
                            CheckedSelection::Matched,
                            Some(CheckedRuntimeCandidate {
                                prepared,
                                core_spec,
                                interrupt_mode,
                                ..
                            }),
                        ) => {
                            checked_dispatched = true;
                            let id = state.allocate_operation_id();
                            state.pending.push_back(Request {
                                command: Command::ApplyChecked {
                                    prepared,
                                    core_spec,
                                    domain,
                                    version,
                                    authority,
                                    interrupt_mode,
                                },
                                response: Response { id, reply: None },
                                deferred_retry: None,
                            });
                        }
                        (selection, _) => {
                            // No proof which of several same-domain checks
                            // became durable: do not guess from FIFO order.
                            // Keep unrelated-domain candidates, and let the
                            // *same* Core owner rebuild the latest source.
                            tracing::debug!(
                                ?domain,
                                ?selection,
                                "checked source requires latest-target recovery"
                            );
                            state.runtime.source_dirty = true;
                            state.mark_runtime_dirty(&myself);
                        }
                    }
                } else {
                    // No authoritative reader: an ACKed checked candidate
                    // must not become a Core submission. Retain it and
                    // request a latest-source recovery in the same owner.
                    state.runtime.source_dirty = true;
                    state.mark_runtime_dirty(&myself);
                }
                if checked_mode_effect_deferred(
                    previous,
                    version,
                    mode_requested,
                    checked_dispatched,
                    state.runtime.run_intent == RunIntent::Running && !state.shutting_down,
                ) {
                    // The old/recovery ReconcileSource may rebuild current
                    // bytes, but has no guaranteed old Core API/replaced
                    // result. Preserve the non-retryable one-shot effect debt
                    // visibly instead of marking the interruption complete.
                    let degradation = deferred_mode_effect_degradation(version);
                    state.runtime.connection_interruption_maintenance =
                        Some(degradation.message.clone());
                    state.reconciler.send_modify(|status| {
                        status.record_system_degradation(degradation);
                    });
                    state.publish_maintenance();
                }
            }
            Message::ApplyCheckedCommitted {
                domain,
                version,
                target,
                authority,
                reply,
            } => {
                // Explicit target dispatch is valid only for the uniquely
                // matching candidate in the specified source domain. Other
                // checked candidates remain cached, never overwritten.
                let valid = !state.shutting_down
                    && !state.uncertain
                    && state.runtime.run_intent == RunIntent::Running
                    && !state.startup_owner_unproven
                    && !state.startup_owner_unproven_shared.load(Ordering::Acquire)
                    && !state.owner_transition_pending()
                    && state.runtime.seen_sources.version(domain) == Some(version)
                    && state
                        .runtime
                        .checked
                        .matching_count(domain, Some(target.as_str()))
                        == 1;
                if !valid || state.pending.len() >= MAX_PENDING {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "checked candidate has no matching committed source or Core owner"
                    )));
                } else {
                    let (selection, checked) =
                        state.runtime.checked.take_unique(domain, Some(&target));
                    debug_assert_eq!(selection, CheckedSelection::Matched);
                    let CheckedRuntimeCandidate {
                        prepared,
                        core_spec,
                        interrupt_mode,
                        ..
                    } = checked.expect("uniquely matching checked candidate");
                    let id = state.allocate_operation_id();
                    state.pending.push_back(Request {
                        command: Command::ApplyChecked {
                            prepared,
                            core_spec,
                            domain,
                            version,
                            authority,
                            interrupt_mode,
                        },
                        response: Response {
                            id,
                            reply: Some(reply),
                        },
                        deferred_retry: None,
                    });
                }
            }
            Message::RuntimeDirty => {
                if state.runtime.run_intent == RunIntent::Stopped
                    || state.uncertain
                    || state.shutting_down
                {
                    tracing::debug!(
                        ?state.runtime.run_intent,
                        uncertain = state.uncertain,
                        shutting_down = state.shutting_down,
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
                state.runtime.dirty_tick_scheduled = false;
                if state.runtime.run_intent == RunIntent::Stopped
                    || state.uncertain
                    || state.shutting_down
                {
                    state.runtime.dirty = false;
                } else if state.owner_transition_pending() {
                    // Do not consume dirty/source_dirty or dispatch a runtime
                    // submit while Startup or FollowService owns the host.
                    // A resolved transition will rearm this same dirty state.
                } else if state.pending.len() >= MAX_PENDING {
                    // Do not consume a durable source apply intent until its
                    // serialized Core command can actually be admitted.
                    // The next completed lifecycle command will rearm this
                    // dirty tick as queue capacity becomes available.
                    tracing::debug!("deferring runtime dirty while lifecycle queue is full");
                } else if state.runtime.dirty {
                    state.runtime.dirty = false;
                    let source_round = std::mem::take(&mut state.runtime.source_dirty);
                    state.push_background(if source_round {
                        Command::ReconcileSource
                    } else {
                        Command::Reconcile
                    });
                }
            }
            Message::StartupRecoveryFinished { succeeded } => {
                if succeeded {
                    state.replan_retained_source_on_service_change(&myself);
                    state.rearm_deferred_dirty_after_owner(&myself);
                }
            }
            Message::StartupRetryDue { identity } => {
                // A timer armed before explicit StopCore must not take a
                // Service transition lease after RunIntent becomes Stopped.
                // The serialized RetryStartup command has its own stop gate,
                // but that alone cannot undo an already acquired lease.
                if state.runtime.run_intent == RunIntent::Running
                    && !state.uncertain
                    && !state.shutting_down
                {
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

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.service_recovery_timer.abort();
        // The workflow (and its transaction/Service lease) is moved into the
        // active task. An unexpected Actor stop cannot drop that owner while
        // it might still be mutating Core or committing a profile decision.
        // The outcome is unproven because its Completed message was not
        // handled; do not publish a successful runtime conclusion.
        if let Some(active) = state.active.take() {
            state.uncertain = true;
            state
                .startup_owner_unproven_shared
                .store(true, Ordering::Release);
            {
                let mut status = state.status.lock();
                status.active = None;
                status.uncertain = true;
                status.shutting_down = true;
            }
            if let Err(error) = active.task.await {
                match error.try_into_panic() {
                    Ok(panic) => std::panic::resume_unwind(panic),
                    Err(error) => tracing::warn!(%error, "core lifecycle owner task cancelled"),
                }
            }
        }
        state.runtime.checked.clear();
        state.reconciler.send_modify(ReconcilerStatus::close);
        Ok(())
    }
}

enum ClientInner {
    Actor {
        actor_ref: ActorRef<Message>,
        next_id: Arc<AtomicU64>,
        status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
        reconciler: tokio::sync::watch::Receiver<ReconcilerStatus>,
        service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        core: Arc<dyn CoreLifecyclePort>,
        startup_owner_unproven: Arc<AtomicBool>,
        service_failed_generation: Arc<AtomicU64>,
        startup_recovery_pending: Arc<AtomicBool>,
        /// The source verifier comes only from the composition root's
        /// committed Application, Clash and Profiles snapshots plus files.
        source_authority:
            Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
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
                            clash.clone(),
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
        let client = Self(Arc::new(ClientInner::Actor {
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
        }));
        #[cfg(not(test))]
        {
            let recovery_client = client.clone();
            let mut status_updates = service_status_rx.clone();
            let recovery_shutdown = shutdown.child_token();
            tasks.spawn(async move {
                if let Some(mut ref_updates) = service_actor_updates {
                    // Production startup recovery observes the *same* ref
                    // ServiceActor watch that owns readiness generation and
                    // probe phase. CoreLifecycle's legacy watch remains only
                    // for its not-yet-migrated workflow commands.
                    let mut previous = crate::client::service_status::project_actor_service_status(
                        &ref_updates.borrow_and_update(),
                    );
                    loop {
                        tokio::select! {
                            biased;
                            _ = recovery_shutdown.cancelled() => break,
                            changed = ref_updates.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                                let current = crate::client::service_status::project_actor_service_status(
                                    &ref_updates.borrow_and_update(),
                                );
                                let evidence_changed = startup_recovery_evidence_changed(
                                    &previous,
                                    &current,
                                    startup_owner_unproven_signal.load(Ordering::Acquire),
                                );
                                let followup_changed =
                                    service_followup_evidence_changed(&previous, &current);
                                previous = current;
                                if followup_changed
                                    && service_followup_actor.cast(Message::ServiceReadiness).is_err()
                                {
                                    break;
                                }
                                if evidence_changed && recovery_client.startup_recovery_needed() {
                                    recovery_client.request_startup_recovery();
                                }
                            }
                        }
                    }
                } else {
                    // Injected compatibility port without a ref ServiceActor.
                    let mut previous = status_updates.borrow_and_update().clone();
                    loop {
                        tokio::select! {
                            biased;
                            _ = recovery_shutdown.cancelled() => break,
                            changed = status_updates.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                                let current = status_updates.borrow_and_update().clone();
                                let evidence_changed = startup_recovery_evidence_changed(
                                    &previous,
                                    &current,
                                    startup_owner_unproven_signal.load(Ordering::Acquire),
                                );
                                let followup_changed =
                                    service_followup_evidence_changed(&previous, &current);
                                previous = current;
                                if followup_changed
                                    && service_followup_actor.cast(Message::ServiceReadiness).is_err()
                                {
                                    break;
                                }
                                if evidence_changed && recovery_client.startup_recovery_needed() {
                                    recovery_client.request_startup_recovery();
                                }
                            }
                        }
                    }
                }
            });
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
            ClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| Message::InstallCandidateGate { gate, reply },
                        Some(CALL_WAIT),
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => anyhow::bail!("candidate gate installation timed out"),
                    CallResult::SenderError => {
                        anyhow::bail!("candidate gate installation reply dropped")
                    }
                }
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                anyhow::bail!("production CandidateCheck requires the lifecycle actor")
            }
        }
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
            ClientInner::Actor { actor_ref, .. } => {
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

    pub(crate) fn begin_profile_mutation(
        &self,
        request: crate::client::application_workflow::mutation::MutationRequest,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor {
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
            ClientInner::Actor {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
            ClientInner::Actor { actor_ref, .. } => {
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
            ClientInner::Direct { workflow, .. } => Ok(workflow.lock().await.startup_report()),
        }
    }

    async fn startup_reconcile_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        match self.0.as_ref() {
            ClientInner::Actor {
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
            ClientInner::Actor {
                startup_owner_unproven,
                service_failed_generation,
                service_status,
                service,
                core,
                ..
            } => {
                !core.outcome_uncertain()
                    && (startup_owner_unproven.load(Ordering::Acquire) || {
                        let failed_generation = service_failed_generation.load(Ordering::Acquire);
                        // The failed generation was recorded from the ref
                        // ServiceActor during startup. Compare only with that
                        // same Actor's generation, not CoreLifecycle's
                        // independently incremented compatibility watch.
                        let current = service
                            .actor_status()
                            .unwrap_or_else(|| service_status.borrow().clone());
                        failed_generation != 0
                            && current.ready_generation != failed_generation
                            && current.phase.readiness() == Some(true)
                    })
            }
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
            ClientInner::Actor { core, .. } => core.runtime_preparation_session(),
            #[cfg(test)]
            ClientInner::Direct { core, .. } => core.runtime_preparation_session(),
        }
    }

    pub(crate) fn status(&self) -> CoreLifecycleStatus {
        match self.0.as_ref() {
            ClientInner::Actor { status, .. } => status.lock().clone(),
            #[cfg(test)]
            ClientInner::Direct { .. } => CoreLifecycleStatus::default(),
        }
    }

    /// Ref-shaped outcome projection; unlike the legacy lifecycle status,
    /// this carries the last sampled source versions and runtime conclusion.
    pub(crate) fn reconciler_status(&self) -> ReconcilerStatus {
        match self.0.as_ref() {
            ClientInner::Actor { reconciler, .. } => reconciler.borrow().clone(),
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
            ClientInner::Actor {
                source_authority: Some(port),
                ..
            } => port.clone(),
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
            anyhow::bail!("checked runtime apply requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::ApplyCheckedCommitted {
                    domain,
                    version,
                    target,
                    authority,
                    reply,
                },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("checked runtime apply outcome is unknown"),
            CallResult::SenderError => {
                anyhow::bail!("checked runtime apply reply was dropped; outcome unknown")
            }
        }
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
        use chimera_contracts::runtime_error::RuntimeError;
        match self.0.as_ref() {
            ClientInner::Actor { actor_ref, .. } => {
                let operation_id = chimera_core_manager::OperationId::generate();
                match actor_ref
                    .call(
                        |reply| Message::Checked {
                            checked: CheckedRuntimeCandidate {
                                domain,
                                request,
                                interrupt_mode,
                                prepared,
                                core_spec,
                            },
                            reply,
                        },
                        None,
                    )
                    .await
                {
                    Ok(CallResult::Success(result)) => result,
                    // The message never entered the Actor, so no candidate
                    // could have been accepted.
                    Err(ractor::MessagingErr::SendErr(Message::Checked { .. })) => {
                        Err(Arc::new(RuntimeError::ShuttingDown))
                    }
                    // A delivered request without an ACK may have been
                    // accepted. Do not turn an uncertain ownership outcome
                    // into a definitely closed owner.
                    _ => Err(Arc::new(RuntimeError::OwnerUnresponsive {
                        operation_id: operation_id.to_string(),
                    })),
                }
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { actor_ref, .. } = self.0.as_ref() else {
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
        let ClientInner::Actor { reconciler, .. } = self.0.as_ref() else {
            return ReconcilerStatus::default();
        };
        let mut watch = reconciler.clone();
        loop {
            let current = watch.borrow_and_update().clone();
            if current.settled_for(domain, version) {
                return current;
            }
            if watch.changed().await.is_err() {
                return watch.borrow().clone();
            }
        }
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
            ClientInner::Actor { actor_ref, .. } => {
                if actor_ref
                    .cast(Message::AbortChecked { domain, request })
                    .is_err()
                {
                    tracing::debug!(?domain, "checked abort skipped: Core owner is unavailable");
                }
            }
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
            ClientInner::Actor { actor_ref, .. } => {
                if actor_ref
                    .cast(Message::CheckedSourceCommitted {
                        domain,
                        version,
                        request,
                    })
                    .is_err()
                {
                    tracing::warn!(
                        ?domain,
                        version,
                        "checked source notice could not reach Core owner"
                    );
                }
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
            ClientInner::Actor { actor_ref, .. } => {
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
            ClientInner::Actor { actor_ref, .. } => {
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
            ClientInner::Actor {
                actor_ref,
                next_id,
                service,
                startup_recovery_pending,
                ..
            } => (actor_ref, next_id, service, startup_recovery_pending),
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
            ClientInner::Actor {
                actor_ref,
                next_id,
                service,
                ..
            } => (actor_ref.clone(), next_id.clone(), service.clone()),
            #[cfg(test)]
            ClientInner::Direct { .. } => return,
        };

        if !still_current() {
            return;
        }

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

    pub(in crate::client) fn service_status(&self) -> ServiceHostStatus {
        match self.0.as_ref() {
            ClientInner::Actor { service_status, .. } => service_status.borrow().clone(),
            #[cfg(test)]
            ClientInner::Direct { service_status, .. } => service_status.borrow().clone(),
        }
    }

    pub(in crate::client) fn observe_service_status(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
    ) {
        if let ClientInner::Actor {
            actor_ref, service, ..
        } = self.0.as_ref()
            && service.actor_status().is_none()
            && actor_ref.cast(Message::ObserveService { info }).is_err()
        {
            tracing::debug!("failed to publish legacy service status observation");
        }
    }

    pub(in crate::client) fn observe_service_probe_failure(&self) {
        if let ClientInner::Actor {
            actor_ref, service, ..
        } = self.0.as_ref()
            && service.actor_status().is_none()
            && actor_ref.cast(Message::ServiceProbeFailed).is_err()
        {
            tracing::debug!("failed to publish legacy service probe failure");
        }
    }

    #[cfg(test)]
    pub(in crate::client) async fn probe_service(&self) -> anyhow::Result<ServiceHostStatus> {
        match self.0.as_ref() {
            ClientInner::Actor { actor_ref, .. } => {
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
            ClientInner::Actor {
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
            ClientInner::Actor { core, .. } => core.status().await,
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
            ClientInner::Actor { service, .. } => service.clone(),
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
        self.inner.core.status().await
    }

    pub(crate) fn service_status(&self) -> ServiceHostStatus {
        if let Some(service) = &self.inner.service_actor {
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
        self.inner.core.runtime_transform_diagnostics()
    }

    pub(crate) fn promoted_runtime_snapshot(
        &self,
    ) -> Option<Arc<crate::client::runtime::RuntimeSnapshot>> {
        self.inner.core.promoted_runtime_snapshot()
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

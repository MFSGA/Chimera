//! StartupReconcile and owner reestablishment orchestration for ApplicationWorkflow.
//!
//! CoreLifecycleWorkflow supplies typed Core/Service adapter operations.
//! Startup evidence and owner decisions live here so setup cannot silently use
//! ordinary reconciliation when another host may still own a core.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
use crate::client::convergence::{OutcomeClass, RetryBudget, next_wait};
use crate::client::{
    application_workflow::{
        attempt::{AttemptOrigin, AttemptStage, LiveAttempt},
        mutation::{DeferredTarget, ReestablishCause, RuntimeCheckFailure, TargetOrigin},
        policy::TryCauseKind,
    },
    convergence::ConvergenceHealth,
    core_lifecycle::{CoreLifecycleWorkflow, ServiceTransitionLease, ports::CoreStatusSnapshot},
};
use chimera_core::{
    actor_v2::{
        endpoint::ExecutionHost, service_policy::ServicePhase, service_status::ServiceHostStatus,
    },
    client::reconciler::plan::{
        OwnerPlan, ServiceEvidence as ReconcilerServiceEvidence, plan_owner as reconcile_plan_owner,
    },
};
use chimera_core_manager::{CoreError, CoreErrorKind, CoreOwner, Epoch, OperationId, RevisionId};
use chimera_ipc::{
    api::status::{ConfigRevisionInfo, CoreState},
    types::ServiceStatus,
};

use super::workflow::ApplicationWorkflow;

pub(crate) fn startup_failure_is_retryable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.retryable)
            || cause
                .downcast_ref::<RuntimeCheckFailure>()
                .is_some_and(|failure| failure.cause() == TryCauseKind::Transient)
    })
}

pub(crate) fn startup_failure_waits_for_dependency(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<RuntimeCheckFailure>()
            .is_some_and(|failure| {
                matches!(
                    failure.record(),
                    super::mutation::CheckRecord::Unserviceable(_)
                )
            })
    })
}

pub(crate) fn startup_stop_error_is_idempotent(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::NotStarted))
    })
}

fn host_for_run_type(
    run_type: crate::core::RunType,
) -> crate::core::actor_v2::control_endpoint::ExecutionHost {
    use crate::core::{RunType, actor_v2::control_endpoint::ExecutionHost};

    match run_type {
        RunType::Service => ExecutionHost::Service,
        RunType::Normal | RunType::Elevated => ExecutionHost::Local,
    }
}

fn runtime_evidence(status: &CoreStatusSnapshot) -> RuntimeEvidence {
    use crate::core::actor_v2::control_endpoint::ExecutionHost;

    let host = host_for_run_type(status.run_type);
    RuntimeEvidence {
        host,
        state: (host != ExecutionHost::Service || status.state_changed_at > 0)
            .then(|| status.state.clone()),
        state_changed_at: status.state_changed_at,
    }
}

#[derive(Debug)]
pub(crate) enum StartupOwnerFailure {
    WaitingDependency(String),
    Blocked(String),
    RecoveryRequired(String),
}

impl StartupOwnerFailure {
    pub(crate) fn outcome(self) -> StartupOutcome {
        match self {
            Self::WaitingDependency(reason) => StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::WaitingDependency,
                reason,
            },
            Self::Blocked(reason) => StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason,
            },
            Self::RecoveryRequired(reason) => StartupOutcome::RecoveryRequired { reason },
        }
    }

    pub(crate) fn stop_rejected(error: anyhow::Error, uncertain: bool) -> Self {
        let reason = format!("the Core owner could not be stopped: {error:#}");
        if uncertain {
            Self::RecoveryRequired(reason)
        } else if startup_failure_is_retryable(&error) {
            Self::WaitingDependency(reason)
        } else {
            Self::Blocked(reason)
        }
    }
}

/// What StartupReconcile observed and how it concluded.
#[derive(Debug, Clone)]
pub(crate) struct StartupReport {
    /// The CoreLifecycle actor's operation sequence number.
    pub(crate) operation_id: u64,
    /// `None` when the workflow could not obtain a trustworthy observation.
    pub(crate) observation: Option<StartupObservation>,
    pub(crate) outcome: StartupOutcome,
}

/// Startup state owned by ApplicationWorkflow. The owner signal is mirrored
/// through an atomic only for the Service readiness observer outside the actor.
pub(crate) struct StartupWorkflowState {
    #[cfg(test)]
    retry_target: Option<StartupRetryTarget>,
    owner_unproven: Arc<AtomicBool>,
}

impl Default for StartupWorkflowState {
    fn default() -> Self {
        Self {
            #[cfg(test)]
            retry_target: None,
            owner_unproven: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl StartupWorkflowState {
    pub(crate) fn with_owner_signal(owner_unproven: Arc<AtomicBool>) -> Self {
        Self {
            #[cfg(test)]
            retry_target: None,
            owner_unproven,
        }
    }

    #[cfg(test)]
    pub(crate) fn retry_target(&self) -> Option<&StartupRetryTarget> {
        self.retry_target.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn retry_target_mut(&mut self) -> &mut Option<StartupRetryTarget> {
        &mut self.retry_target
    }

    pub(crate) fn owner_unproven(&self) -> bool {
        self.owner_unproven.load(Ordering::Acquire)
    }

    pub(crate) fn mark_owner_unproven(&self) {
        self.owner_unproven.store(true, Ordering::Release);
    }

    pub(crate) fn mark_owner_proven(&self) {
        self.owner_unproven.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupOutcome {
    /// The committed runtime was reconciled under the selected host.
    Ready,
    /// Startup completed safely, but runtime convergence is still pending.
    ReadyDegraded {
        health: ConvergenceHealth,
        reason: String,
    },
    /// A side effect or owner could not be established safely.
    RecoveryRequired { reason: String },
    /// The startup workflow did not return a terminal report.
    Unsettled { reason: String },
}

/// Compatibility state used only by the test-only Direct CoreLifecycle adapter.
/// Production startup retries use ApplicationWorkflow's shared DeferredTarget.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct StartupRetryTarget {
    pub(crate) identity: String,
    /// Recovery is the only startup origin allowed to keep a verified live
    /// runtime receipt. Preserve this origin across scheduled retries.
    pub(crate) allow_receipt_reuse: bool,
    pub(crate) cause: String,
    pub(crate) budget: RetryBudget,
    pub(crate) waits: u8,
    pub(crate) health: ConvergenceHealth,
    pub(crate) next_attempt: Option<tokio::time::Instant>,
}

#[cfg(test)]
impl StartupRetryTarget {
    pub(crate) fn new(identity: String, allow_receipt_reuse: bool) -> Self {
        Self {
            identity,
            allow_receipt_reuse,
            cause: "the runtime is being re-established".into(),
            budget: RetryBudget::default(),
            waits: 0,
            health: ConvergenceHealth::Pending,
            next_attempt: None,
        }
    }

    pub(crate) fn schedule_application(
        &mut self,
        reason: String,
        automatic: bool,
    ) -> StartupOutcome {
        self.schedule_application_at(tokio::time::Instant::now(), reason, automatic)
    }

    pub(crate) fn schedule_application_at(
        &mut self,
        now: tokio::time::Instant,
        reason: String,
        automatic: bool,
    ) -> StartupOutcome {
        self.waits = 0;
        self.cause.clone_from(&reason);
        if automatic && self.budget.record_automatic_attempt().is_none() {
            self.health = ConvergenceHealth::Blocked;
            self.next_attempt = None;
        } else if let Some(delay) = self.budget.next_delay() {
            self.health = ConvergenceHealth::RetryScheduled;
            self.next_attempt = Some(now + delay);
        } else {
            self.health = ConvergenceHealth::Blocked;
            self.next_attempt = None;
        }
        StartupOutcome::ReadyDegraded {
            health: self.health,
            reason,
        }
    }

    pub(crate) fn schedule_dependency(&mut self, reason: String) -> StartupOutcome {
        self.schedule_dependency_at(tokio::time::Instant::now(), reason)
    }

    pub(crate) fn schedule_dependency_at(
        &mut self,
        now: tokio::time::Instant,
        reason: String,
    ) -> StartupOutcome {
        self.cause.clone_from(&reason);
        if self.budget.remaining == 0 {
            let reason = format!(
                "{reason}; automatic retries are spent, so retry explicitly once the dependency is available"
            );
            self.cause.clone_from(&reason);
            self.health = ConvergenceHealth::Blocked;
            self.next_attempt = None;
            return StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason,
            };
        }
        let (delay, waits) = next_wait(self.waits, OutcomeClass::Dependency);
        self.waits = waits;
        self.health = ConvergenceHealth::WaitingDependency;
        self.next_attempt = Some(now + delay);
        StartupOutcome::ReadyDegraded {
            health: self.health,
            reason,
        }
    }

    pub(crate) fn block(&mut self, reason: String) -> StartupOutcome {
        self.cause.clone_from(&reason);
        self.health = ConvergenceHealth::Blocked;
        self.next_attempt = None;
        StartupOutcome::ReadyDegraded {
            health: self.health,
            reason,
        }
    }

    pub(crate) fn begin(&mut self, now: tokio::time::Instant) -> bool {
        if self.budget.remaining == 0
            || self.health != ConvergenceHealth::RetryScheduled
                && self.health != ConvergenceHealth::WaitingDependency
            || self
                .next_attempt
                .is_none_or(|next_attempt| next_attempt > now)
        {
            return false;
        }
        self.next_attempt = None;
        self.health = ConvergenceHealth::Pending;
        true
    }
}

#[cfg(test)]
impl From<StartupRetryTarget> for DeferredTarget {
    fn from(target: StartupRetryTarget) -> Self {
        let cause = if target.allow_receipt_reuse {
            ReestablishCause::Recovery
        } else {
            ReestablishCause::Startup
        };
        Self {
            operation_id: OperationId::generate(),
            origin: TargetOrigin::Reestablish(cause),
            identity: target.identity,
            cause: crate::client::application_workflow::mutation::RetryableCause {
                stage: crate::client::application_workflow::mutation::MutationStage::Preparing,
                message: target.cause,
            },
            attempts_remaining: target.budget.remaining,
            attempts: target.budget.attempts,
            waits: target.waits,
            health: target.health,
            next_attempt: target.next_attempt,
            mutation: None,
        }
    }
}

#[cfg(test)]
impl TryFrom<DeferredTarget> for StartupRetryTarget {
    type Error = ();

    fn try_from(target: DeferredTarget) -> Result<Self, Self::Error> {
        let TargetOrigin::Reestablish(cause) = target.origin else {
            return Err(());
        };
        Ok(Self {
            identity: target.identity,
            allow_receipt_reuse: cause == ReestablishCause::Recovery,
            cause: target.cause.message,
            budget: RetryBudget {
                remaining: target.attempts_remaining,
                attempts: target.attempts,
            },
            waits: target.waits,
            health: target.health,
            next_attempt: target.next_attempt,
        })
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn startup_application_retry_budget_matches_reference_delays() {
        let mut target = StartupRetryTarget::new("first target".into(), false);
        let now = tokio::time::Instant::now();
        assert!(matches!(
            target.schedule_application_at(now, "initial failure".into(), false),
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::RetryScheduled,
                ..
            }
        ));
        assert_eq!(target.budget.attempts, 0);
        assert_eq!(target.budget.remaining, 3);
        assert_eq!(
            target.next_attempt,
            Some(now + std::time::Duration::from_secs(1))
        );

        let now = tokio::time::Instant::now();
        target.schedule_application_at(now, "first retry failed".into(), true);
        assert_eq!(target.budget.attempts, 1);
        assert_eq!(target.budget.remaining, 2);
        assert_eq!(
            target.next_attempt,
            Some(now + std::time::Duration::from_secs(5))
        );

        let now = tokio::time::Instant::now();
        target.schedule_application_at(now, "second retry failed".into(), true);
        assert_eq!(target.budget.attempts, 2);
        assert_eq!(target.budget.remaining, 1);
        assert_eq!(
            target.next_attempt,
            Some(now + std::time::Duration::from_secs(30))
        );

        let exhausted = target.schedule_application_at(
            tokio::time::Instant::now(),
            "last retry failed".into(),
            true,
        );
        assert!(matches!(
            exhausted,
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                ..
            }
        ));
        assert_eq!(target.budget.attempts, 3);
        assert_eq!(target.budget.remaining, 0);
        assert!(target.next_attempt.is_none());
    }

    #[test]
    fn startup_dependency_backoff_does_not_spend_application_budget() {
        let mut target = StartupRetryTarget::new("first target".into(), false);
        for expected in [5, 5, 10, 30, 60, 60] {
            let now = tokio::time::Instant::now();
            target.schedule_dependency_at(now, "dependency is unavailable".into());
            assert_eq!(
                target.next_attempt,
                Some(now + std::time::Duration::from_secs(expected))
            );
            assert_eq!(target.budget.attempts, 0);
            assert_eq!(target.budget.remaining, 3);
        }
        assert_eq!(target.waits, 4);

        let now = tokio::time::Instant::now();
        target.schedule_application_at(now, "application attempt".into(), true);
        assert_eq!(target.waits, 0);
        assert_eq!(target.budget.attempts, 1);
        assert_eq!(
            target.next_attempt,
            Some(now + std::time::Duration::from_secs(5))
        );
    }

    #[test]
    fn startup_retry_only_begins_after_its_due_time() {
        let mut target = StartupRetryTarget::new("first target".into(), false);
        target.schedule_dependency("waiting".into());
        let due = target.next_attempt.unwrap();
        assert!(!target.begin(due - std::time::Duration::from_secs(1)));
        assert!(target.begin(due));
        assert!(target.next_attempt.is_none());
        assert_eq!(target.health, ConvergenceHealth::Pending);
    }
}

impl StartupOutcome {
    pub(crate) fn failure_reason(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::ReadyDegraded { reason, .. }
            | Self::RecoveryRequired { reason }
            | Self::Unsettled { reason } => Some(reason),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StartupObservation {
    pub(crate) desired: ExecutionHost,
    pub(crate) service: Option<ServiceEvidence>,
    /// Chimera's current app-side Core status projection is narrower than
    /// ref's CoreStatusProjection. A missing state on the Service host means
    /// the legacy adapter supplied its timestamp-zero unreachable sentinel.
    pub(crate) runtime: Option<RuntimeEvidence>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeEvidence {
    pub(crate) host: ExecutionHost,
    pub(crate) state: Option<CoreState>,
    pub(crate) state_changed_at: i64,
}

/// What one Service probe says about a core the daemon may hold.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ServiceEvidence {
    /// Not installed, or the daemon is stopped: it holds no core.
    Absent { phase: ServicePhase },
    /// The daemon answers, and its core is stopped.
    CoreStopped {
        ready: bool,
        phase: ServicePhase,
        runtime_owned: bool,
    },
    /// The daemon reports a stopped core with a reason. For this installation,
    /// error stops may hide a quarantined process, so the manager must recover
    /// before another host is allowed to start. A foreign manager stays
    /// unproven and must never receive this installation's recovery command.
    CoreStoppedWithReason {
        ready: bool,
        phase: ServicePhase,
        reason: String,
        runtime_owned: bool,
    },
    /// The daemon answers, and its core is not stopped.
    CoreRunning {
        ready: bool,
        phase: ServicePhase,
        instance: Option<String>,
        revision: Option<ConfigRevisionInfo>,
    },
    /// The daemon answers for another app instance; this instance must leave
    /// that core alone.
    ForeignCore { ready: bool, phase: ServicePhase },
    /// The probe failed, or the answer cannot establish whether a core is held.
    Unreadable { phase: ServicePhase },
}

impl ServiceEvidence {
    pub(crate) fn from_status(status: &ServiceHostStatus) -> Self {
        if status.phase == ServicePhase::Unknown {
            return Self::Unreadable {
                phase: status.phase,
            };
        }

        let ready = status.phase == ServicePhase::Ready;
        match (status.status, status.server.as_ref()) {
            (ServiceStatus::NotInstalled | ServiceStatus::Stopped, _) => Self::Absent {
                phase: status.phase,
            },
            (ServiceStatus::Running, None) => Self::Unreadable {
                phase: status.phase,
            },
            (ServiceStatus::Running, Some(server)) => match &server.core_infos.detail {
                Some(chimera_ipc::api::status::CoreStateDetail::Stopped {
                    reason: Some(reason),
                }) => Self::CoreStoppedWithReason {
                    ready,
                    phase: status.phase,
                    reason: reason.clone(),
                    runtime_owned: status.runtime_owned,
                },
                Some(chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }) => {
                    Self::CoreStopped {
                        ready,
                        phase: status.phase,
                        runtime_owned: status.runtime_owned,
                    }
                }
                Some(_) if !status.runtime_owned => Self::ForeignCore {
                    ready,
                    phase: status.phase,
                },
                Some(_) => Self::CoreRunning {
                    ready,
                    phase: status.phase,
                    instance: server.core_infos.instance_id.clone(),
                    revision: server.core_infos.revision.clone(),
                },
                None => Self::Unreadable {
                    phase: status.phase,
                },
            },
        }
    }

    pub(crate) fn phase(&self) -> ServicePhase {
        match self {
            Self::Absent { phase }
            | Self::CoreStopped { phase, .. }
            | Self::CoreStoppedWithReason { phase, .. }
            | Self::CoreRunning { phase, .. }
            | Self::ForeignCore { phase, .. }
            | Self::Unreadable { phase } => *phase,
        }
    }

    pub(crate) fn proves_no_owned_core(&self) -> bool {
        matches!(
            self,
            Self::Absent { .. } | Self::CoreStopped { .. } | Self::ForeignCore { .. }
        )
    }

    pub(crate) fn proves_runtime_owner(&self) -> bool {
        match self {
            Self::CoreStopped { runtime_owned, .. }
            | Self::CoreStoppedWithReason { runtime_owned, .. } => *runtime_owned,
            Self::CoreRunning { .. } => true,
            Self::Absent { .. } | Self::ForeignCore { .. } | Self::Unreadable { .. } => false,
        }
    }

    /// Build the server-side conditional-stop token from complete startup
    /// evidence. Missing or malformed fields fail closed.
    pub(crate) fn expected_owner(&self) -> Result<CoreOwner, String> {
        let Self::CoreRunning {
            instance: Some(instance),
            revision: Some(revision),
            ..
        } = self
        else {
            return Err("the Service probe did not provide a complete Core owner token".into());
        };
        let epoch = Epoch::try_from(revision.epoch)
            .map_err(|_| "the Service probe returned a zero Core revision epoch")?;
        CoreOwner::from_parts(
            instance,
            RevisionId {
                epoch,
                generation: revision.generation,
                effective_hash: revision.effective_hash.clone(),
            },
        )
    }
}

/// The shared startup rule: Service Mode is a preference. An unusable
/// Service falls back to Local; an unknown transition stays with the current
/// host until the next observation.
pub(crate) fn effective_host(
    enable_service_mode: bool,
    service_ready: Option<bool>,
    current: ExecutionHost,
) -> ExecutionHost {
    match (enable_service_mode, service_ready) {
        (true, Some(true)) => ExecutionHost::Service,
        (true, None) => current,
        _ => ExecutionHost::Local,
    }
}

/// Adapt Chimera's quarantine evidence into ref's runtime ownership planner.
/// A stopped Core with an error reason may still own quarantined processes:
/// it must never be presented to the planner as an ordinary stopped Core.
fn reconciler_service_evidence(service: &ServiceEvidence) -> ReconcilerServiceEvidence {
    match service {
        ServiceEvidence::Absent { phase } => ReconcilerServiceEvidence::Absent { phase: *phase },
        ServiceEvidence::CoreStopped { ready, phase, .. } => {
            ReconcilerServiceEvidence::CoreStopped {
                ready: *ready,
                phase: *phase,
            }
        }
        ServiceEvidence::CoreStoppedWithReason { phase, .. }
        | ServiceEvidence::Unreadable { phase } => {
            ReconcilerServiceEvidence::Unreadable { phase: *phase }
        }
        ServiceEvidence::CoreRunning {
            ready,
            phase,
            instance,
            revision,
        } => ReconcilerServiceEvidence::CoreRunning {
            ready: *ready,
            phase: *phase,
            instance: instance.clone(),
            revision: revision.clone(),
        },
        ServiceEvidence::ForeignCore { ready, phase } => ReconcilerServiceEvidence::ForeignCore {
            ready: *ready,
            phase: *phase,
        },
    }
}

/// Production startup uses ref's RuntimeReconciler ownership planner.
/// CoreLifecycle still executes the chosen transition until its actor is ported.
pub(crate) fn plan_owner(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: &ServiceEvidence,
) -> OwnerPlan {
    reconcile_plan_owner(desired, owner, &reconciler_service_evidence(service))
}

/// A readiness change only starts ownership work if ref's S2 planner found a
/// concrete, safe host transition. Importantly, an already selected Local
/// Core may still owe retirement of a second *owned* Service Core: checking
/// only `desired != owner` would silently leave both owners running.
fn owner_followup_required(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: &ServiceEvidence,
) -> bool {
    matches!(
        plan_owner(desired, owner, service),
        OwnerPlan::RetireServiceThenLocal | OwnerPlan::HandBackToLocal | OwnerPlan::AdoptService
    )
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use chimera_core::actor_v2::service_policy::ServicePhase;
    use chimera_core::actor_v2::service_status::ServiceHostStatus;
    use chimera_ipc::{
        api::status::{
            ConfigRevisionInfo, CoreInfos, CoreStateDetail, RuntimeInfos, StatusResBody,
        },
        types::ServiceStatus,
    };

    use super::{OwnerPlan, ServiceEvidence, effective_host, owner_followup_required, plan_owner};
    use crate::core::actor_v2::control_endpoint::ExecutionHost::{Local, Service};

    #[test]
    fn ref_s2_followup_retirement_is_required_even_with_local_already_selected() {
        use ServicePhase::{DaemonStopped, Ready, Unknown};

        let owned_service_core = ServiceEvidence::CoreRunning {
            ready: true,
            phase: Ready,
            instance: Some("owned".into()),
            revision: None,
        };
        assert!(owner_followup_required(Local, Local, &owned_service_core));
        assert!(owner_followup_required(Local, Service, &owned_service_core));
        assert!(owner_followup_required(Service, Local, &owned_service_core));

        for service in [
            ServiceEvidence::Absent {
                phase: DaemonStopped,
            },
            ServiceEvidence::CoreStopped {
                ready: true,
                phase: Ready,
                runtime_owned: true,
            },
            ServiceEvidence::ForeignCore {
                ready: true,
                phase: Ready,
            },
            ServiceEvidence::Unreadable { phase: Unknown },
        ] {
            assert!(
                !owner_followup_required(Local, Local, &service),
                "an absent, stopped, foreign or unreadable Service Core must not be retired: {service:?}"
            );
        }
        assert!(!owner_followup_required(
            Service,
            Local,
            &ServiceEvidence::Absent {
                phase: DaemonStopped
            }
        ));
        assert!(!owner_followup_required(
            Service,
            Local,
            &ServiceEvidence::Unreadable { phase: Unknown }
        ));
    }

    fn evidence(
        status: ServiceStatus,
        phase: ServicePhase,
        detail: Option<Option<CoreStateDetail>>,
        owned: bool,
    ) -> ServiceEvidence {
        let server = detail.map(|detail| StatusResBody {
            version: Cow::Borrowed("1.0.0"),
            core_infos: CoreInfos {
                instance_id: Some("instance".into()),
                r#type: None,
                state: chimera_ipc::api::status::CoreState::Running,
                state_changed_at: 0,
                config_path: None,
                controller: None,
                health: None,
                revision: None,
                detail,
            },
            runtime_infos: RuntimeInfos {
                service_data_dir: Cow::Owned(Default::default()),
                service_config_dir: Cow::Owned(Default::default()),
                nyanpasu_config_dir: Cow::Owned(Default::default()),
                nyanpasu_data_dir: Cow::Owned(Default::default()),
            },
        });
        let mut projected = ServiceHostStatus::probing();
        projected.status = status;
        projected.phase = phase;
        projected.server = server;
        projected.runtime_owned = owned;
        ServiceEvidence::from_status(&projected)
    }

    #[test]
    fn startup_owner_plan_matches_reference_table() {
        let absent = evidence(
            ServiceStatus::NotInstalled,
            ServicePhase::NotInstalled,
            None,
            false,
        );
        let stopped_ready = evidence(
            ServiceStatus::Running,
            ServicePhase::Ready,
            Some(Some(CoreStateDetail::Stopped { reason: None })),
            true,
        );
        let stopped_unready = evidence(
            ServiceStatus::Running,
            ServicePhase::Incompatible,
            Some(Some(CoreStateDetail::Stopped { reason: None })),
            true,
        );
        let running_ready = evidence(
            ServiceStatus::Running,
            ServicePhase::Ready,
            Some(Some(CoreStateDetail::Running { epoch: 1, pid: 7 })),
            true,
        );
        let running_unready = evidence(
            ServiceStatus::Running,
            ServicePhase::Exhausted,
            Some(Some(CoreStateDetail::Running { epoch: 1, pid: 7 })),
            true,
        );
        let foreign_ready = evidence(
            ServiceStatus::Running,
            ServicePhase::Ready,
            Some(Some(CoreStateDetail::Running { epoch: 1, pid: 7 })),
            false,
        );
        let foreign_unready = evidence(
            ServiceStatus::Running,
            ServicePhase::Exhausted,
            Some(Some(CoreStateDetail::Running { epoch: 1, pid: 7 })),
            false,
        );
        let unreadable = evidence(ServiceStatus::Running, ServicePhase::Unknown, None, false);

        let cases = [
            (Local, Local, &absent, OwnerPlan::Owned),
            (Local, Local, &stopped_ready, OwnerPlan::Owned),
            (Local, Local, &stopped_unready, OwnerPlan::Owned),
            (
                Local,
                Local,
                &running_ready,
                OwnerPlan::RetireServiceThenLocal,
            ),
            (
                Local,
                Local,
                &running_unready,
                OwnerPlan::Unproven {
                    phase: ServicePhase::Exhausted,
                },
            ),
            (Local, Local, &foreign_ready, OwnerPlan::Owned),
            (Local, Local, &foreign_unready, OwnerPlan::Owned),
            (
                Local,
                Local,
                &unreadable,
                OwnerPlan::Unproven {
                    phase: ServicePhase::Unknown,
                },
            ),
            (Local, Service, &absent, OwnerPlan::HandBackToLocal),
            (Local, Service, &running_ready, OwnerPlan::HandBackToLocal),
            (Local, Service, &unreadable, OwnerPlan::HandBackToLocal),
            (Service, Service, &absent, OwnerPlan::Owned),
            (Service, Service, &running_unready, OwnerPlan::Owned),
            (Service, Service, &unreadable, OwnerPlan::Owned),
            (Service, Local, &stopped_ready, OwnerPlan::AdoptService),
            (Service, Local, &running_ready, OwnerPlan::AdoptService),
            (Service, Local, &foreign_ready, OwnerPlan::AdoptService),
            (
                Service,
                Local,
                &foreign_unready,
                OwnerPlan::Unproven {
                    phase: ServicePhase::Exhausted,
                },
            ),
            (
                Service,
                Local,
                &absent,
                OwnerPlan::WaitForService {
                    phase: ServicePhase::NotInstalled,
                },
            ),
            (
                Service,
                Local,
                &stopped_unready,
                OwnerPlan::WaitForService {
                    phase: ServicePhase::Incompatible,
                },
            ),
            (
                Service,
                Local,
                &running_unready,
                OwnerPlan::Unproven {
                    phase: ServicePhase::Exhausted,
                },
            ),
            (
                Service,
                Local,
                &unreadable,
                OwnerPlan::Unproven {
                    phase: ServicePhase::Unknown,
                },
            ),
        ];

        for (desired, owner, service, expected) in cases {
            assert_eq!(
                plan_owner(desired, owner, service),
                expected,
                "{desired:?}/{owner:?}/{service:?}"
            );
        }
    }

    #[test]
    fn error_stopped_service_requires_manager_recovery_before_another_host_starts() {
        let error_stopped = evidence(
            ServiceStatus::Running,
            ServicePhase::Ready,
            Some(Some(CoreStateDetail::Stopped {
                reason: Some("service manager quarantine".into()),
            })),
            true,
        );

        assert!(matches!(
            &error_stopped,
            ServiceEvidence::CoreStoppedWithReason { .. }
        ));
        assert!(!error_stopped.proves_no_owned_core());
        assert_eq!(
            plan_owner(Local, Local, &error_stopped),
            OwnerPlan::Unproven {
                phase: ServicePhase::Ready
            }
        );

        let foreign_error_stopped = evidence(
            ServiceStatus::Running,
            ServicePhase::Ready,
            Some(Some(CoreStateDetail::Stopped {
                reason: Some("foreign service manager quarantine".into()),
            })),
            false,
        );
        assert!(matches!(
            &foreign_error_stopped,
            ServiceEvidence::CoreStoppedWithReason {
                runtime_owned: false,
                ..
            }
        ));
        assert!(!foreign_error_stopped.proves_no_owned_core());
        assert!(!foreign_error_stopped.proves_runtime_owner());
        assert_eq!(
            plan_owner(Local, Local, &foreign_error_stopped),
            OwnerPlan::Unproven {
                phase: ServicePhase::Ready
            }
        );
    }

    #[test]
    fn service_mode_falls_back_on_settled_unready_and_waits_during_transition() {
        assert_eq!(effective_host(true, Some(true), Local), Service);
        assert_eq!(effective_host(true, Some(false), Service), Local);
        assert_eq!(effective_host(true, None, Service), Service);
        assert_eq!(effective_host(false, None, Service), Local);
    }

    #[test]
    fn service_evidence_never_reads_a_stale_unknown_probe() {
        let stale_running = evidence(
            ServiceStatus::Running,
            ServicePhase::Unknown,
            Some(Some(CoreStateDetail::Running { epoch: 1, pid: 7 })),
            true,
        );
        assert!(matches!(stale_running, ServiceEvidence::Unreadable { .. }));

        let foreign = evidence(
            ServiceStatus::Running,
            ServicePhase::Ready,
            Some(Some(CoreStateDetail::Running { epoch: 1, pid: 7 })),
            false,
        );
        assert!(matches!(foreign, ServiceEvidence::ForeignCore { .. }));
    }

    #[test]
    fn expected_service_owner_requires_complete_and_well_formed_identity() {
        let incomplete = ServiceEvidence::CoreRunning {
            ready: true,
            phase: ServicePhase::Ready,
            instance: None,
            revision: None,
        };
        assert!(incomplete.expected_owner().is_err());

        let malformed = ServiceEvidence::CoreRunning {
            ready: true,
            phase: ServicePhase::Ready,
            instance: Some("not-a-uuid".into()),
            revision: Some(ConfigRevisionInfo {
                epoch: 1,
                generation: 2,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            }),
        };
        assert!(malformed.expected_owner().is_err());

        let zero_epoch = ServiceEvidence::CoreRunning {
            ready: true,
            phase: ServicePhase::Ready,
            instance: Some("9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
            revision: Some(ConfigRevisionInfo {
                epoch: 0,
                generation: 2,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            }),
        };
        assert!(zero_epoch.expected_owner().is_err());
    }
}

impl ApplicationWorkflow {
    async fn startup_reconcile_once(
        &mut self,
        operation_id: u64,
        allow_receipt_reuse: bool,
    ) -> StartupReport {
        use crate::core::actor_v2::control_endpoint::ExecutionHost;

        let lifecycle = self.lifecycle_handle();
        self.startup.mark_owner_unproven();
        let app = lifecycle.startup_application_config();
        let current = match lifecycle.startup_core_status().await {
            Ok(current) => current,
            Err(error) => {
                return StartupReport {
                    operation_id,
                    observation: None,
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: format!("the current Core owner could not be read: {error:#}"),
                    },
                };
            }
        };
        let initial_owner = host_for_run_type(current.run_type);
        let runtime = Some(runtime_evidence(&current));

        if lifecycle.outcome_uncertain() {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired: effective_host(app.enable_service_mode, None, initial_owner),
                    service: None,
                    runtime,
                }),
                outcome: StartupOutcome::RecoveryRequired {
                    reason: "a previous Core mutation has an uncertain outcome; startup will not issue another mutation".into(),
                },
            };
        }

        if !lifecycle.startup_service_command_settled().await {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired: effective_host(app.enable_service_mode, None, initial_owner),
                    service: None,
                    runtime,
                }),
                outcome: StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason: "a Service command started earlier is still running".into(),
                },
            };
        }

        let (service, service_ready_generation) = lifecycle.service_evidence().await;
        let (service, service_ready_generation) = match lifecycle
            .recover_service_error_stop(service.clone(), service_ready_generation)
            .await
        {
            Ok(recovered) => recovered,
            Err(failure) => {
                return self.startup_owner_failure_report(
                    operation_id,
                    StartupObservation {
                        desired: effective_host(
                            app.enable_service_mode,
                            service.phase().readiness(),
                            initial_owner,
                        ),
                        service: Some(service),
                        runtime,
                    },
                    failure,
                );
            }
        };
        let current = match lifecycle.startup_core_status().await {
            Ok(current) => current,
            Err(error) => {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired: effective_host(
                            app.enable_service_mode,
                            service.phase().readiness(),
                            initial_owner,
                        ),
                        service: Some(service),
                        runtime: None,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: format!("the Core owner could not be refreshed: {error:#}"),
                    },
                };
            }
        };
        let initial_owner = host_for_run_type(current.run_type);
        let runtime = Some(runtime_evidence(&current));
        let failed_generation = lifecycle.startup_failed_generation();
        let service_readiness =
            if failed_generation != 0 && failed_generation == service_ready_generation {
                Some(false)
            } else {
                service.phase().readiness()
            };
        let desired = if matches!(service, ServiceEvidence::ForeignCore { .. }) {
            ExecutionHost::Local
        } else {
            effective_host(app.enable_service_mode, service_readiness, initial_owner)
        };

        // The legacy Core status projection uses timestamp zero when the
        // selected Service endpoint could not answer. Do not read its
        // synthetic Stopped state as a proof that the owner is idle.
        if initial_owner == ExecutionHost::Service
            && runtime.as_ref().is_some_and(|r| r.state.is_none())
        {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired,
                    service: Some(service),
                    runtime,
                }),
                outcome: StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason: "the Service host did not publish a current Core state".into(),
                },
            };
        }

        let plan = plan_owner(desired, initial_owner, &service);
        let mut current = current;
        let mut service = service;
        match plan {
            OwnerPlan::WaitForService { phase } => {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: format!(
                            "the Service host is {phase:?}; startup waits for readiness"
                        ),
                    },
                };
            }
            OwnerPlan::Unproven { phase } => {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::RecoveryRequired,
                        reason: format!(
                            "the Service host ({phase:?}) may still hold a Core, so startup did not start another host"
                        ),
                    },
                };
            }
            OwnerPlan::RetireServiceThenLocal => {
                service = match lifecycle
                    .retire_service_startup_owner(&service, false)
                    .await
                {
                    Ok(service) => service,
                    Err(failure) => {
                        return self.startup_owner_failure_report(
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            failure,
                        );
                    }
                };
                current = match lifecycle.startup_core_status().await {
                    Ok(current) => current,
                    Err(error) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime: None,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "the Local Core owner could not be refreshed after Service retirement: {error:#}"
                                ),
                            },
                        };
                    }
                };
            }
            OwnerPlan::HandBackToLocal => {
                match &service {
                    ServiceEvidence::CoreRunning { .. } => {
                        service = match lifecycle
                            .retire_service_startup_owner(&service, false)
                            .await
                        {
                            Ok(service) => service,
                            Err(failure) => {
                                return self.startup_owner_failure_report(
                                    operation_id,
                                    StartupObservation {
                                        desired,
                                        service: Some(service),
                                        runtime,
                                    },
                                    failure,
                                );
                            }
                        };
                    }
                    ServiceEvidence::Absent { .. }
                    | ServiceEvidence::CoreStopped { .. }
                    | ServiceEvidence::ForeignCore { .. } => {}
                    ServiceEvidence::CoreStoppedWithReason { .. }
                    | ServiceEvidence::Unreadable { .. } => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::RecoveryRequired {
                                reason: "the Service Core owner is unreadable; startup will not stop Service or start Local".into(),
                            },
                        };
                    }
                }
                if let Err(error) = lifecycle
                    .startup_select_execution_host(ExecutionHost::Local)
                    .await
                {
                    return self
                        .startup_failure(
                            &lifecycle,
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            error,
                        )
                        .await;
                }
                current = match lifecycle.startup_core_status().await {
                    Ok(current) => current,
                    Err(error) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime: None,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "the Local Core owner could not be read after handback: {error:#}"
                                ),
                            },
                        };
                    }
                };
            }
            OwnerPlan::Owned => {
                if initial_owner == ExecutionHost::Service
                    && matches!(
                        &service,
                        ServiceEvidence::Unreadable { .. } | ServiceEvidence::ForeignCore { .. }
                    )
                {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::RecoveryRequired {
                            reason: "the current Service Core owner is not verified; startup did not submit a Service mutation".into(),
                        },
                    };
                }
                if initial_owner == ExecutionHost::Service {
                    let state_matches_service = matches!(
                        (
                            &service,
                            runtime.as_ref().and_then(|runtime| runtime.state.as_ref())
                        ),
                        (
                            ServiceEvidence::CoreRunning { .. },
                            Some(CoreState::Running)
                        ) | (
                            ServiceEvidence::CoreStopped { .. },
                            Some(CoreState::Stopped(_))
                        )
                    );
                    if !state_matches_service {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: "the Service probe and Core status do not describe the same settled owner state".into(),
                            },
                        };
                    }
                }
            }
            OwnerPlan::AdoptService => {
                if matches!(
                    &service,
                    ServiceEvidence::ForeignCore { .. } | ServiceEvidence::Unreadable { .. }
                ) {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::RecoveryRequired {
                            reason:
                                "the Service Core owner is not verified; startup did not adopt it"
                                    .into(),
                        },
                    };
                }
                if matches!(current.state, CoreState::Running) {
                    if let Err(failure) = lifecycle.confirm_local_owner_stopped(&current).await {
                        return self.startup_owner_failure_report(
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            failure,
                        );
                    }
                } else if current.state_changed_at > 0
                    && let Err(reason) = lifecycle.confirm_local_stopped_baseline(&current).await
                {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::ReadyDegraded {
                            health:
                                crate::client::convergence::ConvergenceHealth::WaitingDependency,
                            reason,
                        },
                    };
                }
                if let Err(error) = lifecycle
                    .startup_select_execution_host(ExecutionHost::Service)
                    .await
                {
                    return self
                        .startup_failure(
                            &lifecycle,
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            error,
                        )
                        .await;
                }
                current = match lifecycle.startup_core_status().await {
                    Ok(current) => current,
                    Err(error) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime: None,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "the Service Core owner could not be read after adoption: {error:#}"
                                ),
                            },
                        };
                    }
                };
                let (fresh_service, _) = lifecycle.service_evidence().await;
                service = fresh_service;
                if matches!(
                    &service,
                    ServiceEvidence::ForeignCore { .. } | ServiceEvidence::Unreadable { .. }
                ) {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime: Some(runtime_evidence(&current)),
                        }),
                        outcome: StartupOutcome::RecoveryRequired {
                            reason: "the adopted Service Core owner changed or is not verified"
                                .into(),
                        },
                    };
                }
            }
        };

        let owner = host_for_run_type(current.run_type);
        let runtime = Some(runtime_evidence(&current));
        if owner != desired {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired,
                    service: Some(service),
                    runtime,
                }),
                outcome: StartupOutcome::RecoveryRequired {
                    reason: format!(
                        "startup handoff selected {owner:?}, but the committed target requires {desired:?}"
                    ),
                },
            };
        }
        if owner == ExecutionHost::Service {
            let state_matches_service = matches!(
                (
                    &service,
                    runtime.as_ref().and_then(|runtime| runtime.state.as_ref())
                ),
                (
                    ServiceEvidence::CoreRunning { .. },
                    Some(CoreState::Running)
                ) | (
                    ServiceEvidence::CoreStopped { .. },
                    Some(CoreState::Stopped(_))
                )
            );
            if !state_matches_service {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: "the Service probe and selected Core status do not describe the same settled owner state".into(),
                    },
                };
            }
        }

        // Every settled owner plan now enters the same S3 receipt/retirement
        // checks before the committed target is applied in S4.
        let is_running = runtime
            .as_ref()
            .is_some_and(|runtime| matches!(runtime.state.as_ref(), Some(CoreState::Running)));
        if is_running {
            match lifecycle
                .recovery_receipt_matches(&current, desired, allow_receipt_reuse)
                .await
            {
                Ok(true) => {
                    self.startup.mark_owner_proven();
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::Ready,
                    };
                }
                Ok(false) => {}
                Err(reason) => {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::ReadyDegraded {
                            health:
                                crate::client::convergence::ConvergenceHealth::WaitingDependency,
                            reason,
                        },
                    };
                }
            }

            let retired = if desired == ExecutionHost::Service {
                match lifecycle.retire_service_startup_owner(&service, true).await {
                    Ok(fresh_service) => {
                        service = fresh_service;
                        Ok(())
                    }
                    Err(failure) => Err(failure),
                }
            } else {
                lifecycle.confirm_local_owner_stopped(&current).await
            };
            if let Err(failure) = retired {
                return self.startup_owner_failure_report(
                    operation_id,
                    StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    },
                    failure,
                );
            }
        } else if desired == ExecutionHost::Local
            && runtime.as_ref().is_some_and(|runtime| {
                runtime.state_changed_at > 0
                    && matches!(runtime.state.as_ref(), Some(CoreState::Stopped(_)))
            })
            && let Err(reason) = lifecycle.confirm_local_stopped_baseline(&current).await
        {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired,
                    service: Some(service),
                    runtime,
                }),
                outcome: StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason,
                },
            };
        }

        self.startup.mark_owner_proven();
        let mut result = lifecycle
            .startup_reconcile_application(
                lifecycle.startup_application_config(),
                desired == ExecutionHost::Service,
            )
            .await;

        if desired == ExecutionHost::Service && result.is_err() && !lifecycle.outcome_uncertain() {
            let failed_owner = lifecycle
                .startup_core_status()
                .await
                .ok()
                .map(|current| host_for_run_type(current.run_type));
            if failed_owner == Some(ExecutionHost::Service) {
                lifecycle.set_startup_failed_generation(service_ready_generation);
                match lifecycle.release_service_owner().await {
                    Ok(fresh_service) => {
                        self.startup.mark_owner_proven();
                        result = lifecycle
                            .startup_reconcile_application_after_service_release(
                                lifecycle.startup_application_config(),
                            )
                            .await;
                        return self.finish_startup_report(
                            &lifecycle,
                            operation_id,
                            StartupObservation {
                                desired: ExecutionHost::Local,
                                service: Some(fresh_service),
                                runtime,
                            },
                            result,
                        );
                    }
                    Err(reason) => {
                        self.startup.mark_owner_unproven();
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "Service was ready but its Core did not reconcile, and Local fallback could not prove release: {reason}"
                                ),
                            },
                        };
                    }
                }
            }
        }

        self.finish_startup_report(
            &lifecycle,
            operation_id,
            StartupObservation {
                desired,
                service: Some(service),
                runtime,
            },
            result,
        )
    }

    fn finish_startup_report(
        &self,
        lifecycle: &CoreLifecycleWorkflow,
        operation_id: u64,
        observation: StartupObservation,
        result: anyhow::Result<()>,
    ) -> StartupReport {
        let outcome = match result {
            Ok(()) => StartupOutcome::Ready,
            Err(error) if lifecycle.outcome_uncertain() => {
                self.startup.mark_owner_unproven();
                StartupOutcome::RecoveryRequired {
                    reason: format!(
                        "startup Core reconciliation has an uncertain outcome: {error:#}"
                    ),
                }
            }
            Err(error) if startup_failure_waits_for_dependency(&error) => {
                StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason: format!(
                        "startup Core reconciliation is waiting for a dependency: {error:#}"
                    ),
                }
            }
            Err(error) if startup_failure_is_retryable(&error) => StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                reason: format!("startup Core reconciliation failed transiently: {error:#}"),
            },
            Err(error) => StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                reason: format!("startup Core reconciliation failed: {error:#}"),
            },
        };
        if outcome == StartupOutcome::Ready
            && (observation.desired
                == crate::core::actor_v2::control_endpoint::ExecutionHost::Service
                || !lifecycle.startup_application_config().enable_service_mode)
        {
            lifecycle.set_startup_failed_generation(0);
        }
        StartupReport {
            operation_id,
            observation: Some(observation),
            outcome,
        }
    }

    async fn startup_failure(
        &self,
        lifecycle: &CoreLifecycleWorkflow,
        operation_id: u64,
        observation: StartupObservation,
        error: anyhow::Error,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        let outcome = if lifecycle.outcome_uncertain() {
            StartupOutcome::RecoveryRequired {
                reason: format!("startup owner transition has an uncertain outcome: {error:#}"),
            }
        } else if startup_failure_waits_for_dependency(&error) {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                reason: format!("startup owner transition is waiting for a dependency: {error:#}"),
            }
        } else if startup_failure_is_retryable(&error) {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                reason: format!("startup owner transition failed transiently: {error:#}"),
            }
        } else {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                reason: format!("startup owner transition failed: {error:#}"),
            }
        };
        StartupReport {
            operation_id,
            observation: Some(observation),
            outcome,
        }
    }

    fn startup_owner_failure_report(
        &self,
        operation_id: u64,
        observation: StartupObservation,
        failure: StartupOwnerFailure,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        StartupReport {
            operation_id,
            observation: Some(observation),
            outcome: failure.outcome(),
        }
    }

    fn begin_reestablish_attempt(&mut self, cause: ReestablishCause) {
        if let Some(attempt) = &mut self.live {
            if let Some(target) = attempt.target_mut() {
                target.origin = TargetOrigin::Reestablish(cause);
                attempt.stage = if cause == ReestablishCause::Recovery {
                    AttemptStage::Recovering
                } else {
                    AttemptStage::Preparing
                };
                attempt.unresolved = None;
                return;
            }
            debug_assert!(false, "an unresolved attempt is never replaced");
            return;
        }

        let mut target = self.deferred.take().unwrap_or_else(|| {
            DeferredTarget::reestablish(OperationId::generate(), String::new(), cause)
        });
        target.origin = TargetOrigin::Reestablish(cause);
        let operation_id = target.operation_id;
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
    }

    async fn update_reestablish_attempt(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        report: StartupReport,
        automatic: bool,
        cause: ReestablishCause,
    ) -> StartupReport {
        let target = self
            .live
            .as_ref()
            .and_then(|attempt| match &attempt.origin {
                AttemptOrigin::CommittedTarget(target) => Some(target.as_ref().clone()),
                AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => None,
            })
            .expect("startup reestablishment runs on its committed target");
        let mut deferred = Some(target);
        let report =
            update_reestablish_target(lifecycle, report, automatic, cause, &mut deferred).await;
        if let Some(target) = deferred
            && let Some(live_target) = self.live.as_mut().and_then(LiveAttempt::target_mut)
        {
            *live_target = target;
        }

        match &report.outcome {
            StartupOutcome::Ready => {
                self.live = None;
                self.deferred = None;
            }
            StartupOutcome::RecoveryRequired { reason } | StartupOutcome::Unsettled { reason } => {
                if let Some(attempt) = &mut self.live {
                    attempt.unresolved = Some(reason.clone());
                }
                self.deferred = None;
                self.publish_recovery_context().await;
            }
            StartupOutcome::ReadyDegraded { .. } => {
                self.conclude_attempt(None);
            }
        }
        report
    }

    pub(in crate::client) async fn startup_reconcile(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        if let Some(report) = &self.startup_report {
            return report.clone();
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Startup);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Startup)
            .await;
        self.startup_report = Some(report.clone());
        report
    }

    pub(in crate::client) async fn recover_startup(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Recovery);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::Recovering;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, true).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Recovery)
            .await;
        if self.startup_report.is_none() {
            self.startup_report = Some(report.clone());
        }
        report
    }

    pub(in crate::client) async fn retry_startup(
        &mut self,
        operation_id: u64,
        identity: String,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        let target = self.deferred.as_ref().filter(|target| {
            target.is_reestablish()
                && target.identity == identity
                && target.health == ConvergenceHealth::Pending
        });
        let Some(target) = target else {
            return StartupReport {
                operation_id,
                observation: None,
                outcome: StartupOutcome::Ready,
            };
        };
        let TargetOrigin::Reestablish(cause) = target.origin else {
            unreachable!("filtered reestablish target")
        };
        let allow_receipt_reuse = target.allows_receipt_reuse();

        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(cause);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self
            .startup_reconcile_once(operation_id, allow_receipt_reuse)
            .await;
        self.update_reestablish_attempt(&lifecycle, report, true, cause)
            .await
    }

    pub(in crate::client) async fn service_host_change_due(&self) -> bool {
        if self.startup_report.is_none() || self.live.is_some() || self.startup.owner_unproven() {
            return false;
        }
        if self.deferred.as_ref().is_some_and(|target| {
            target.is_reestablish()
                && (matches!(
                    target.health,
                    crate::client::convergence::ConvergenceHealth::Blocked
                        | crate::client::convergence::ConvergenceHealth::RecoveryRequired
                ) || target
                    .next_attempt
                    .is_some_and(|next_attempt| next_attempt > tokio::time::Instant::now()))
        }) {
            return false;
        }
        let lifecycle = self.lifecycle();
        if lifecycle.outcome_uncertain() {
            return false;
        }
        let app = lifecycle.startup_application_config();
        let Ok(current) = lifecycle.startup_core_status().await else {
            return false;
        };
        if self.service_recovery_blocks_handoff() {
            return false;
        }
        // Explicit RunIntent is already enforced by CoreLifecycle's sole
        // mailbox before this method. Here the ref S2 planner decides whether
        // a host transition is owed; ready Service evidence can require
        // retiring another owned Core even when Local is already selected.
        let owner = host_for_run_type(current.run_type);
        let (service, generation) = lifecycle.service_evidence().await;
        let failed_generation = lifecycle.startup_failed_generation();
        let readiness = if failed_generation != 0 && failed_generation == generation {
            Some(false)
        } else {
            service.phase().readiness()
        };
        let desired = if matches!(service, ServiceEvidence::ForeignCore { .. }) {
            ExecutionHost::Local
        } else {
            effective_host(app.enable_service_mode, readiness, owner)
        };
        owner_followup_required(desired, owner, &service)
    }

    pub(in crate::client) async fn follow_service(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> anyhow::Result<()> {
        if !self.service_host_change_due().await {
            return Ok(());
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::ServiceReadiness);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(
                &lifecycle,
                report,
                false,
                ReestablishCause::ServiceReadiness,
            )
            .await;
        super::workflow::startup_report_result(&report)
    }

    pub(super) async fn refresh_startup_retry_target_after_reconcile(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        result: &anyhow::Result<()>,
    ) {
        let Some(target) = self
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if result.is_ok() {
            self.clear_startup_retry_target();
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };

        let error = result
            .as_ref()
            .expect_err("failed reconciliation has an error");
        let outcome = if lifecycle.outcome_uncertain() {
            StartupOutcome::RecoveryRequired {
                reason: format!("runtime target reconciliation is uncertain: {error:#}"),
            }
        } else if startup_failure_waits_for_dependency(error) {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::WaitingDependency,
                reason: format!(
                    "runtime target reconciliation is waiting for a dependency: {error:#}"
                ),
            }
        } else if startup_failure_is_retryable(error) {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::RetryScheduled,
                reason: format!("runtime target reconciliation failed transiently: {error:#}"),
            }
        } else {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason: format!("runtime target reconciliation failed: {error:#}"),
            }
        };
        let report = StartupReport {
            operation_id: 0,
            observation: None,
            outcome,
        };
        update_reestablish_target(lifecycle, report, false, cause, &mut self.deferred).await;
    }

    pub(super) async fn reschedule_startup_retry_after_committed_config_patch(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
    ) {
        let Some(target) = self
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if target.health == ConvergenceHealth::RecoveryRequired {
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };
        let identity = match lifecycle.startup_target_identity().await {
            Ok(identity) => identity,
            Err(_) => match lifecycle.unresolved_startup_target_identity().await {
                Some(identity) => identity,
                None => return,
            },
        };
        let target = self
            .deferred
            .as_mut()
            .filter(|target| target.is_reestablish())
            .expect("reestablish target remains attached to the workflow");
        if target.identity != identity {
            *target = DeferredTarget::reestablish(OperationId::generate(), identity, cause);
        } else if target.attempts_remaining == 0 {
            return;
        }
        target.health = ConvergenceHealth::RetryScheduled;
        target.next_attempt = Some(tokio::time::Instant::now());
    }
}

async fn update_reestablish_target(
    lifecycle: &CoreLifecycleWorkflow,
    mut report: StartupReport,
    automatic: bool,
    cause: ReestablishCause,
    deferred: &mut Option<DeferredTarget>,
) -> StartupReport {
    match &report.outcome {
        StartupOutcome::Ready => {
            *deferred = None;
            return report;
        }
        StartupOutcome::RecoveryRequired { .. } | StartupOutcome::Unsettled { .. } => {
            if let Some(target) = deferred.as_mut() {
                target.health = ConvergenceHealth::RecoveryRequired;
                target.next_attempt = None;
            }
            return report;
        }
        StartupOutcome::ReadyDegraded { .. } => {}
    }

    let identity = match lifecycle.startup_target_identity().await {
        Ok(identity) => identity,
        Err(error) => {
            let reason = format!(
                "{}; committed runtime target identity is unavailable: {error:#}",
                report
                    .outcome
                    .failure_reason()
                    .unwrap_or("startup did not converge")
            );
            if let Some(identity) = lifecycle.unresolved_startup_target_identity().await {
                if deferred
                    .as_ref()
                    .is_none_or(|target| target.identity != identity)
                {
                    *deferred = Some(DeferredTarget::reestablish(
                        OperationId::generate(),
                        identity,
                        cause,
                    ));
                }
                if let Some(target) = deferred.as_mut() {
                    target.origin = TargetOrigin::Reestablish(cause);
                    let health = target.schedule_dependency(reason);
                    report.outcome = StartupOutcome::ReadyDegraded {
                        health,
                        reason: target.cause.message.clone(),
                    };
                    return report;
                }
            }
            if let Some(target) = deferred.as_mut().filter(|target| target.is_reestablish()) {
                target.block(reason.clone());
            }
            report.outcome = StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason,
            };
            return report;
        }
    };

    if deferred
        .as_ref()
        .is_none_or(|target| target.identity != identity)
    {
        *deferred = Some(DeferredTarget::reestablish(
            OperationId::generate(),
            identity,
            cause,
        ));
    }
    let target = deferred
        .as_mut()
        .expect("reestablish target was initialized");
    target.origin = TargetOrigin::Reestablish(cause);
    let reason = report
        .outcome
        .failure_reason()
        .unwrap_or("startup did not converge")
        .to_owned();
    let health = match report.outcome {
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::WaitingDependency | ConvergenceHealth::RecoveryRequired,
            ..
        } => target.schedule_dependency(reason.clone()),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::RetryScheduled,
            ..
        } => target.schedule_application(reason.clone(), automatic),
        StartupOutcome::ReadyDegraded { .. } => target.block(reason.clone()),
        StartupOutcome::Ready
        | StartupOutcome::RecoveryRequired { .. }
        | StartupOutcome::Unsettled { .. } => unreachable!("handled above"),
    };
    report.outcome = StartupOutcome::ReadyDegraded {
        health,
        reason: target.cause.message.clone(),
    };
    report
}

#[cfg(test)]
pub(in crate::client) async fn update_startup_retry_target(
    lifecycle: &CoreLifecycleWorkflow,
    mut report: StartupReport,
    automatic: bool,
    allow_receipt_reuse: bool,
    retry_target: &mut Option<StartupRetryTarget>,
) -> StartupReport {
    match &report.outcome {
        StartupOutcome::Ready => {
            *retry_target = None;
            return report;
        }
        StartupOutcome::RecoveryRequired { .. } | StartupOutcome::Unsettled { .. } => {
            if let Some(target) = retry_target.as_mut() {
                target.health = ConvergenceHealth::RecoveryRequired;
                target.next_attempt = None;
            }
            return report;
        }
        StartupOutcome::ReadyDegraded { .. } => {}
    }

    let identity = match lifecycle.startup_target_identity().await {
        Ok(identity) => identity,
        Err(error) => {
            let reason = format!(
                "{}; committed runtime target identity is unavailable: {error:#}",
                report
                    .outcome
                    .failure_reason()
                    .unwrap_or("startup did not converge")
            );
            if let Some(identity) = lifecycle.unresolved_startup_target_identity().await {
                if retry_target
                    .as_ref()
                    .is_none_or(|target| target.identity != identity)
                {
                    *retry_target = Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
                }
                if let Some(target) = retry_target.as_mut() {
                    report.outcome = target.schedule_dependency(reason);
                    return report;
                }
            }
            report.outcome = StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason,
            };
            return report;
        }
    };
    if retry_target
        .as_ref()
        .is_none_or(|target| target.identity != identity)
    {
        *retry_target = Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
    }
    let target = retry_target
        .as_mut()
        .expect("startup retry target was initialized");
    target.allow_receipt_reuse = allow_receipt_reuse;
    let reason = report
        .outcome
        .failure_reason()
        .unwrap_or("startup did not converge")
        .to_owned();
    report.outcome = match report.outcome {
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::WaitingDependency | ConvergenceHealth::RecoveryRequired,
            ..
        } => target.schedule_dependency(reason),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::RetryScheduled,
            ..
        } => target.schedule_application(reason, automatic),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::Blocked,
            ..
        }
        | StartupOutcome::ReadyDegraded { .. } => target.block(reason),
        StartupOutcome::Ready
        | StartupOutcome::RecoveryRequired { .. }
        | StartupOutcome::Unsettled { .. } => {
            unreachable!("handled above")
        }
    };
    report
}

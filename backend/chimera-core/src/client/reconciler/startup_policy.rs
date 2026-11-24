//! Shared startup ownership policy and readiness signal.
#[cfg(any(test, feature = "test-support"))]
use super::reestablish::{DeferredTarget, ReestablishCause, TargetOrigin};
use super::{
    check_record::{RuntimeCheckFailure, TryCauseKind},
    plan::{OwnerPlan, Step, plan_owner as reconcile_plan_owner},
};
use crate::actor_v2::endpoint::ExecutionHost;
#[cfg(any(test, feature = "test-support"))]
use crate::client::convergence::{OutcomeClass, RetryBudget, next_wait};
use crate::client::{convergence::ConvergenceHealth, core_lifecycle::status::CoreStatusSnapshot};
use chimera_core_manager::CoreError;
#[cfg(any(test, feature = "test-support"))]
use chimera_core_manager::OperationId;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Ref S4 only permits a Service-to-Local recovery for a positively
/// refused submission. Missing dependencies, source movement and unknown
/// Core outcomes must leave Service ownership untouched for the next round.
pub fn startup_service_fallback_refused(result: &anyhow::Result<()>, uncertain: bool) -> bool {
    let Err(error) = result else {
        return false;
    };
    !startup_failure_waits_for_dependency(error)
        && matches!(
            crate::client::reconciler::attempt::legacy_error_verdict(error, uncertain, false),
            crate::client::reconciler::attempt::LegacyApplyVerdict::Refused(_)
        )
}

pub fn startup_failure_is_retryable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.retryable)
            || cause
                .downcast_ref::<RuntimeCheckFailure>()
                .is_some_and(|failure| failure.cause() == TryCauseKind::Transient)
    })
}

pub fn startup_failure_waits_for_dependency(error: &anyhow::Error) -> bool {
    if crate::client::reconciler::attempt::waiting_dependency(error) {
        return true;
    }
    error.chain().any(|cause| {
        cause
            .downcast_ref::<RuntimeCheckFailure>()
            .is_some_and(|failure| {
                matches!(
                    failure.record(),
                    super::check_record::CheckRecord::Unserviceable(_)
                )
            })
    })
}

pub use super::proof::host_for as host_for_run_type;

pub fn runtime_evidence(status: &CoreStatusSnapshot) -> RuntimeEvidence {
    use crate::actor_v2::endpoint::ExecutionHost;

    let host = host_for_run_type(status.run_type);
    RuntimeEvidence {
        host,
        state: (host != ExecutionHost::Service || status.state_changed_at > 0)
            .then(|| status.state.clone()),
        state_changed_at: status.state_changed_at,
    }
}

pub use crate::client::core_lifecycle::ownership::StartupOwnerFailure;
pub fn startup_owner_outcome(failure: StartupOwnerFailure) -> StartupOutcome {
    match failure {
        StartupOwnerFailure::WaitingDependency(reason) => StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::WaitingDependency,
            reason,
        },
        StartupOwnerFailure::Blocked(reason) => StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::Blocked,
            reason,
        },
        StartupOwnerFailure::RecoveryRequired(reason) => {
            StartupOutcome::RecoveryRequired { reason }
        }
    }
}

pub use crate::client::reconciler::reestablish::{
    RuntimeEvidence, StartupObservation, StartupReport,
};

/// Startup state owned by ApplicationWorkflow. The owner signal is mirrored
/// through an atomic only for the Service readiness observer outside the actor.
pub struct StartupWorkflowState {
    #[cfg(any(test, feature = "test-support"))]
    retry_target: Option<StartupRetryTarget>,
    owner_unproven: Arc<AtomicBool>,
}

impl Default for StartupWorkflowState {
    fn default() -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            retry_target: None,
            owner_unproven: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl StartupWorkflowState {
    pub fn with_owner_signal(owner_unproven: Arc<AtomicBool>) -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            retry_target: None,
            owner_unproven,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn retry_target(&self) -> Option<&StartupRetryTarget> {
        self.retry_target.as_ref()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn retry_target_mut(&mut self) -> &mut Option<StartupRetryTarget> {
        &mut self.retry_target
    }

    pub fn owner_unproven(&self) -> bool {
        self.owner_unproven.load(Ordering::Acquire)
    }

    pub fn mark_owner_unproven(&self) {
        self.owner_unproven.store(true, Ordering::Release);
    }

    pub fn mark_owner_proven(&self) {
        self.owner_unproven.store(false, Ordering::Release);
    }
}

pub use crate::client::reconciler::startup::StartupOutcome;

/// Compatibility state used only by the test-only Direct CoreLifecycle adapter.
/// Production startup retries use ApplicationWorkflow's shared DeferredTarget.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone)]
pub struct StartupRetryTarget {
    pub identity: String,
    /// Recovery is the only startup origin allowed to keep a verified live
    /// runtime receipt. Preserve this origin across scheduled retries.
    pub allow_receipt_reuse: bool,
    pub cause: String,
    pub budget: RetryBudget,
    pub waits: u8,
    pub health: ConvergenceHealth,
    pub next_attempt: Option<tokio::time::Instant>,
}

#[cfg(any(test, feature = "test-support"))]
impl StartupRetryTarget {
    pub fn new(identity: String, allow_receipt_reuse: bool) -> Self {
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

    pub fn schedule_application(&mut self, reason: String, automatic: bool) -> StartupOutcome {
        self.schedule_application_at(tokio::time::Instant::now(), reason, automatic)
    }

    pub fn schedule_application_at(
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

    pub fn schedule_dependency(&mut self, reason: String) -> StartupOutcome {
        self.schedule_dependency_at(tokio::time::Instant::now(), reason)
    }

    pub fn schedule_dependency_at(
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

    pub fn block(&mut self, reason: String) -> StartupOutcome {
        self.cause.clone_from(&reason);
        self.health = ConvergenceHealth::Blocked;
        self.next_attempt = None;
        StartupOutcome::ReadyDegraded {
            health: self.health,
            reason,
        }
    }

    pub fn begin(&mut self, now: tokio::time::Instant) -> bool {
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

#[cfg(any(test, feature = "test-support"))]
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
            cause: crate::client::reconciler::reestablish::RetryableCause {
                stage: crate::client::reconciler::reestablish::MutationStage::Preparing,
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

#[cfg(any(test, feature = "test-support"))]
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

pub use crate::client::reconciler::service_evidence::{
    ServiceEvidence, reconciler_service_evidence,
};

/// The shared startup rule: Service Mode is a preference. An unusable
/// Service falls back to Local; an unknown transition stays with the current
/// host until the next observation.
pub use crate::client::reconciler::workflow::effective_host;

/// An S2-only `Facts` round from ref. Ownership has not been established
/// until the decision is made; the core's S3/S4 facts are intentionally
/// unreadable here because this routine must NEVER select Apply or Stop.
pub fn s2_step(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: &ServiceEvidence,
    recovering: bool,
) -> Step {
    crate::client::reconciler::decision::plan_owner_boundary(
        desired,
        owner,
        reconciler_service_evidence(service),
        recovering,
    )
}

/// Production startup now first consults the *full* ref `plan(Facts)`
/// decision for owner transitions. For the two waiting owner cells, preserve
/// the typed `OwnerPlan` so the existing StartupReport reports the correct
/// readiness versus unproven-ownership failure without changing its API.
pub fn plan_owner(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: &ServiceEvidence,
) -> OwnerPlan {
    match s2_step(desired, owner, service, false) {
        Step::Owner(action) => action,
        Step::Idle(_) => {
            reconcile_plan_owner(desired, owner, &reconciler_service_evidence(service))
        }
        _ => unreachable!("S2 owner planning must not apply or stop a Core"),
    }
}

/// Readiness work is scheduled only for a concrete ref S2 ownership action,
/// including retirement of an extra owned Service Core when Local is already
/// selected. Waiting and unproven ownership cannot authorize a handoff.
pub fn owner_followup_required(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: &ServiceEvidence,
    recovering: bool,
) -> bool {
    matches!(
        s2_step(desired, owner, service, recovering),
        Step::Owner(
            OwnerPlan::RetireServiceThenLocal
                | OwnerPlan::HandBackToLocal
                | OwnerPlan::AdoptService
        )
    )
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::actor_v2::service_policy::ServicePhase;
    use crate::actor_v2::service_status::ServiceHostStatus;
    use chimera_ipc::{
        api::status::{
            ConfigRevisionInfo, CoreInfos, CoreStateDetail, RuntimeInfos, StatusResBody,
        },
        types::ServiceStatus,
    };

    use super::{
        OwnerPlan, ServiceEvidence, effective_host, owner_followup_required, plan_owner, s2_step,
        startup_service_fallback_refused,
    };
    use crate::actor_v2::endpoint::ExecutionHost::{Local, Service};
    use crate::client::reconciler::plan::{Conclusion, Step};

    #[test]
    fn startup_service_fallback_requires_refused_not_waiting_or_unknown() {
        use chimera_core_manager::{CoreError, CoreErrorKind};

        assert!(!startup_service_fallback_refused(&Ok(()), false));
        for kind in [
            CoreErrorKind::RevisionConflict,
            CoreErrorKind::OperationConflict,
            CoreErrorKind::QueueFull,
            CoreErrorKind::BackendUnavailable,
        ] {
            let awaiting = Err(CoreError::new(kind, "Service dependency pending", false).into());
            assert!(
                !startup_service_fallback_refused(&awaiting, false),
                "{kind:?} may not stop the selected Service Core"
            );
            assert!(!startup_service_fallback_refused(&awaiting, true));
        }
        let source_advanced = Err(anyhow::Error::new(
            crate::client::reconciler::attempt::CommittedTargetAdvanced,
        ));
        assert!(!startup_service_fallback_refused(&source_advanced, false));
        let owner_pending = Err(anyhow::Error::new(
            crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                "Service handoff not confirmed",
            ),
        ));
        assert!(!startup_service_fallback_refused(&owner_pending, false));

        let refusal = Err(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "Service rejected the runtime config",
            false,
        )
        .into());
        assert!(startup_service_fallback_refused(&refusal, false));
        assert!(!startup_service_fallback_refused(&refusal, true));
    }

    #[test]
    fn ref_full_round_is_authoritative_for_all_s2_handoff_actions() {
        use ServicePhase::{DaemonStopped, Ready, Unknown};
        let owned_running = ServiceEvidence::CoreRunning {
            ready: true,
            phase: Ready,
            instance: Some("our-service-core".to_owned()),
            revision: None,
        };
        let stopped = ServiceEvidence::CoreStopped {
            ready: true,
            phase: Ready,
            runtime_owned: true,
        };
        let absent = ServiceEvidence::Absent {
            phase: DaemonStopped,
        };
        let unknown = ServiceEvidence::Unreadable { phase: Unknown };
        for (desired, owner, evidence, expected) in [
            (
                Local,
                Local,
                &owned_running,
                OwnerPlan::RetireServiceThenLocal,
            ),
            (Local, Service, &owned_running, OwnerPlan::HandBackToLocal),
            (Service, Local, &stopped, OwnerPlan::AdoptService),
        ] {
            assert!(
                matches!(s2_step(desired, owner, evidence, false), Step::Owner(actual) if actual == expected),
                "{desired:?}/{owner:?}/{evidence:?}"
            );
            assert_eq!(plan_owner(desired, owner, evidence), expected);
            assert!(owner_followup_required(desired, owner, evidence, false));
        }
        for (desired, owner, evidence) in [
            (Local, Local, &absent),
            (Service, Local, &absent),
            (Service, Local, &unknown),
            (Local, Local, &unknown),
        ] {
            assert!(
                matches!(
                    s2_step(desired, owner, evidence, false),
                    Step::Idle(Conclusion::Waiting { .. })
                ),
                "S2 with unproven ownership must not submit or stop Core: {desired:?}/{owner:?}/{evidence:?}"
            );
            assert!(!owner_followup_required(desired, owner, evidence, false));
        }
        let healthy = ServiceEvidence::CoreRunning {
            ready: true,
            phase: Ready,
            instance: Some("owned".into()),
            revision: None,
        };
        assert!(owner_followup_required(Local, Local, &healthy, false));
        assert!(!owner_followup_required(Local, Local, &healthy, true));
        assert!(matches!(
            s2_step(Local, Local, &healthy, true),
            Step::Idle(Conclusion::Waiting { .. })
        ));
    }

    #[test]
    fn ref_s2_followup_retirement_is_required_even_with_local_already_selected() {
        use ServicePhase::{DaemonStopped, Ready, Unknown};

        let owned_service_core = ServiceEvidence::CoreRunning {
            ready: true,
            phase: Ready,
            instance: Some("owned".into()),
            revision: None,
        };
        assert!(owner_followup_required(
            Local,
            Local,
            &owned_service_core,
            false
        ));
        assert!(owner_followup_required(
            Local,
            Service,
            &owned_service_core,
            false
        ));
        assert!(owner_followup_required(
            Service,
            Local,
            &owned_service_core,
            false
        ));

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
                !owner_followup_required(Local, Local, &service, false),
                "an absent, stopped, foreign or unreadable Service Core must not be retired: {service:?}"
            );
        }
        assert!(!owner_followup_required(
            Service,
            Local,
            &ServiceEvidence::Absent {
                phase: DaemonStopped
            },
            false
        ));
        assert!(!owner_followup_required(
            Service,
            Local,
            &ServiceEvidence::Unreadable { phase: Unknown },
            false
        ));
    }

    fn evidence(
        status: ServiceStatus,
        phase: ServicePhase,
        detail: Option<Option<CoreStateDetail>>,
        owned: bool,
    ) -> ServiceEvidence {
        let server = detail.map(|detail| StatusResBody {
            log_query_version: None,
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

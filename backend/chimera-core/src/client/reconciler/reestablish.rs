//! Shared committed-target retry and recovery evidence.
use super::check_record::TryCauseKind;
use crate::{
    client::runtime::{RuntimeApplyReceipt, RuntimeSnapshot},
    state::DecisionHandle,
};
use chimera_contracts::mutation::ConfigDomain;
use chimera_core_manager::OperationId;
use std::sync::Arc;

/// Runtime state a cancelled Profile mutation must restore. An explicit stop
/// is distinct from a missing receipt: the last successful apply can remain in
/// memory after the user has stopped the core.
#[derive(Debug, Clone)]
pub enum KnownRuntimeState {
    Applied(Arc<RuntimeApplyReceipt>),
    Stopped,
    NeverApplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStage {
    Preparing,
    TryingCritical,
    AwaitDecision,
    Confirming,
    Cancelling,
}

/// Evidence required before submitting a Profile runtime mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceGap {
    BaselineUnconfirmed,
    NoRestorableBaseline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCause {
    Try(TryCauseKind),
    Evidence(EvidenceGap),
}

#[derive(Debug, Clone)]
pub struct ApplyFailure {
    pub stage: MutationStage,
    pub cause: RefusalCause,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct RetryableCause {
    pub stage: MutationStage,
    pub message: String,
}

pub const DEFERRED_RETRY_BUDGET: u8 = 3;

/// Where a committed runtime target came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOrigin {
    Mutation,
    Reestablish(ReestablishCause),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReestablishCause {
    Startup,
    Recovery,
    ExplicitStart,
    ServiceReadiness,
}

/// Chimera's Profile TCC keeps the source decision and rollback baseline with
/// a mutation target until the runtime has settled. Ref's shared target does
/// not need this transaction-only evidence.
#[derive(Debug, Clone)]
pub struct MutationDeferredContext {
    pub digest: String,
    pub baseline: KnownRuntimeState,
    pub domain: ConfigDomain,
    pub decision: DecisionHandle,
}

#[derive(Debug, Clone)]
pub struct DeferredTarget {
    pub operation_id: OperationId,
    pub origin: TargetOrigin,
    pub identity: String,
    pub cause: RetryableCause,
    pub attempts_remaining: u8,
    pub attempts: u32,
    pub waits: u8,
    pub health: crate::client::convergence::ConvergenceHealth,
    pub next_attempt: Option<tokio::time::Instant>,
    pub mutation: Option<MutationDeferredContext>,
}

impl DeferredTarget {
    pub fn reestablish(
        operation_id: OperationId,
        identity: String,
        cause: ReestablishCause,
    ) -> Self {
        Self {
            operation_id,
            origin: TargetOrigin::Reestablish(cause),
            identity,
            cause: RetryableCause {
                stage: MutationStage::Preparing,
                message: "the runtime is being re-established".into(),
            },
            attempts_remaining: crate::client::convergence::RETRY_DELAYS.len() as u8,
            attempts: 0,
            waits: 0,
            health: crate::client::convergence::ConvergenceHealth::Pending,
            next_attempt: None,
            mutation: None,
        }
    }

    pub fn is_reestablish(&self) -> bool {
        matches!(self.origin, TargetOrigin::Reestablish(_))
    }

    pub fn is_mutation(&self) -> bool {
        self.origin == TargetOrigin::Mutation
            && self
                .mutation
                .as_ref()
                .is_some_and(|mutation| mutation.digest == self.identity)
    }

    pub fn allows_receipt_reuse(&self) -> bool {
        self.origin == TargetOrigin::Reestablish(ReestablishCause::Recovery)
    }

    pub fn begin(&mut self, identity: &str, now: tokio::time::Instant) -> bool {
        if !self.is_reestablish()
            || self.identity != identity
            || self.attempts_remaining == 0
            || !matches!(
                self.health,
                crate::client::convergence::ConvergenceHealth::RetryScheduled
                    | crate::client::convergence::ConvergenceHealth::WaitingDependency
            )
            || self.next_attempt.is_none_or(|deadline| deadline > now)
        {
            return false;
        }
        self.next_attempt = None;
        self.health = crate::client::convergence::ConvergenceHealth::Pending;
        true
    }

    pub fn schedule_application(
        &mut self,
        reason: String,
        automatic: bool,
    ) -> crate::client::convergence::ConvergenceHealth {
        self.waits = 0;
        self.cause.message = reason;
        let mut budget = crate::client::convergence::RetryBudget {
            remaining: self.attempts_remaining,
            attempts: self.attempts,
        };
        let delay = if automatic {
            budget.record_automatic_attempt()
        } else {
            budget.next_delay()
        };
        self.attempts_remaining = budget.remaining;
        self.attempts = budget.attempts;
        if let Some(delay) = delay {
            self.health = crate::client::convergence::ConvergenceHealth::RetryScheduled;
            self.next_attempt = Some(tokio::time::Instant::now() + delay);
        } else {
            self.health = crate::client::convergence::ConvergenceHealth::Blocked;
            self.next_attempt = None;
        }
        self.health
    }

    pub fn schedule_dependency(
        &mut self,
        reason: String,
    ) -> crate::client::convergence::ConvergenceHealth {
        if self.attempts_remaining == 0 {
            self.cause.message = format!(
                "{reason}; automatic retries are spent, so retry explicitly once the dependency is available"
            );
            self.health = crate::client::convergence::ConvergenceHealth::Blocked;
            self.next_attempt = None;
            return self.health;
        }
        self.cause.message = reason;
        let (delay, waits) = crate::client::convergence::next_wait(
            self.waits,
            crate::client::convergence::OutcomeClass::Dependency,
        );
        self.waits = waits;
        self.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
        self.next_attempt = Some(tokio::time::Instant::now() + delay);
        self.health
    }

    pub fn block(&mut self, reason: String) -> crate::client::convergence::ConvergenceHealth {
        self.cause.message = reason;
        self.health = crate::client::convergence::ConvergenceHealth::Blocked;
        self.next_attempt = None;
        self.health
    }
}

/// A core-confirmed Profile candidate held until the source transaction
/// publishes its decision. Chimera keeps the private candidate file with the
/// reference-shaped receipt so Confirm can promote it and Cancel can restore
/// the pre-Try baseline without exposing the uncommitted snapshot.
#[derive(Debug, Clone)]
pub struct AppliedCandidate {
    pub replaced: bool,
    pub receipt: Arc<RuntimeApplyReceipt>,
    pub product: Arc<RuntimeSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptStage {
    Preparing,
    TryingCritical,
    AwaitDecision,
    Confirming,
    Cancelling,
    Recovering,
}

impl From<MutationStage> for AttemptStage {
    fn from(stage: MutationStage) -> Self {
        match stage {
            MutationStage::Preparing => Self::Preparing,
            MutationStage::TryingCritical => Self::TryingCritical,
            MutationStage::AwaitDecision => Self::AwaitDecision,
            MutationStage::Confirming => Self::Confirming,
            MutationStage::Cancelling => Self::Cancelling,
        }
    }
}

#[derive(Clone)]
pub struct LiveAttempt {
    pub operation_id: OperationId,
    pub origin: AttemptOrigin,
    pub stage: AttemptStage,
    pub baseline: Option<KnownRuntimeState>,
    /// The receipt the current recovery path can restore. The full candidate
    /// remains available when a prepare failed after the runtime accepted it.
    pub target: Option<Arc<RuntimeApplyReceipt>>,
    /// The applied Try and the remaining work its source commit owes.
    pub verdict: Option<AppliedVerdict>,
    pub unresolved: Option<String>,
}

#[derive(Clone)]
pub enum AttemptOrigin {
    SourceDecision { decision: DecisionHandle },
    CommittedTarget(Box<DeferredTarget>),
    Lifecycle,
}

/// An applied Profile Try, with the effects its source commit still owes.
///
/// The replaced flag is retained for Chimera's runtime adapter, which uses the
/// full candidate when promoting the staged product.
#[derive(Debug, Clone)]
pub struct AppliedVerdict {
    pub receipt: Arc<RuntimeApplyReceipt>,
    pub product: Arc<RuntimeSnapshot>,
    pub releases_service: bool,
    pub replaced: bool,
}

impl AppliedVerdict {
    pub fn new(candidate: &AppliedCandidate, releases_service: bool) -> Self {
        Self {
            receipt: candidate.receipt.clone(),
            product: candidate.product.clone(),
            releases_service,
            replaced: candidate.replaced,
        }
    }

    pub fn candidate(&self) -> AppliedCandidate {
        AppliedCandidate {
            replaced: self.replaced,
            receipt: self.receipt.clone(),
            product: self.product.clone(),
        }
    }

    pub fn with_runtime(
        &self,
        receipt: Arc<RuntimeApplyReceipt>,
        product: Arc<RuntimeSnapshot>,
    ) -> Self {
        Self {
            receipt,
            product,
            releases_service: self.releases_service,
            replaced: self.replaced,
        }
    }
}

impl LiveAttempt {
    pub fn source_decision(operation_id: OperationId, decision: DecisionHandle) -> Self {
        Self::new(operation_id, AttemptOrigin::SourceDecision { decision })
    }

    pub fn committed_target(operation_id: OperationId, target: DeferredTarget) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::CommittedTarget(Box::new(target)),
        )
    }

    pub fn lifecycle(operation_id: OperationId) -> Self {
        // A lifecycle command has already entered its critical execution leg.
        // Match ref's write-ahead stage so an interrupted command is not
        // misreported as an unstarted preparation.
        let mut attempt = Self::new(operation_id, AttemptOrigin::Lifecycle);
        attempt.stage = AttemptStage::TryingCritical;
        attempt
    }

    pub fn target_mut(&mut self) -> Option<&mut DeferredTarget> {
        match &mut self.origin {
            AttemptOrigin::CommittedTarget(target) => Some(target),
            AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => None,
        }
    }

    fn new(operation_id: OperationId, origin: AttemptOrigin) -> Self {
        Self {
            operation_id,
            origin,
            stage: AttemptStage::Preparing,
            baseline: None,
            target: None,
            verdict: None,
            unresolved: None,
        }
    }

    pub fn into_target(self) -> Option<Box<DeferredTarget>> {
        match self.origin {
            AttemptOrigin::CommittedTarget(target) => Some(target),
            AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => None,
        }
    }
}

/// Match ref's positive clearance rule: a successful high-level result does
/// not settle a lower Core owner whose outcome is still unknown.
pub fn clearance_reason(
    unresolved: Option<String>,
    lower_outcome_uncertain: bool,
) -> Option<String> {
    unresolved.or_else(|| {
        lower_outcome_uncertain.then(|| "the lower Core owner has an unsettled action".to_owned())
    })
}

/// A canonical reconciliation may release its native attempt only after the
/// lower owner is settled. Never replay an old source transaction in production.
pub fn reconciled_native_attempt(
    attempt: Option<&LiveAttempt>,
    lower_uncertain: bool,
) -> anyhow::Result<Option<OperationId>> {
    let Some(attempt) = attempt.filter(|attempt| attempt.unresolved.is_some()) else {
        return Ok(None);
    };
    anyhow::ensure!(
        !lower_uncertain,
        "runtime still needs recovery after {:?}: {}",
        attempt.stage,
        attempt.unresolved.as_deref().unwrap_or_default()
    );
    anyhow::ensure!(
        match &attempt.origin {
            AttemptOrigin::Lifecycle => true,
            AttemptOrigin::CommittedTarget(target) => target.is_reestablish(),
            AttemptOrigin::SourceDecision { .. } => false,
        },
        "legacy source decision cannot be cleared by native recovery"
    );
    Ok(Some(attempt.operation_id))
}

use super::{service_evidence::ServiceEvidence, startup::StartupOutcome};
use crate::client::convergence::ConvergenceHealth;
use chimera_contracts::execution_host::ExecutionHost;
use chimera_ipc::api::status::CoreState;
/// What StartupReconcile observed and how it concluded.
#[derive(Debug, Clone)]
pub struct StartupReport {
    /// The CoreLifecycle actor's operation sequence number.
    pub operation_id: u64,
    /// `None` when the workflow could not obtain a trustworthy observation.
    pub observation: Option<StartupObservation>,
    pub outcome: StartupOutcome,
}

#[derive(Debug, Clone)]
pub struct StartupObservation {
    pub desired: ExecutionHost,
    pub service: Option<ServiceEvidence>,
    /// Chimera's current app-side Core status projection is narrower than
    /// ref's CoreStatusProjection. A missing state on the Service host means
    /// the legacy adapter supplied its timestamp-zero unreachable sentinel.
    pub runtime: Option<RuntimeEvidence>,
}

#[derive(Debug, Clone)]
pub struct RuntimeEvidence {
    pub host: ExecutionHost,
    pub state: Option<CoreState>,
    pub state_changed_at: i64,
}

pub async fn update_reestablish_target<F, U>(
    target_identity: impl FnOnce() -> F,
    unresolved_identity: impl FnOnce() -> U,
    mut report: StartupReport,
    automatic: bool,
    cause: ReestablishCause,
    deferred: &mut Option<DeferredTarget>,
) -> StartupReport
where
    F: std::future::Future<Output = anyhow::Result<String>>,
    U: std::future::Future<Output = Option<String>>,
{
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

    let identity = match target_identity().await {
        Ok(identity) => identity,
        Err(error) => {
            let reason = format!(
                "{}; committed runtime target identity is unavailable: {error:#}",
                report
                    .outcome
                    .failure_reason()
                    .unwrap_or("startup did not converge")
            );
            if let Some(identity) = unresolved_identity().await {
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

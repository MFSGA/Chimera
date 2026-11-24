//! Shared live-attempt, retry and recovery progress for the single reconciler.
//! No platform operations or task spawning: the mailbox owns execution.
use super::{
    reestablish::{
        AppliedVerdict, AttemptOrigin, AttemptStage, DeferredTarget, KnownRuntimeState,
        LiveAttempt, MutationStage, TargetOrigin,
    },
    startup_policy::{StartupReport, StartupWorkflowState},
};
use crate::{client::runtime::RuntimeApplyReceipt, state::DecisionHandle};
use chimera_contracts::mutation::ConfigDomain;
use chimera_core_manager::OperationId;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCommand {
    SourceOnly,
    RecoverRuntime,
    RecoverStartup,
    Other,
}

/// What recovery needs to restore one Profile mutation after its source
/// decision and runtime effect stopped agreeing.
#[derive(Debug, Clone)]
pub struct RecoveryContext {
    pub operation_id: Option<OperationId>,
    pub runtime_operation: Option<OperationId>,
    pub domain: ConfigDomain,
    pub stage: MutationStage,
    pub decision: Option<DecisionHandle>,
    pub baseline: Option<KnownRuntimeState>,
    pub target: Option<Arc<RuntimeApplyReceipt>>,
    pub error: String,
}

pub struct WorkflowProgress {
    pub deferred: Option<DeferredTarget>,
    pub live: Option<LiveAttempt>,
    pub startup_report: Option<StartupReport>,
    pub startup: StartupWorkflowState,
    pub recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
}

impl WorkflowProgress {
    pub fn new(recovery_required: Arc<Mutex<Option<RecoveryContext>>>) -> Self {
        Self {
            deferred: None,
            live: None,
            startup_report: None,
            startup: StartupWorkflowState::default(),
            recovery_required,
        }
    }

    pub fn recovery_command_allowed(&self, command: RecoveryCommand) -> bool {
        let Some(attempt) = self
            .live
            .as_ref()
            .filter(|attempt| attempt.unresolved.is_some())
        else {
            return true;
        };
        if command == RecoveryCommand::SourceOnly {
            return true;
        }
        match &attempt.origin {
            AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => {
                command == RecoveryCommand::RecoverRuntime
            }
            AttemptOrigin::CommittedTarget(target) if target.is_mutation() => {
                command == RecoveryCommand::RecoverRuntime
            }
            AttemptOrigin::CommittedTarget(target)
                if matches!(target.origin, TargetOrigin::Reestablish(_)) =>
            {
                command == RecoveryCommand::RecoverStartup
            }
            AttemptOrigin::CommittedTarget(_) => false,
        }
    }
    pub fn has_unresolved_attempt(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|attempt| attempt.unresolved.is_some())
    }

    pub fn startup_owner_unproven(&self) -> bool {
        self.startup.owner_unproven()
    }

    pub fn clear_startup_retry_target(&mut self) {
        if self
            .deferred
            .as_ref()
            .is_some_and(DeferredTarget::is_reestablish)
        {
            self.deferred = None;
        }
    }

    pub fn startup_report(&self) -> Option<StartupReport> {
        self.startup_report.clone()
    }

    pub fn startup_retry_schedule(&self) -> Option<(String, tokio::time::Instant)> {
        self.deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
            .and_then(|target| {
                target
                    .next_attempt
                    .map(|deadline| (target.identity.clone(), deadline))
            })
    }

    pub fn startup_retry_due(&self, identity: &str) -> bool {
        self.deferred.as_ref().is_some_and(|target| {
            target.is_reestablish()
                && target.identity == identity
                && target
                    .next_attempt
                    .is_some_and(|deadline| deadline <= tokio::time::Instant::now())
                && target.attempts_remaining > 0
        })
    }

    pub fn begin_startup_retry(&mut self, identity: &str) -> bool {
        self.deferred
            .as_mut()
            .is_some_and(|target| target.begin(identity, tokio::time::Instant::now()))
    }

    pub fn defer_startup_retry(&mut self, identity: &str, reason: String) {
        if let Some(target) = self
            .deferred
            .as_mut()
            .filter(|target| target.is_reestablish() && target.identity == identity)
        {
            target.schedule_dependency(reason);
        }
    }

    pub fn deferred_retry(&self) -> Option<(String, tokio::time::Instant)> {
        self.deferred.as_ref().and_then(|deferred| {
            (deferred.is_mutation())
                .then(|| {
                    deferred
                        .next_attempt
                        .map(|next_attempt| (deferred.identity.clone(), next_attempt))
                })
                .flatten()
        })
    }

    pub fn begin_deferred_retry(&mut self, digest: &str) -> bool {
        let Some(deferred) = &mut self.deferred else {
            return false;
        };
        if !deferred.is_mutation()
            || deferred.identity != digest
            || deferred.attempts_remaining == 0
            || deferred.health != crate::client::convergence::ConvergenceHealth::RetryScheduled
            || deferred
                .next_attempt
                .is_none_or(|next_attempt| next_attempt > tokio::time::Instant::now())
        {
            return false;
        }
        deferred.next_attempt = None;
        deferred.health = crate::client::convergence::ConvergenceHealth::Pending;
        true
    }

    pub async fn finish_deferred_retry(
        &mut self,
        digest: &str,
        succeeded: bool,
        retryable: bool,
        waiting_dependency: bool,
        uncertain: bool,
        error: Option<String>,
    ) -> Option<tokio::time::Instant> {
        let deferred = self.deferred.as_mut()?;
        if !deferred.is_mutation() || deferred.identity != digest {
            return None;
        }
        if succeeded {
            self.deferred = None;
            return None;
        }
        if let Some(error) = &error {
            deferred.cause.message = error.clone();
        }
        if uncertain {
            deferred.health = crate::client::convergence::ConvergenceHealth::RecoveryRequired;
            deferred.next_attempt = None;
            let target = self
                .deferred
                .take()
                .expect("the mutation target remains available during retry completion");
            let baseline = target
                .mutation
                .as_ref()
                .expect("mutation deferred target carries its transaction context")
                .baseline
                .clone();
            self.live = Some(LiveAttempt::committed_target(target.operation_id, target));
            self.advance_attempt(AttemptStage::TryingCritical);
            self.record_attempt_baseline(&baseline);
            let reason = error.unwrap_or_else(|| {
                "deferred Profile runtime retry has an uncertain outcome".into()
            });
            super::reestablish::conclude_attempt(
                &mut self.live,
                &mut self.deferred,
                Some(reason),
                true,
            );
            self.publish_recovery_context().await;
            return None;
        }
        if waiting_dependency {
            deferred.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
            let next_attempt = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            deferred.next_attempt = Some(next_attempt);
            return Some(next_attempt);
        }
        if !retryable {
            deferred.health = crate::client::convergence::ConvergenceHealth::Blocked;
            deferred.next_attempt = None;
            return None;
        }
        let mut budget = crate::client::convergence::RetryBudget {
            remaining: deferred.attempts_remaining,
            attempts: deferred.attempts,
        };
        let delay = budget.record_automatic_attempt();
        deferred.attempts = budget.attempts;
        deferred.attempts_remaining = budget.remaining;
        if let Some(delay) = delay {
            deferred.health = crate::client::convergence::ConvergenceHealth::RetryScheduled;
            let next_attempt = tokio::time::Instant::now() + delay;
            deferred.next_attempt = Some(next_attempt);
            Some(next_attempt)
        } else {
            deferred.health = crate::client::convergence::ConvergenceHealth::Blocked;
            deferred.next_attempt = None;
            None
        }
    }

    pub fn reschedule_deferred_retry(
        &mut self,
        digest: &str,
        delay: std::time::Duration,
    ) -> Option<tokio::time::Instant> {
        let deferred = self.deferred.as_mut()?;
        if !deferred.is_mutation()
            || deferred.identity != digest
            || deferred.attempts_remaining == 0
        {
            return None;
        }
        let next_attempt = tokio::time::Instant::now() + delay;
        deferred.health = crate::client::convergence::ConvergenceHealth::RetryScheduled;
        deferred.next_attempt = Some(next_attempt);
        Some(next_attempt)
    }

    pub fn wait_for_deferred_dependency(&mut self, digest: &str) {
        if let Some(deferred) = self
            .deferred
            .as_mut()
            .filter(|deferred| deferred.is_mutation() && deferred.identity == digest)
        {
            deferred.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
            deferred.next_attempt = None;
        }
    }

    pub fn runtime_reconciled(&mut self) {
        self.deferred = None;
    }

    pub async fn publish_recovery_context(&self) -> Option<RecoveryContext> {
        let context = self.live.as_ref().and_then(|attempt| {
            let error = attempt.unresolved.clone()?;
            let (decision, domain, baseline) = match &attempt.origin {
                AttemptOrigin::SourceDecision { decision } => (
                    Some(decision.clone()),
                    ConfigDomain::Profiles,
                    attempt.baseline.clone(),
                ),
                AttemptOrigin::CommittedTarget(target) => {
                    let mutation = target.mutation.as_ref()?;
                    (
                        Some(mutation.decision.clone()),
                        mutation.domain,
                        Some(mutation.baseline.clone()),
                    )
                }
                AttemptOrigin::Lifecycle => return None,
            };
            Some(RecoveryContext {
                operation_id: Some(attempt.operation_id),
                runtime_operation: None,
                domain,
                stage: match attempt.stage {
                    AttemptStage::Preparing => MutationStage::Preparing,
                    AttemptStage::TryingCritical => MutationStage::TryingCritical,
                    AttemptStage::AwaitDecision => MutationStage::AwaitDecision,
                    AttemptStage::Confirming => MutationStage::Confirming,
                    AttemptStage::Cancelling => MutationStage::Cancelling,
                    AttemptStage::Recovering => MutationStage::TryingCritical,
                },
                decision,
                baseline,
                target: attempt.target.clone(),
                error,
            })
        });
        *self.recovery_required.lock().await = context.clone();
        context
    }
    pub fn advance_attempt(&mut self, stage: AttemptStage) {
        if let Some(attempt) = &mut self.live {
            attempt.stage = stage;
        }
    }

    pub fn advance_attempt_for(&mut self, operation_id: &OperationId, stage: AttemptStage) {
        if let Some(attempt) = self
            .live
            .as_mut()
            .filter(|attempt| &attempt.operation_id == operation_id)
        {
            attempt.stage = stage;
        }
    }

    pub fn record_attempt_baseline(&mut self, baseline: &KnownRuntimeState) {
        if let Some(attempt) = &mut self.live {
            attempt.baseline = Some(baseline.clone());
        }
    }

    pub fn record_attempt_verdict(&mut self, operation_id: &OperationId, verdict: AppliedVerdict) {
        if let Some(attempt) = self
            .live
            .as_mut()
            .filter(|attempt| &attempt.operation_id == operation_id)
        {
            attempt.verdict = Some(verdict);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::reestablish::{MutationDeferredContext, ReestablishCause, RetryableCause};
    use super::*;
    use crate::client::convergence::ConvergenceHealth;
    use crate::state::decision::DecisionWriter;

    fn progress() -> WorkflowProgress {
        WorkflowProgress::new(Arc::new(Mutex::new(None)))
    }

    fn mutation_target() -> DeferredTarget {
        DeferredTarget {
            operation_id: OperationId::generate(),
            origin: TargetOrigin::Mutation,
            identity: "profile".into(),
            cause: RetryableCause {
                stage: MutationStage::TryingCritical,
                message: "initial refusal".into(),
            },
            attempts_remaining: 3,
            attempts: 0,
            waits: 0,
            health: ConvergenceHealth::RetryScheduled,
            next_attempt: Some(tokio::time::Instant::now()),
            mutation: Some(MutationDeferredContext {
                digest: "profile".into(),
                baseline: KnownRuntimeState::Stopped,
                domain: ConfigDomain::Profiles,
                decision: DecisionWriter::new().handle(),
            }),
        }
    }

    #[tokio::test]
    async fn dependency_wait_preserves_budget_and_stale_completion_does_nothing() {
        let mut state = progress();
        state.deferred = Some(mutation_target());
        assert!(!state.begin_deferred_retry("stale"));
        assert!(state.begin_deferred_retry("profile"));
        assert!(
            state
                .finish_deferred_retry("stale", true, false, false, false, None)
                .await
                .is_none()
        );
        assert_eq!(
            state.deferred.as_ref().unwrap().health,
            ConvergenceHealth::Pending
        );
        assert!(
            state
                .finish_deferred_retry("profile", false, false, true, false, None)
                .await
                .is_some()
        );
        let target = state.deferred.as_ref().unwrap();
        assert_eq!(target.health, ConvergenceHealth::WaitingDependency);
        assert_eq!((target.attempts, target.attempts_remaining), (0, 3));
        assert!(!state.begin_deferred_retry("profile"));
        state
            .reschedule_deferred_retry("profile", std::time::Duration::ZERO)
            .unwrap();
        assert!(state.begin_deferred_retry("profile"));
        state
            .finish_deferred_retry("profile", false, true, false, false, None)
            .await;
        let target = state.deferred.as_ref().unwrap();
        assert_eq!((target.attempts, target.attempts_remaining), (1, 2));
    }

    #[tokio::test]
    async fn unknown_retry_preserves_transaction_evidence_and_blocks_unrelated_commands() {
        let mut state = progress();
        let target = mutation_target();
        let id = target.operation_id;
        state.deferred = Some(target);
        assert!(state.begin_deferred_retry("profile"));
        assert!(
            state
                .finish_deferred_retry(
                    "profile",
                    false,
                    true,
                    false,
                    true,
                    Some("unknown outcome".into())
                )
                .await
                .is_none()
        );
        assert!(state.deferred.is_none());
        assert!(state.has_unresolved_attempt());
        assert!(state.recovery_command_allowed(RecoveryCommand::SourceOnly));
        assert!(state.recovery_command_allowed(RecoveryCommand::RecoverRuntime));
        assert!(!state.recovery_command_allowed(RecoveryCommand::RecoverStartup));
        assert!(!state.recovery_command_allowed(RecoveryCommand::Other));
        let context = state.recovery_required.lock().await.clone().unwrap();
        assert_eq!(context.operation_id, Some(id));
        assert_eq!(context.error, "unknown outcome");
        assert!(context.decision.is_some());
        assert!(matches!(context.baseline, Some(KnownRuntimeState::Stopped)));
    }

    #[test]
    fn startup_recovery_does_not_admit_ordinary_runtime_recovery() {
        let mut state = progress();
        let target = DeferredTarget::reestablish(
            OperationId::generate(),
            "startup".into(),
            ReestablishCause::Startup,
        );
        let mut live = LiveAttempt::committed_target(target.operation_id, target);
        live.unresolved = Some("startup owner unknown".into());
        state.live = Some(live);
        assert!(state.recovery_command_allowed(RecoveryCommand::RecoverStartup));
        assert!(!state.recovery_command_allowed(RecoveryCommand::RecoverRuntime));
        assert!(!state.recovery_command_allowed(RecoveryCommand::Other));
    }
}

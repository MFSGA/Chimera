//! Write-ahead state for the operation that currently owns the runtime.
//!
//! This follows ref's `LiveAttempt` and `AttemptOrigin` boundary. Chimera keeps
//! the Profile runtime receipt needed by its existing rollback adapter here;
//! the externally shared `RecoveryContext` is only a reporting projection.

use std::sync::Arc;

use chimera_core::state::DecisionHandle;
use chimera_core_manager::OperationId;

use super::{
    mutation::{AppliedCandidate, DeferredTarget, KnownRuntimeState, MutationStage},
    workflow::ApplicationWorkflow,
};
use crate::client::runtime::{RuntimeApplyReceipt, RuntimeSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptStage {
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
pub(crate) struct LiveAttempt {
    pub(crate) operation_id: OperationId,
    pub(crate) origin: AttemptOrigin,
    pub(crate) stage: AttemptStage,
    pub(crate) baseline: Option<KnownRuntimeState>,
    /// The receipt the current recovery path can restore. The full candidate
    /// remains available when a prepare failed after the runtime accepted it.
    pub(crate) target: Option<Arc<RuntimeApplyReceipt>>,
    /// The applied Try and the remaining work its source commit owes.
    pub(crate) verdict: Option<AppliedVerdict>,
    pub(crate) unresolved: Option<String>,
}

#[derive(Clone)]
pub(crate) enum AttemptOrigin {
    SourceDecision { decision: DecisionHandle },
    CommittedTarget(Box<DeferredTarget>),
    Lifecycle,
}

/// An applied Profile Try, with the effects its source commit still owes.
///
/// The replaced flag is retained for Chimera's runtime adapter, which uses the
/// full candidate when promoting the staged product.
#[derive(Debug, Clone)]
pub(crate) struct AppliedVerdict {
    pub(crate) receipt: Arc<RuntimeApplyReceipt>,
    pub(crate) product: Arc<RuntimeSnapshot>,
    pub(crate) releases_service: bool,
    pub(crate) replaced: bool,
}

impl AppliedVerdict {
    pub(crate) fn new(candidate: &AppliedCandidate, releases_service: bool) -> Self {
        Self {
            receipt: candidate.receipt.clone(),
            product: candidate.product.clone(),
            releases_service,
            replaced: candidate.replaced,
        }
    }

    pub(crate) fn candidate(&self) -> AppliedCandidate {
        AppliedCandidate {
            replaced: self.replaced,
            receipt: self.receipt.clone(),
            product: self.product.clone(),
        }
    }

    pub(crate) fn with_runtime(
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
    pub(crate) fn source_decision(operation_id: OperationId, decision: DecisionHandle) -> Self {
        Self::new(operation_id, AttemptOrigin::SourceDecision { decision })
    }

    pub(crate) fn committed_target(operation_id: OperationId, target: DeferredTarget) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::CommittedTarget(Box::new(target)),
        )
    }

    pub(crate) fn lifecycle(operation_id: OperationId) -> Self {
        Self::new(operation_id, AttemptOrigin::Lifecycle)
    }

    pub(crate) fn target_mut(&mut self) -> Option<&mut DeferredTarget> {
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

    pub(crate) fn into_target(self) -> Option<Box<DeferredTarget>> {
        match self.origin {
            AttemptOrigin::CommittedTarget(target) => Some(target),
            AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => None,
        }
    }
}

impl ApplicationWorkflow {
    pub(crate) fn advance_attempt(&mut self, stage: AttemptStage) {
        if let Some(attempt) = &mut self.live {
            attempt.stage = stage;
        }
    }

    pub(crate) fn advance_attempt_for(&mut self, operation_id: &OperationId, stage: AttemptStage) {
        if let Some(attempt) = self
            .live
            .as_mut()
            .filter(|attempt| &attempt.operation_id == operation_id)
        {
            attempt.stage = stage;
        }
    }

    pub(crate) fn record_attempt_baseline(&mut self, baseline: &KnownRuntimeState) {
        if let Some(attempt) = &mut self.live {
            attempt.baseline = Some(baseline.clone());
        }
    }

    pub(crate) fn record_attempt_verdict(
        &mut self,
        operation_id: &OperationId,
        verdict: AppliedVerdict,
    ) {
        if let Some(attempt) = self
            .live
            .as_mut()
            .filter(|attempt| &attempt.operation_id == operation_id)
        {
            attempt.verdict = Some(verdict);
        }
    }

    pub(crate) fn conclude_attempt(&mut self, unresolved: Option<String>) -> bool {
        match unresolved {
            None => {
                if let Some(target) = self.live.take().and_then(LiveAttempt::into_target) {
                    self.deferred = Some(*target);
                }
                true
            }
            Some(reason) => {
                if let Some(attempt) = &mut self.live {
                    attempt.unresolved = Some(reason);
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    struct NoopTransition;

    #[async_trait::async_trait]
    impl crate::client::core_lifecycle::ServiceTransitionLease for NoopTransition {
        async fn install_daemon(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update_daemon(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn start_daemon(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn restart_daemon(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop_daemon(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn confirm_ready(&mut self, _timeout: std::time::Duration) -> anyhow::Result<()> {
            Ok(())
        }

        async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn unresolved_attempt_only_admits_its_explicit_recovery_command() {
        let mut workflow = ApplicationWorkflow::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(None)),
            None,
        );
        workflow.live = Some(LiveAttempt::lifecycle(OperationId::generate()));
        assert!(!workflow.conclude_attempt(Some("runtime outcome is unresolved".into())));

        assert!(workflow.has_unresolved_attempt());
        assert!(
            workflow.recovery_command_allowed(&crate::client::core_lifecycle::Command::RecoverCore)
        );
        assert!(
            workflow.recovery_command_allowed(&crate::client::core_lifecycle::Command::Reconcile)
        );
    }

    #[test]
    fn a_settled_attempt_releases_the_live_slot() {
        let mut workflow = ApplicationWorkflow::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(None)),
            None,
        );
        workflow.live = Some(LiveAttempt::lifecycle(OperationId::generate()));

        assert!(workflow.conclude_attempt(None));
        assert!(workflow.live.is_none());
        assert!(!workflow.has_unresolved_attempt());
    }

    #[test]
    fn unresolved_startup_target_requires_recovery_instead_of_a_retry_tick() {
        let mut workflow = ApplicationWorkflow::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(None)),
            None,
        );
        let operation_id = OperationId::generate();
        let target = DeferredTarget::reestablish(
            operation_id,
            "startup-target".into(),
            super::super::mutation::ReestablishCause::Recovery,
        );
        workflow.live = Some(LiveAttempt::committed_target(operation_id, target));
        workflow.conclude_attempt(Some("startup result is unresolved".into()));

        let recover = crate::client::core_lifecycle::Command::RecoverStartup {
            operation_id: 1,
            transition: Box::new(NoopTransition),
        };
        let retry_tick = crate::client::core_lifecycle::Command::RetryStartup {
            operation_id: 1,
            identity: "startup-target".into(),
            transition: Box::new(NoopTransition),
        };

        assert!(workflow.recovery_command_allowed(&recover));
        assert!(!workflow.recovery_command_allowed(&retry_tick));
    }

    #[tokio::test]
    async fn recording_interruption_preserves_the_current_attempt_stage() {
        let mut workflow = ApplicationWorkflow::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(None)),
            None,
        );
        workflow.live = Some(LiveAttempt::lifecycle(OperationId::generate()));
        workflow.advance_attempt(AttemptStage::Confirming);

        workflow
            .mark_recovery_required("operation was interrupted".into())
            .await;

        let attempt = workflow.live.as_ref().expect("attempt remains recoverable");
        assert_eq!(attempt.stage, AttemptStage::Confirming);
        assert!(attempt.unresolved.is_some());
    }
}

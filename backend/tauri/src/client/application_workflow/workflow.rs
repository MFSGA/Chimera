//! The tracked application workflow state for Profile mutations.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core::state::DecisionHandle;
use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::core_lifecycle::{CoreLifecycleStatus, ports::CoreStatusSnapshot};

use super::{
    inputs::RuntimeInputs,
    mutation::{
        AppliedCandidate, CheckRecord, ConfigDomain, DeferredTarget, KnownRuntimeState,
        MutationReceipt, MutationStage,
    },
};
use crate::client::runtime::RuntimeApplyReceipt;

/// What recovery needs to restore one Profile mutation after its source
/// decision and runtime effect stopped agreeing.
#[derive(Debug, Clone)]
pub(crate) struct RecoveryContext {
    pub(crate) operation_id: Option<OperationId>,
    pub(crate) runtime_operation: Option<OperationId>,
    pub(crate) domain: ConfigDomain,
    pub(crate) stage: MutationStage,
    pub(crate) decision: Option<DecisionHandle>,
    pub(crate) baseline: Option<KnownRuntimeState>,
    pub(crate) target: Option<Arc<RuntimeApplyReceipt>>,
    pub(crate) error: String,
}

#[async_trait::async_trait]
pub(crate) trait ProfileRuntime: Send + Sync + 'static {
    fn status(&self) -> CoreLifecycleStatus;
    async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn capture_profile_inputs(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<RuntimeInputs>;
    async fn observe_runtime_baseline(&self) -> anyhow::Result<KnownRuntimeState>;
    async fn restore_runtime_baseline(&self, baseline: &KnownRuntimeState) -> anyhow::Result<()>;
    async fn validate_profile_runtime(&self, inputs: RuntimeInputs) -> anyhow::Result<CheckRecord>;
    async fn prepare_profile_runtime(
        &self,
        inputs: RuntimeInputs,
        operation_id: &OperationId,
    ) -> anyhow::Result<(AppliedCandidate, CheckRecord)>;
    async fn confirm_profile_runtime(
        &self,
        operation_id: &OperationId,
        candidate: AppliedCandidate,
    ) -> anyhow::Result<()>;
    async fn discard_profile_runtime(&self, operation_id: &OperationId);
    async fn reconcile_profiles(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()>;
    fn request_runtime_rebuild(&self);
}

pub(in crate::client) struct ApplicationWorkflow {
    pub(super) outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
    pub(super) recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
    pub(super) notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    pub(super) deferred: Option<DeferredTarget>,
}

impl ApplicationWorkflow {
    pub(super) fn new(
        outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    ) -> Self {
        Self {
            outcomes,
            recovery_required,
            notifications,
            deferred: None,
        }
    }

    pub(in crate::client) fn deferred_retry(&self) -> Option<(String, tokio::time::Instant)> {
        self.deferred.as_ref().and_then(|deferred| {
            deferred
                .next_attempt
                .map(|next_attempt| (deferred.digest.clone(), next_attempt))
        })
    }

    pub(in crate::client) fn begin_deferred_retry(&mut self, digest: &str) -> bool {
        let Some(deferred) = &mut self.deferred else {
            return false;
        };
        if deferred.digest != digest
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

    pub(in crate::client) async fn finish_deferred_retry(
        &mut self,
        digest: &str,
        succeeded: bool,
        retryable: bool,
        waiting_dependency: bool,
        uncertain: bool,
        error: Option<String>,
    ) -> Option<tokio::time::Instant> {
        let deferred = self.deferred.as_mut()?;
        if deferred.digest != digest {
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
            let context = RecoveryContext {
                operation_id: Some(deferred.operation_id.clone()),
                runtime_operation: None,
                domain: deferred.domain,
                stage: MutationStage::TryingCritical,
                decision: Some(deferred.decision.clone()),
                baseline: Some(deferred.baseline.clone()),
                target: None,
                error: error.unwrap_or_else(|| {
                    "deferred Profile runtime retry has an uncertain outcome".into()
                }),
            };
            *self.recovery_required.lock().await = Some(context);
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

    pub(in crate::client) fn reschedule_deferred_retry(
        &mut self,
        digest: &str,
        delay: std::time::Duration,
    ) -> Option<tokio::time::Instant> {
        let deferred = self.deferred.as_mut()?;
        if deferred.digest != digest || deferred.attempts_remaining == 0 {
            return None;
        }
        let next_attempt = tokio::time::Instant::now() + delay;
        deferred.health = crate::client::convergence::ConvergenceHealth::RetryScheduled;
        deferred.next_attempt = Some(next_attempt);
        Some(next_attempt)
    }

    pub(in crate::client) fn wait_for_deferred_dependency(&mut self, digest: &str) {
        if let Some(deferred) = self
            .deferred
            .as_mut()
            .filter(|deferred| deferred.digest == digest)
        {
            deferred.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
            deferred.next_attempt = None;
        }
    }

    pub(in crate::client) fn runtime_reconciled(&mut self) {
        self.deferred = None;
    }

    pub(in crate::client) async fn mark_recovery_required(
        &self,
        message: String,
    ) -> RecoveryContext {
        let context = RecoveryContext {
            operation_id: None,
            runtime_operation: None,
            domain: ConfigDomain::Profiles,
            stage: MutationStage::TryingCritical,
            decision: None,
            baseline: None,
            target: None,
            error: message,
        };
        *self.recovery_required.lock().await = Some(context.clone());
        context
    }

    pub(in crate::client) async fn mark_recovery_with_attempt(
        &self,
        operation_id: OperationId,
        baseline: Option<KnownRuntimeState>,
        decision: DecisionHandle,
        target: Option<Arc<RuntimeApplyReceipt>>,
        stage: MutationStage,
        message: String,
    ) -> RecoveryContext {
        let context = RecoveryContext {
            operation_id: Some(operation_id),
            runtime_operation: None,
            domain: ConfigDomain::Profiles,
            stage,
            decision: Some(decision),
            baseline,
            target,
            error: message,
        };
        *self.recovery_required.lock().await = Some(context.clone());
        context
    }
}

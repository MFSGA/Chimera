//! The tracked application workflow state for startup and Profile mutations.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core::state::DecisionHandle;
use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::core_lifecycle::{
    Command as CoreCommand, CoreLifecycleStatus, CoreLifecycleWorkflow, ports::CoreStatusSnapshot,
};

use super::{
    inputs::RuntimeInputs,
    mutation::{
        AppliedCandidate, CheckRecord, ConfigDomain, DeferredTarget, KnownRuntimeState,
        MutationReceipt, MutationStage,
    },
    startup::{StartupOutcome, StartupReport, StartupWorkflowState},
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
    async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        None
    }
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
    startup_report: Option<StartupReport>,
    startup: StartupWorkflowState,
    /// Startup and Profile mutations share the application workflow actor;
    /// CoreLifecycleWorkflow adapts its existing runtime and Service ports.
    lifecycle: Option<Arc<CoreLifecycleWorkflow>>,
    mutation_connected: bool,
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
            startup_report: None,
            startup: StartupWorkflowState::default(),
            lifecycle: None,
            mutation_connected: false,
        }
    }

    pub(in crate::client) fn with_lifecycle(lifecycle: CoreLifecycleWorkflow) -> Self {
        let startup =
            StartupWorkflowState::with_owner_signal(lifecycle.startup_owner_unproven_handle());
        Self {
            lifecycle: Some(Arc::new(lifecycle)),
            startup,
            ..Self::new(
                Arc::new(Mutex::new(HashMap::new())),
                Arc::new(Mutex::new(None)),
                None,
            )
        }
    }

    pub(in crate::client) fn lifecycle(&self) -> &CoreLifecycleWorkflow {
        self.lifecycle
            .as_deref()
            .expect("actor-owned application workflow has a lifecycle adapter")
    }

    pub(in crate::client) fn mutation_connected(&self) -> bool {
        self.mutation_connected
    }

    pub(in crate::client) fn startup_owner_unproven(&self) -> bool {
        self.startup.owner_unproven()
    }

    pub(in crate::client) fn clear_startup_retry_target(&mut self) {
        *self.startup.retry_target_mut() = None;
    }

    #[cfg(test)]
    pub(crate) fn for_startup_retry_tests() -> Self {
        Self::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(None)),
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn set_startup_retry_target_for_tests(
        &mut self,
        target: super::startup::StartupRetryTarget,
    ) {
        *self.startup.retry_target_mut() = Some(target);
    }

    pub(in crate::client) fn startup_report(&self) -> Option<StartupReport> {
        self.startup_report.clone()
    }

    pub(in crate::client) fn startup_retry_schedule(
        &self,
    ) -> Option<(String, tokio::time::Instant)> {
        self.startup.retry_target().and_then(|target| {
            target
                .next_attempt
                .map(|deadline| (target.identity.clone(), deadline))
        })
    }

    pub(in crate::client) fn startup_retry_due(&self, identity: &str) -> bool {
        self.startup.retry_target().is_some_and(|target| {
            target.identity == identity
                && target
                    .next_attempt
                    .is_some_and(|deadline| deadline <= tokio::time::Instant::now())
                && target.budget.remaining > 0
        })
    }

    pub(in crate::client) fn begin_startup_retry(&mut self, identity: &str) -> bool {
        self.startup
            .retry_target_mut()
            .as_mut()
            .is_some_and(|target| {
                target.identity == identity && target.begin(tokio::time::Instant::now())
            })
    }

    pub(in crate::client) fn defer_startup_retry(&mut self, identity: &str, reason: String) {
        if let Some(target) = self
            .startup
            .retry_target_mut()
            .as_mut()
            .filter(|target| target.identity == identity)
        {
            target.schedule_dependency(reason);
        }
    }

    pub(in crate::client) fn attach_mutation_workflow(
        &mut self,
        workflow: ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        self.validate_mutation_connection(&workflow)?;
        self.outcomes = workflow.outcomes;
        self.recovery_required = workflow.recovery_required;
        self.notifications = workflow.notifications;
        self.deferred = workflow.deferred;
        self.mutation_connected = true;
        Ok(())
    }

    pub(in crate::client) fn validate_mutation_connection(
        &self,
        _workflow: &ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        if self.lifecycle.is_none() {
            anyhow::bail!("application workflow has no lifecycle adapter")
        }
        if self.mutation_connected {
            anyhow::bail!("application workflow is already connected")
        }
        Ok(())
    }

    pub(in crate::client) async fn execute_lifecycle(
        &mut self,
        command: CoreCommand,
    ) -> anyhow::Result<()> {
        match command {
            CoreCommand::StartupReconcile {
                operation_id,
                transition,
            } => {
                if let Some(report) = &self.startup_report {
                    return startup_report_result(report);
                }
                let lifecycle = self
                    .lifecycle
                    .as_ref()
                    .cloned()
                    .expect("actor-owned application workflow has a lifecycle adapter");
                let report = lifecycle
                    .startup_reconcile(operation_id, transition, self.startup.retry_target_mut())
                    .await;
                self.startup_report = Some(report.clone());
                startup_report_result(&report)
            }
            CoreCommand::RecoverStartup {
                operation_id,
                transition,
            } => {
                let lifecycle = self
                    .lifecycle
                    .as_ref()
                    .cloned()
                    .expect("actor-owned application workflow has a lifecycle adapter");
                let report = lifecycle
                    .recover_startup(operation_id, transition, self.startup.retry_target_mut())
                    .await;
                if self.startup_report.is_none() {
                    self.startup_report = Some(report.clone());
                }
                startup_report_result(&report)
            }
            CoreCommand::RetryStartup {
                operation_id,
                identity,
                transition,
            } => {
                let lifecycle = self
                    .lifecycle
                    .as_ref()
                    .cloned()
                    .expect("actor-owned application workflow has a lifecycle adapter");
                let report = lifecycle
                    .retry_startup(
                        operation_id,
                        identity,
                        transition,
                        self.startup.retry_target_mut(),
                    )
                    .await;
                startup_report_result(&report)
            }
            CoreCommand::CacheStartupUnsettled {
                operation_id,
                reason,
            } => {
                if self.startup_report.is_none() {
                    self.startup.mark_owner_unproven();
                    self.startup_report = Some(StartupReport {
                        operation_id,
                        observation: None,
                        outcome: StartupOutcome::Unsettled {
                            reason: reason.clone(),
                        },
                    });
                    if let Some(notifications) = &self.notifications {
                        let ports = self.lifecycle().effect_port_binding().await;
                        notifications.publish_full(ports);
                    }
                }
                anyhow::bail!(reason)
            }
            command => {
                let lifecycle = self
                    .lifecycle
                    .as_ref()
                    .cloned()
                    .expect("actor-owned application workflow has a lifecycle adapter");
                lifecycle
                    .execute(command, self.startup.retry_target_mut())
                    .await
            }
        }
    }

    pub(in crate::client) async fn run_profile_mutation(
        &mut self,
        request: &mut super::mutation::MutationRequest,
    ) -> (bool, Result<(), String>) {
        let Some(lifecycle) = self.lifecycle.as_ref().cloned() else {
            return (
                false,
                Err("application workflow has no lifecycle adapter".into()),
            );
        };
        self.run_mutation(request, lifecycle.as_ref()).await
    }

    pub(in crate::client) fn deferred_retry(&self) -> Option<(String, tokio::time::Instant)> {
        self.deferred.as_ref().and_then(|deferred| {
            deferred
                .next_attempt
                .map(|next_attempt| (deferred.digest.clone(), next_attempt))
        })
    }

    pub(in crate::client) fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
    ) {
        if let Some(notifications) = &self.notifications {
            notifications.runtime_bound(ports, false);
        }
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

fn startup_report_result(report: &StartupReport) -> anyhow::Result<()> {
    match report.outcome.failure_reason() {
        Some(reason) => Err(anyhow::anyhow!(reason.to_owned())),
        None => Ok(()),
    }
}

//! The tracked application workflow state for startup and Profile mutations.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core::state::DecisionHandle;
use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::core_lifecycle::{
    Command as CoreCommand, CoreLifecycleStatus, CoreLifecycleWorkflow, ports::CoreStatusSnapshot,
};

#[cfg(test)]
use super::mutation::{MutationDeferredContext, RetryableCause};
use super::{
    attempt::{AttemptOrigin, AttemptStage, LiveAttempt},
    mutation::{
        AppliedCandidate, CheckRecord, ConfigDomain, DeferredTarget, KnownRuntimeState,
        MutationReceipt, MutationStage, TargetOrigin,
    },
    startup::{StartupOutcome, StartupReport, StartupWorkflowState},
};
use crate::client::reconciler::inputs::RuntimeInputs;
use crate::client::runtime::RuntimeApplyReceipt;

use chimera_core::client::reconciler::application_workflow::ApplicationWorkflow as SharedApplicationWorkflow;
pub use chimera_core::client::reconciler::application_workflow::{ProfileRuntime, RecoveryContext};

/// Desktop composition and legacy test fixtures. All actual progress and
/// production lifecycle execution are owned by the shared workflow.
pub(in crate::client) struct ApplicationWorkflow {
    shared: SharedApplicationWorkflow<CoreLifecycleWorkflow>,
    #[cfg(test)]
    mutation_connected: bool,
}
impl std::ops::Deref for ApplicationWorkflow {
    type Target = SharedApplicationWorkflow<CoreLifecycleWorkflow>;
    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}
impl std::ops::DerefMut for ApplicationWorkflow {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.shared
    }
}
impl ApplicationWorkflow {
    #[cfg(test)]
    pub(super) fn new(
        outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    ) -> Self {
        Self {
            shared: SharedApplicationWorkflow::new(outcomes, recovery_required, notifications),
            mutation_connected: false,
        }
    }
    pub(in crate::client) fn with_lifecycle(lifecycle: CoreLifecycleWorkflow) -> Self {
        Self {
            shared: SharedApplicationWorkflow::with_lifecycle(lifecycle),
            #[cfg(test)]
            mutation_connected: false,
        }
    }

    // Existing test fixture recovery must use the original TCC participant.
    #[cfg(test)]
    pub(in crate::client) async fn execute_lifecycle(
        &mut self,
        command: CoreCommand,
    ) -> anyhow::Result<bool> {
        use chimera_core::client::reconciler::application_command::ApplicationCommandOutcome;
        let lifecycle = self.lifecycle_handle();
        match self.shared.execute_command(command).await? {
            ApplicationCommandOutcome::Settled {
                service_reconnected,
            } => Ok(service_reconnected),
            ApplicationCommandOutcome::RecoverPrevious => {
                self.recover_after_reconcile(lifecycle.as_ref()).await?;
                Ok(false)
            }
        }
    }

    // UFCS is needed at the shared mailbox host boundary.
    pub(in crate::client) fn lifecycle(&self) -> &CoreLifecycleWorkflow {
        self.shared.lifecycle()
    }
    pub(in crate::client) fn lifecycle_handle(&self) -> Arc<CoreLifecycleWorkflow> {
        self.shared.lifecycle_handle()
    }
    pub(in crate::client) fn progress(
        &self,
    ) -> &chimera_core::client::reconciler::workflow_progress::WorkflowProgress {
        self.shared.progress()
    }
    pub(in crate::client) fn progress_mut(
        &mut self,
    ) -> &mut chimera_core::client::reconciler::workflow_progress::WorkflowProgress {
        self.shared.progress_mut()
    }
    pub(in crate::client) fn recovery_command_allowed(&self, command: &CoreCommand) -> bool {
        self.shared.recovery_command_allowed(command)
    }
    pub(in crate::client) fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
    ) {
        self.shared.runtime_bound(ports)
    }
    pub(in crate::client) async fn service_endpoint_recovery_due(&self) -> bool {
        self.shared.service_endpoint_recovery_due().await
    }
    #[cfg(test)]
    pub(in crate::client) fn startup_retry_schedule(
        &self,
    ) -> Option<(String, tokio::time::Instant)> {
        self.shared.startup_retry_schedule()
    }
    #[cfg(test)]
    pub(crate) fn seed_unresolved_lifecycle_attempt_for_tests(
        &mut self,
    ) -> chimera_core_manager::OperationId {
        let operation_id = chimera_core_manager::OperationId::generate();
        let mut attempt = LiveAttempt::lifecycle(operation_id.clone());
        attempt.unresolved = Some("test runtime outcome is unresolved".into());
        self.progress.live = Some(attempt);
        operation_id
    }

    #[cfg(test)]
    pub(crate) fn seed_unresolved_source_decision_for_tests(
        &mut self,
        decision: DecisionHandle,
    ) -> chimera_core_manager::OperationId {
        let operation_id = chimera_core_manager::OperationId::generate();
        let mut attempt = LiveAttempt::source_decision(operation_id.clone(), decision);
        attempt.stage = AttemptStage::AwaitDecision;
        attempt.unresolved = Some("test Profile source decision is unsettled".into());
        self.progress.live = Some(attempt);
        operation_id
    }

    #[cfg(test)]
    pub(crate) fn seed_unresolved_attempt_with_deferred_retry_for_tests(
        &mut self,
        decision: DecisionHandle,
    ) -> (chimera_core_manager::OperationId, String) {
        let operation_id = chimera_core_manager::OperationId::generate();
        let target_id = chimera_core_manager::OperationId::generate();
        let digest = "test-deferred-profile-target".to_owned();
        self.progress.deferred = Some(DeferredTarget {
            operation_id: target_id,
            origin: TargetOrigin::Mutation,
            identity: digest.clone(),
            cause: RetryableCause {
                stage: MutationStage::TryingCritical,
                message: "test deferred Profile runtime apply".into(),
            },
            attempts_remaining: 3,
            attempts: 1,
            waits: 0,
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            next_attempt: Some(tokio::time::Instant::now() - std::time::Duration::from_secs(1)),
            mutation: Some(MutationDeferredContext {
                digest: digest.clone(),
                baseline: KnownRuntimeState::NeverApplied,
                domain: ConfigDomain::Profiles,
                decision: decision.clone(),
            }),
        });
        let mut attempt = LiveAttempt::source_decision(operation_id.clone(), decision);
        attempt.unresolved = Some("test Profile source decision is unsettled".into());
        self.progress.live = Some(attempt);
        assert!(self.progress.begin_deferred_retry(&digest));
        (operation_id, digest)
    }

    #[cfg(test)]
    pub(crate) fn seed_unresolved_committed_target_for_tests(
        &mut self,
        decision: DecisionHandle,
    ) -> chimera_core_manager::OperationId {
        let operation_id = chimera_core_manager::OperationId::generate();
        let digest = "test-committed-profile-target".to_owned();
        let baseline = KnownRuntimeState::Stopped;
        let target = DeferredTarget {
            operation_id: operation_id.clone(),
            origin: TargetOrigin::Mutation,
            identity: digest.clone(),
            cause: RetryableCause {
                stage: MutationStage::TryingCritical,
                message: "test committed Profile runtime target".into(),
            },
            attempts_remaining: 2,
            attempts: 1,
            waits: 0,
            health: crate::client::convergence::ConvergenceHealth::RecoveryRequired,
            next_attempt: None,
            mutation: Some(MutationDeferredContext {
                digest,
                baseline: baseline.clone(),
                domain: ConfigDomain::Profiles,
                decision,
            }),
        };
        let mut attempt = LiveAttempt::committed_target(operation_id.clone(), target);
        attempt.stage = AttemptStage::TryingCritical;
        attempt.baseline = Some(baseline);
        attempt.unresolved = Some("test committed Profile retry outcome is unresolved".into());
        self.progress.live = Some(attempt);
        operation_id
    }

    #[cfg(test)]
    pub(crate) fn recovery_state_for_tests(
        &self,
    ) -> (
        Option<(chimera_core_manager::OperationId, bool)>,
        Option<(String, tokio::time::Instant)>,
        Option<(u8, u32)>,
    ) {
        let attempt = self
            .progress
            .live
            .as_ref()
            .map(|attempt| (attempt.operation_id.clone(), attempt.unresolved.is_some()));
        let retry_budget = self
            .progress
            .deferred
            .as_ref()
            .filter(|target| target.is_mutation())
            .or_else(|| {
                self.progress
                    .live
                    .as_ref()
                    .and_then(|attempt| match &attempt.origin {
                        AttemptOrigin::CommittedTarget(target) if target.is_mutation() => {
                            Some(target.as_ref())
                        }
                        AttemptOrigin::SourceDecision { .. }
                        | AttemptOrigin::CommittedTarget(_)
                        | AttemptOrigin::Lifecycle => None,
                    })
            })
            .map(|target| (target.attempts_remaining, target.attempts));
        (attempt, self.progress.deferred_retry(), retry_budget)
    }

    #[cfg(test)]
    pub(in crate::client) fn mutation_connected(&self) -> bool {
        self.mutation_connected
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
        self.progress.deferred = Some(target.into());
    }

    #[cfg(test)]
    pub(crate) fn startup_retry_target_for_tests(
        &self,
    ) -> Option<super::startup::StartupRetryTarget> {
        self.progress
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
            .cloned()
            .map(TryInto::try_into)
            .and_then(Result::ok)
    }

    #[cfg(test)]
    pub(in crate::client) fn attach_mutation_workflow(
        &mut self,
        workflow: ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        self.validate_mutation_connection(&workflow)?;
        let workflow = workflow.shared;
        self.outcomes = workflow.outcomes;
        self.progress.recovery_required = workflow.progress.recovery_required;
        self.notifications = workflow.notifications;
        if workflow.progress.deferred.is_some() {
            self.progress.deferred = workflow.progress.deferred;
        }
        self.mutation_connected = true;
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::client) fn validate_mutation_connection(
        &self,
        workflow: &ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        if !self.shared.has_lifecycle() {
            anyhow::bail!("application workflow has no lifecycle adapter")
        }
        if self.mutation_connected {
            anyhow::bail!("application workflow is already connected")
        }
        if self.progress.deferred.is_some() && workflow.progress.deferred.is_some() {
            anyhow::bail!("startup and Profile workflows both have an outstanding deferred target")
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::client) async fn run_profile_mutation(
        &mut self,
        request: &mut super::mutation::MutationRequest,
    ) -> (bool, Result<(), String>) {
        let Some(lifecycle) = self.shared.optional_lifecycle_handle() else {
            return (
                false,
                Err("application workflow has no lifecycle adapter".into()),
            );
        };
        self.run_mutation(request, lifecycle.as_ref()).await
    }

    #[cfg(test)]
    pub(in crate::client) async fn mark_recovery_with_attempt(
        &mut self,
        operation_id: OperationId,
        baseline: Option<KnownRuntimeState>,
        decision: DecisionHandle,
        target: Option<Arc<RuntimeApplyReceipt>>,
        stage: MutationStage,
        message: String,
    ) -> RecoveryContext {
        if self
            .progress
            .live
            .as_ref()
            .is_none_or(|attempt| attempt.operation_id != operation_id)
        {
            self.progress.live = Some(LiveAttempt::source_decision(operation_id, decision));
        }
        if let Some(attempt) = &mut self.progress.live {
            attempt.baseline = baseline;
            attempt.target = target;
            attempt.stage = stage.into();
        }
        self.conclude_attempt(Some(message));
        self.progress
            .publish_recovery_context()
            .await
            .expect("an unresolved live attempt has a recovery projection")
    }
}

#[async_trait::async_trait]
impl chimera_core::client::reconciler::application_workflow::ApplicationLifecycleHost
    for CoreLifecycleWorkflow
{
    fn startup_owner_unproven_handle(&self) -> Arc<std::sync::atomic::AtomicBool> {
        CoreLifecycleWorkflow::startup_owner_unproven_handle(self)
    }
    fn outcome_uncertain(&self) -> bool {
        CoreLifecycleWorkflow::outcome_uncertain(self)
    }
    async fn service_endpoint_recovery_due(&self) -> bool {
        CoreLifecycleWorkflow::service_endpoint_recovery_due(self).await
    }
}

#[async_trait::async_trait]
impl chimera_core::client::reconciler::application_command::ApplicationCommandHost
    for CoreLifecycleWorkflow
{
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    type SystemProxy = crate::client::system_proxy::SystemProxyClient;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    type SystemProxy = ();

    async fn execute_lower(&self, command: CoreCommand) -> anyhow::Result<()> {
        #[cfg(test)]
        {
            self.execute(command, &mut None).await
        }
        #[cfg(not(test))]
        {
            self.execute(command).await
        }
    }
    async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        ProfileRuntime::effect_port_binding(self).await
    }
    fn take_service_recovery_reconnected(&self) -> bool {
        self.take_service_recovery_reconnected()
    }
}

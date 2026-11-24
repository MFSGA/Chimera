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

pub use chimera_core::client::reconciler::workflow_progress::RecoveryContext;

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
    async fn try_release_service_owner(&self) -> Result<(), String> {
        Ok(())
    }
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
    pub(super) notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    pub(super) progress: chimera_core::client::reconciler::workflow_progress::WorkflowProgress,
    /// Startup and Profile mutations share the application workflow actor;
    /// CoreLifecycleWorkflow adapts its existing runtime and Service ports.
    lifecycle: Option<Arc<CoreLifecycleWorkflow>>,
    #[cfg(test)]
    mutation_connected: bool,
}

impl ApplicationWorkflow {
    pub(in crate::client) fn has_unresolved_attempt(&self) -> bool {
        self.progress.has_unresolved_attempt()
    }
    pub(in crate::client) fn startup_owner_unproven(&self) -> bool {
        self.progress.startup_owner_unproven()
    }
    pub(in crate::client) fn clear_startup_retry_target(&mut self) {
        self.progress.clear_startup_retry_target()
    }
    pub(in crate::client) fn startup_report(&self) -> Option<StartupReport> {
        self.progress.startup_report()
    }
    pub(in crate::client) fn startup_retry_schedule(
        &self,
    ) -> Option<(String, tokio::time::Instant)> {
        self.progress.startup_retry_schedule()
    }
    pub(in crate::client) fn startup_retry_due(&self, identity: &str) -> bool {
        self.progress.startup_retry_due(identity)
    }
    pub(in crate::client) fn begin_startup_retry(&mut self, identity: &str) -> bool {
        self.progress.begin_startup_retry(identity)
    }
    pub(in crate::client) fn defer_startup_retry(&mut self, identity: &str, reason: String) {
        self.progress.defer_startup_retry(identity, reason)
    }
    pub(in crate::client) fn deferred_retry(&self) -> Option<(String, tokio::time::Instant)> {
        self.progress.deferred_retry()
    }
    pub(in crate::client) fn begin_deferred_retry(&mut self, digest: &str) -> bool {
        self.progress.begin_deferred_retry(digest)
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
        self.progress
            .finish_deferred_retry(
                digest,
                succeeded,
                retryable,
                waiting_dependency,
                uncertain,
                error,
            )
            .await
    }
    pub(in crate::client) fn reschedule_deferred_retry(
        &mut self,
        digest: &str,
        delay: std::time::Duration,
    ) -> Option<tokio::time::Instant> {
        self.progress.reschedule_deferred_retry(digest, delay)
    }
    pub(in crate::client) fn wait_for_deferred_dependency(&mut self, digest: &str) {
        self.progress.wait_for_deferred_dependency(digest)
    }
    pub(in crate::client) fn runtime_reconciled(&mut self) {
        self.progress.runtime_reconciled()
    }
    pub(in crate::client) async fn publish_recovery_context(&self) -> Option<RecoveryContext> {
        self.progress.publish_recovery_context().await
    }
    /// Ref's positive clearance rule consults the lower Core owner before
    /// releasing an attempt. A pure Workflow test has no lifecycle adapter.
    pub(super) fn lower_outcome_uncertain(&self) -> bool {
        self.lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.outcome_uncertain())
    }

    pub(in crate::client) async fn service_endpoint_recovery_due(&self) -> bool {
        if self.progress.has_unresolved_attempt() {
            return false;
        }
        let Some(lifecycle) = &self.lifecycle else {
            return false;
        };
        lifecycle.service_endpoint_recovery_due().await
    }

    pub(in crate::client) fn service_recovery_blocks_handoff(&self) -> bool {
        self.lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.service_recovery_blocks_handoff())
    }

    pub(in crate::client) fn take_service_recovery_reconnected(&self) -> bool {
        self.lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.take_service_recovery_reconnected())
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

    pub(in crate::client) fn recovery_command_allowed(&self, command: &CoreCommand) -> bool {
        use chimera_core::client::reconciler::workflow_progress::RecoveryCommand;
        let command = match command {
            CoreCommand::CacheStartupUnsettled { .. } => RecoveryCommand::SourceOnly,
            CoreCommand::PatchApplicationConfig {
                patch,
                service_connected,
                ..
            } if !super::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            ) =>
            {
                RecoveryCommand::SourceOnly
            }
            CoreCommand::RecoverCore | CoreCommand::Reconcile | CoreCommand::ReconcileSource => {
                RecoveryCommand::RecoverRuntime
            }
            CoreCommand::RecoverStartup { .. } => RecoveryCommand::RecoverStartup,
            _ => RecoveryCommand::Other,
        };
        self.progress.recovery_command_allowed(command)
    }

    pub(super) fn new(
        outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    ) -> Self {
        Self {
            outcomes,
            notifications,
            progress: chimera_core::client::reconciler::workflow_progress::WorkflowProgress::new(
                recovery_required,
            ),
            lifecycle: None,
            #[cfg(test)]
            mutation_connected: false,
        }
    }

    pub(in crate::client) fn with_lifecycle(lifecycle: CoreLifecycleWorkflow) -> Self {
        let startup =
            StartupWorkflowState::with_owner_signal(lifecycle.startup_owner_unproven_handle());
        let mut workflow = Self::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(None)),
            None,
        );
        workflow.lifecycle = Some(Arc::new(lifecycle));
        workflow.progress.startup = startup;
        workflow
    }

    pub(in crate::client) fn lifecycle(&self) -> &CoreLifecycleWorkflow {
        self.lifecycle
            .as_deref()
            .expect("actor-owned application workflow has a lifecycle adapter")
    }

    pub(in crate::client) fn lifecycle_handle(&self) -> Arc<CoreLifecycleWorkflow> {
        self.lifecycle
            .as_ref()
            .cloned()
            .expect("actor-owned application workflow has a lifecycle adapter")
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
        if self.lifecycle.is_none() {
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

    #[cfg(not(test))]
    async fn recover_after_reconcile(&mut self, core: &dyn ProfileRuntime) -> anyhow::Result<()> {
        if let Some(operation_id) =
            chimera_core::client::reconciler::reestablish::reconciled_native_attempt(
                self.progress.live.as_ref(),
                core.status().uncertain,
            )?
        {
            core.discard_profile_runtime(&operation_id).await;
            self.progress.live = None;
            self.progress.deferred = None;
            self.progress.publish_recovery_context().await;
        }
        Ok(())
    }

    pub(in crate::client) async fn execute_lifecycle(
        &mut self,
        command: CoreCommand,
    ) -> anyhow::Result<bool> {
        match command {
            CoreCommand::StartupReconcile {
                operation_id,
                transition,
            } => {
                let report = self.startup_reconcile(operation_id, transition).await;
                startup_report_result(&report)?;
                Ok(false)
            }
            CoreCommand::RecoverStartup {
                operation_id,
                transition,
            } => {
                let report = self.recover_startup(operation_id, transition).await;
                startup_report_result(&report)?;
                Ok(false)
            }
            CoreCommand::RetryStartup {
                operation_id,
                identity,
                transition,
            } => {
                let report = self.retry_startup(operation_id, identity, transition).await;
                startup_report_result(&report)?;
                Ok(false)
            }
            CoreCommand::FollowService {
                operation_id,
                transition,
            } => {
                self.follow_service(operation_id, transition).await?;
                Ok(false)
            }
            CoreCommand::RecoverCore => {
                let lifecycle = self.lifecycle_handle();
                #[cfg(test)]
                {
                    let mut no_startup_retry_target = None;
                    lifecycle
                        .execute(CoreCommand::RecoverCore, &mut no_startup_retry_target)
                        .await?;
                }
                #[cfg(not(test))]
                lifecycle.execute(CoreCommand::RecoverCore).await?;
                self.recover_after_reconcile(lifecycle.as_ref()).await?;
                Ok(false)
            }
            command @ (CoreCommand::Reconcile | CoreCommand::ReconcileSource)
                if self.progress.has_unresolved_attempt() =>
            {
                // Preserve the original ref trigger when recovering a legacy
                // committed target: explicit Retry is forced, whereas a
                // SourceCommitted notice must not replay a known refusal.
                let lifecycle = self.lifecycle_handle();
                #[cfg(test)]
                {
                    let mut no_startup_retry_target = None;
                    lifecycle
                        .execute(command, &mut no_startup_retry_target)
                        .await?;
                }
                #[cfg(not(test))]
                lifecycle.execute(command).await?;
                self.recover_after_reconcile(lifecycle.as_ref()).await?;
                Ok(false)
            }
            CoreCommand::CacheStartupUnsettled {
                operation_id,
                reason,
            } => {
                if self.progress.startup_report.is_none() {
                    self.progress.startup.mark_owner_unproven();
                    self.progress.startup_report = Some(StartupReport {
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
                let lifecycle = self.lifecycle_handle();
                let source_only_patch = matches!(
                    &command,
                    CoreCommand::PatchApplicationConfig {
                        patch,
                        service_connected,
                        ..
                    } if !chimera_core::state::mutation::impact::
                        application_patch_requires_reconcile(patch, *service_connected)
                );
                let preserve_unresolved_attempt = (source_only_patch
                    || matches!(&command, CoreCommand::Shutdown))
                    && self.progress.has_unresolved_attempt();
                if !preserve_unresolved_attempt {
                    debug_assert!(
                        self.progress.live.is_none(),
                        "an unresolved attempt is never replaced"
                    );
                    self.progress.live = Some(LiveAttempt::lifecycle(OperationId::generate()));
                    self.progress.advance_attempt(AttemptStage::TryingCritical);
                }
                let refresh_startup_retry_identity =
                    matches!(&command, CoreCommand::PatchApplicationConfig { .. });
                let service_endpoint_recovery =
                    matches!(&command, CoreCommand::ServiceEndpointDown(_));
                let refresh_startup_target = match &command {
                    CoreCommand::Reconcile
                    | CoreCommand::ReconcileSource
                    | CoreCommand::RecoverCore
                    | CoreCommand::RetryDeferredProfiles { .. }
                    | CoreCommand::ReconcileProfiles { .. } => true,
                    #[cfg(test)]
                    CoreCommand::SelectCore(_) => true,
                    CoreCommand::PatchApplicationConfig {
                        patch,
                        service_connected,
                        ..
                    } => {
                        chimera_core::state::mutation::impact::application_patch_requires_reconcile(
                            patch,
                            *service_connected,
                        )
                    }
                    _ => false,
                };
                #[cfg(test)]
                let result = {
                    let mut no_startup_retry_target = None;
                    lifecycle
                        .execute(command, &mut no_startup_retry_target)
                        .await
                };
                #[cfg(not(test))]
                let result = lifecycle.execute(command).await;
                if refresh_startup_target {
                    self.refresh_startup_retry_target_after_reconcile(&lifecycle, &result)
                        .await;
                } else if refresh_startup_retry_identity && result.is_ok() {
                    self.reschedule_startup_retry_after_committed_config_patch(&lifecycle)
                        .await;
                }
                if !preserve_unresolved_attempt {
                    if lifecycle.outcome_uncertain() {
                        let reason = result
                            .as_ref()
                            .err()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "core lifecycle outcome remains uncertain".into());
                        self.conclude_attempt(Some(reason));
                    } else {
                        self.conclude_attempt(None);
                    }
                }
                let recovered = service_endpoint_recovery
                    && result.is_ok()
                    && lifecycle.take_service_recovery_reconnected();
                result.map(|()| recovered)
            }
        }
    }

    #[cfg(test)]
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

    pub(in crate::client) fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
    ) {
        if let Some(notifications) = &self.notifications {
            notifications.runtime_bound(ports, false);
        }
    }

    pub(in crate::client) async fn mark_recovery_required(
        &mut self,
        message: String,
    ) -> RecoveryContext {
        if self.progress.live.is_some() {
            self.conclude_attempt(Some(message.clone()));
            if let Some(context) = self.progress.publish_recovery_context().await {
                return context;
            }
            let attempt = self
                .progress
                .live
                .as_ref()
                .expect("the unresolved attempt remains live");
            return RecoveryContext {
                operation_id: Some(attempt.operation_id),
                runtime_operation: None,
                domain: ConfigDomain::Profiles,
                stage: MutationStage::TryingCritical,
                decision: None,
                baseline: None,
                target: None,
                error: message,
            };
        }
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
        *self.progress.recovery_required.lock().await = Some(context.clone());
        context
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

pub(super) fn startup_report_result(report: &StartupReport) -> anyhow::Result<()> {
    match report.outcome.failure_reason() {
        Some(reason) => Err(anyhow::anyhow!(reason.to_owned())),
        None => Ok(()),
    }
}

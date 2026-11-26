//! The existing application transaction owner, shared by the reconciler.
pub use super::workflow_progress::RecoveryContext;
use super::{
    application_command::ApplicationCommandHost,
    inputs::RuntimeInputs,
    mailbox::CoreLifecycleStatus,
    startup_policy::{StartupReport, StartupWorkflowState},
    workflow_mutation::{AppliedCandidate, CheckRecord, KnownRuntimeState, MutationReceipt},
};
use crate::client::{
    core_lifecycle::{command::Command, status::CoreStatusSnapshot},
    effects::ports::CommitNotifications,
};
use chimera_config::profile::Profiles;
use chimera_core_manager::OperationId;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};
use tokio::sync::Mutex;

/// Existing lower owner supplies lifecycle commands and Profile recovery.
#[async_trait::async_trait]
pub trait ApplicationLifecycleHost: ApplicationCommandHost + ProfileRuntime {
    fn startup_owner_unproven_handle(&self) -> Arc<AtomicBool>;
    fn outcome_uncertain(&self) -> bool;
    async fn service_endpoint_recovery_due(&self) -> bool;
}

#[async_trait::async_trait]
pub trait ProfileRuntime: Send + Sync + 'static {
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

pub struct ApplicationWorkflow<L> {
    pub outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
    pub notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    pub progress: super::workflow_progress::WorkflowProgress,
    /// Startup and Profile mutations share the application workflow actor;
    /// CoreLifecycleWorkflow adapts its existing runtime and Service ports.
    lifecycle: Option<Arc<L>>,
}

impl<L: ApplicationLifecycleHost> ApplicationWorkflow<L> {
    pub fn progress(&self) -> &super::workflow_progress::WorkflowProgress {
        &self.progress
    }

    pub fn progress_mut(&mut self) -> &mut super::workflow_progress::WorkflowProgress {
        &mut self.progress
    }

    pub fn has_unresolved_attempt(&self) -> bool {
        self.progress.has_unresolved_attempt()
    }

    pub fn startup_owner_unproven(&self) -> bool {
        self.progress.startup_owner_unproven()
    }

    pub fn clear_startup_retry_target(&mut self) {
        self.progress.clear_startup_retry_target()
    }

    pub fn startup_report(&self) -> Option<StartupReport> {
        self.progress.startup_report()
    }

    pub fn startup_retry_schedule(&self) -> Option<(String, tokio::time::Instant)> {
        self.progress.startup_retry_schedule()
    }

    pub fn startup_retry_due(&self, identity: &str) -> bool {
        self.progress.startup_retry_due(identity)
    }

    pub fn begin_startup_retry(&mut self, identity: &str) -> bool {
        self.progress.begin_startup_retry(identity)
    }

    pub fn defer_startup_retry(&mut self, identity: &str, reason: String) {
        self.progress.defer_startup_retry(identity, reason)
    }

    pub fn deferred_retry(&self) -> Option<(String, tokio::time::Instant)> {
        self.progress.deferred_retry()
    }

    pub fn begin_deferred_retry(&mut self, digest: &str) -> bool {
        self.progress.begin_deferred_retry(digest)
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

    pub fn reschedule_deferred_retry(
        &mut self,
        digest: &str,
        delay: std::time::Duration,
    ) -> Option<tokio::time::Instant> {
        self.progress.reschedule_deferred_retry(digest, delay)
    }

    pub fn wait_for_deferred_dependency(&mut self, digest: &str) {
        self.progress.wait_for_deferred_dependency(digest)
    }

    pub fn runtime_reconciled(&mut self) {
        self.progress.runtime_reconciled()
    }

    pub async fn publish_recovery_context(&self) -> Option<RecoveryContext> {
        self.progress.publish_recovery_context().await
    }

    pub fn lower_outcome_uncertain(&self) -> bool {
        self.lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.outcome_uncertain())
    }

    pub async fn service_endpoint_recovery_due(&self) -> bool {
        if self.progress.has_unresolved_attempt() {
            return false;
        }
        let Some(lifecycle) = &self.lifecycle else {
            return false;
        };
        lifecycle.service_endpoint_recovery_due().await
    }

    pub fn service_recovery_blocks_handoff(&self) -> bool {
        self.lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.service_recovery_blocks_handoff())
    }

    pub fn take_service_recovery_reconnected(&self) -> bool {
        self.lifecycle.as_ref().is_some_and(|lifecycle| {
            ApplicationCommandHost::take_service_recovery_reconnected(lifecycle.as_ref())
        })
    }

    pub fn recovery_command_allowed(&self, command: &Command<L::SystemProxy>) -> bool {
        use super::workflow_progress::RecoveryCommand;
        let command = match command {
            Command::CacheStartupUnsettled { .. } => RecoveryCommand::SourceOnly,
            Command::PatchApplicationConfig {
                patch,
                service_connected,
                ..
            } if !crate::state::mutation::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            ) =>
            {
                RecoveryCommand::SourceOnly
            }
            Command::RecoverCore | Command::Reconcile | Command::ReconcileSource => {
                RecoveryCommand::RecoverRuntime
            }
            Command::RecoverStartup { .. } => RecoveryCommand::RecoverStartup,
            _ => RecoveryCommand::Other,
        };
        self.progress.recovery_command_allowed(command)
    }

    pub fn new(
        outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
        notifications: Option<Arc<dyn CommitNotifications>>,
    ) -> Self {
        Self {
            outcomes,
            notifications,
            progress: super::workflow_progress::WorkflowProgress::new(recovery_required),
            lifecycle: None,
        }
    }

    pub fn with_lifecycle(lifecycle: L) -> Self {
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

    pub fn has_lifecycle(&self) -> bool {
        self.lifecycle.is_some()
    }
    pub fn optional_lifecycle_handle(&self) -> Option<Arc<L>> {
        self.lifecycle.clone()
    }

    pub fn lifecycle(&self) -> &L {
        self.lifecycle
            .as_deref()
            .expect("actor-owned application workflow has a lifecycle adapter")
    }

    pub fn lifecycle_handle(&self) -> Arc<L> {
        self.lifecycle
            .as_ref()
            .cloned()
            .expect("actor-owned application workflow has a lifecycle adapter")
    }

    pub async fn recover_after_reconcile(
        &mut self,
        core: &dyn ProfileRuntime,
    ) -> anyhow::Result<()> {
        if let Some(operation_id) = super::reestablish::reconciled_native_attempt(
            self.progress.live.as_ref(),
            core.status().uncertain,
        )? {
            core.discard_profile_runtime(&operation_id).await;
            self.progress.live = None;
            self.progress.deferred = None;
            self.progress.publish_recovery_context().await;
        }
        Ok(())
    }

    pub async fn execute_lifecycle(
        &mut self,
        command: Command<L::SystemProxy>,
    ) -> anyhow::Result<bool> {
        let lifecycle = self.lifecycle_handle();
        match self.execute_command(command).await? {
            super::application_command::ApplicationCommandOutcome::Settled {
                service_reconnected,
            } => Ok(service_reconnected),
            super::application_command::ApplicationCommandOutcome::RecoverPrevious => {
                self.recover_after_reconcile(lifecycle.as_ref()).await?;
                Ok(false)
            }
        }
    }

    pub async fn execute_command(
        &mut self,
        command: Command<L::SystemProxy>,
    ) -> anyhow::Result<super::application_command::ApplicationCommandOutcome> {
        let lifecycle = self.lifecycle_handle();
        super::application_command::execute_application_command(
            &mut self.progress,
            lifecycle.as_ref(),
            self.notifications.as_deref(),
            command,
        )
        .await
    }

    pub fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
    ) {
        if let Some(notifications) = &self.notifications {
            notifications.runtime_bound(ports, false);
        }
    }

    pub async fn mark_recovery_required(&mut self, message: String) -> RecoveryContext {
        self.progress.mark_recovery_required(message).await
    }

    pub fn conclude_attempt(&mut self, unresolved: Option<String>) -> bool {
        let lower_outcome_uncertain = self.lower_outcome_uncertain();
        super::reestablish::conclude_attempt(
            &mut self.progress.live,
            &mut self.progress.deferred,
            unresolved,
            lower_outcome_uncertain,
        )
    }
}

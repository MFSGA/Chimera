//! The tracked application workflow state for Profile mutations.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core::state::DecisionHandle;
use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::core_lifecycle::{CoreLifecycleStatus, ports::CoreStatusSnapshot};

use super::mutation::{
    AppliedCandidate, CheckRecord, ConfigDomain, KnownRuntimeState, MutationReceipt, MutationStage,
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
    async fn observe_runtime_baseline(&self) -> anyhow::Result<KnownRuntimeState>;
    async fn restore_runtime_baseline(&self, baseline: &KnownRuntimeState) -> anyhow::Result<()>;
    async fn validate_profile_runtime(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<CheckRecord>;
    async fn prepare_profile_runtime(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
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
}

impl ApplicationWorkflow {
    pub(super) fn new(
        outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
    ) -> Self {
        Self {
            outcomes,
            recovery_required,
        }
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

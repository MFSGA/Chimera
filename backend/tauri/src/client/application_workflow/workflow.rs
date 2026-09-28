//! The tracked application workflow state for Profile mutations.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::{
    core_lifecycle::{CoreLifecycleStatus, ports::CoreStatusSnapshot},
    runtime::RuntimeCommitStatus,
};

#[async_trait::async_trait]
pub(crate) trait ProfileRuntime: Send + Sync + 'static {
    fn status(&self) -> CoreLifecycleStatus;
    async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn reconcile_profiles(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()>;
    fn request_runtime_rebuild(&self);
}

pub(in crate::client) struct ApplicationWorkflow {
    pub(super) outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
    pub(super) recovery_required: Arc<Mutex<Option<String>>>,
}

impl ApplicationWorkflow {
    pub(super) fn new(
        outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
        recovery_required: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            outcomes,
            recovery_required,
        }
    }

    pub(in crate::client) async fn mark_recovery_required(&self, message: String) {
        *self.recovery_required.lock().await = Some(message);
    }
}

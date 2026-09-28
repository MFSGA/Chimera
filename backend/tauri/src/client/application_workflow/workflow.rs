//! The tracked application workflow state for Profile mutations.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::{
    core_lifecycle::{CoreLifecycleClient, CoreLifecycleStatus, ports::CoreStatusSnapshot},
    runtime::RuntimeCommitStatus,
};

#[async_trait::async_trait]
pub(super) trait ProfileRuntime: Send + Sync + 'static {
    fn status(&self) -> CoreLifecycleStatus;
    async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn reconcile_profiles(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()>;
    fn request_runtime_rebuild(&self);
}

#[async_trait::async_trait]
impl ProfileRuntime for CoreLifecycleClient {
    fn status(&self) -> CoreLifecycleStatus {
        CoreLifecycleClient::status(self)
    }

    async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        CoreLifecycleClient::core_status(self).await
    }

    async fn reconcile_profiles(
        &self,
        profiles: Arc<Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        CoreLifecycleClient::reconcile_profiles(self, profiles, staged_content).await
    }

    fn request_runtime_rebuild(&self) {
        CoreLifecycleClient::request_runtime_rebuild(self);
    }
}

pub(super) struct ApplicationWorkflow {
    pub(super) core: Arc<dyn ProfileRuntime>,
    pub(super) outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
    pub(super) recovery_required: Arc<Mutex<Option<String>>>,
}

impl ApplicationWorkflow {
    pub(super) fn new(
        core: CoreLifecycleClient,
        outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
        recovery_required: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self::with_runtime(Arc::new(core), outcomes, recovery_required)
    }

    pub(super) fn with_runtime(
        core: Arc<dyn ProfileRuntime>,
        outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
        recovery_required: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            core,
            outcomes,
            recovery_required,
        }
    }

    pub(super) async fn mark_recovery_required(&self, message: String) {
        *self.recovery_required.lock().await = Some(message);
    }
}

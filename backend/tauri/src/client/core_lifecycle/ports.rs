//! Ports used by the client-owned core lifecycle boundary.

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use chimera_ipc::api::status::CoreState;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::sync::Arc;

use crate::{
    client::runtime::RuntimeSnapshot,
    config::{
        chimera::ClashCore,
        profile::item_type::{ProfileUid, ScriptType},
    },
    core::clash::{api::ClashRuntimeConfig, core::RunType},
    enhance::PostProcessingOutput,
};

/// Narrow boundary around the running core's `/configs` API.
///
/// The legacy API remains behind this port while the ref core lifecycle is
/// migrated. Keeping read and patch together lets the transaction path verify
/// the applied state without reaching into the global core manager directly.
#[async_trait]
pub(crate) trait RunningConfigPort: Send + Sync {
    async fn read(&self) -> anyhow::Result<ClashRuntimeConfig>;
    async fn patch(&self, patch: &Mapping) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeTransformFailureDiagnostics {
    pub attempt_revision: u64,
    pub transform_uid: ProfileUid,
    pub scope_uid: Option<ProfileUid>,
    pub script_type: Option<ScriptType>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeTransformDiagnostics {
    pub revision: u64,
    pub output: PostProcessingOutput,
    pub failure: Option<RuntimeTransformFailureDiagnostics>,
}

#[async_trait]
pub(crate) trait CoreLifecycleLease: Send {
    async fn rebuild_running_config(
        &mut self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()>;
    async fn run_core_from(&mut self, config_path: &std::path::Path) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<()>;
    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync {
    fn init(&self) -> anyhow::Result<()>;
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        Ok(None)
    }

    fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        None
    }

    fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn on_profile_change(&self, break_when: bool);
}

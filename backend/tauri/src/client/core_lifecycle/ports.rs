//! Ports used by the client-owned core lifecycle boundary.

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;

pub(crate) use crate::core::actor_v2::CoreStatusSnapshot;

use crate::{
    client::runtime::RuntimeSnapshot,
    config::{
        chimera::ClashCore,
        profile::item_type::{ProfileUid, ScriptType},
    },
    core::clash::{api::ClashRuntimeConfig, core::RunType},
    enhance::PostProcessingOutput,
};

/// Owns the staged core binary until installation and any restart have finished.
pub(crate) struct PreparedCoreBinary {
    pub(crate) target: ClashCore,
    pub(crate) source: PathBuf,
    pub(crate) destination: PathBuf,
    pub(crate) staging: Arc<TempDir>,
    pub(crate) progress: Arc<dyn BinaryInstallProgress>,
}

pub(crate) trait BinaryInstallProgress: Send + Sync + 'static {
    fn restarting(&self);
    fn finished(&self, error: Option<&str>);
}

#[async_trait]
pub(crate) trait BinaryInstaller: Send + Sync + 'static {
    async fn install(&self, artifact: &PreparedCoreBinary) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait ServiceTransitionLease: Send {
    async fn install_daemon(&mut self) -> anyhow::Result<()>;
    async fn uninstall_daemon(&mut self) -> anyhow::Result<()>;
    async fn update_daemon(&mut self) -> anyhow::Result<()>;
    async fn start_daemon(&mut self) -> anyhow::Result<()>;
    async fn restart_daemon(&mut self) -> anyhow::Result<()>;
    async fn stop_daemon(&mut self) -> anyhow::Result<()>;
    async fn confirm_ready(&mut self, timeout: std::time::Duration) -> anyhow::Result<()>;
    async fn confirm_stopped(&mut self) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait ServiceLifecyclePort: Send + Sync + 'static {
    async fn subscribe_status(
        &self,
    ) -> anyhow::Result<
        tokio::sync::watch::Receiver<crate::core::actor_v2::service_actor::ServiceHostStatus>,
    >;
    fn observe_status(&self, _info: chimera_ipc::types::StatusInfo<'static>) {}
    fn observe_probe_failure(&self) {}
    async fn report_endpoint_down(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>>;
}

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
pub(crate) trait CoreLifecyclePort: Send + Sync {
    async fn reconcile(
        &self,
        clash: ClashConfig,
        profiles: crate::config::profile::profiles::Profiles,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    async fn recover(&self) -> anyhow::Result<()> {
        anyhow::bail!("lower core recovery is not available")
    }
    async fn change_core(
        &self,
        profiles: crate::config::profile::profiles::Profiles,
        clash_core: ClashCore,
    ) -> anyhow::Result<()>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn api_connection(
        &self,
    ) -> anyhow::Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>> {
        Ok(None)
    }
    fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>>;
    fn outcome_uncertain(&self) -> bool {
        false
    }

    fn lower_operation_info(
        &self,
        _id: u64,
    ) -> Option<crate::core::actor_v2::endpoint::OperationInfo> {
        None
    }

    fn lower_operation_history(&self) -> Vec<crate::core::actor_v2::endpoint::OperationInfo> {
        Vec::new()
    }

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

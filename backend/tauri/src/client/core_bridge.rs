//! Core lifecycle boundary between the application client and the legacy core manager.

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use chimera_ipc::api::status::CoreState;
use serde::{Deserialize, Serialize};

use super::ChimeraClient;

use crate::{
    config::{
        chimera::ClashCore,
        profile::item_type::{ProfileUid, ScriptType},
    },
    core::{
        clash::core::{CoreLifecycleLease as CoreManagerLifecycleLease, CoreManager, RunType},
        connection_interruption::ConnectionInterruptionService,
    },
    enhance::PostProcessingOutput,
};

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
    async fn rebuild_running_config(&mut self, clash: ClashConfig) -> anyhow::Result<()>;
    async fn run_core_from(&mut self, config_path: &std::path::Path) -> anyhow::Result<()>;
    async fn stop(&mut self) -> anyhow::Result<()>;
    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        Ok(None)
    }

    async fn on_profile_change(&self);
}

pub(crate) struct LegacyCoreBridge;

struct LegacyCoreLifecycleLease {
    lease: CoreManagerLifecycleLease<'static>,
}

#[async_trait]
impl CoreLifecycleLease for LegacyCoreLifecycleLease {
    async fn rebuild_running_config(&mut self, clash: ClashConfig) -> anyhow::Result<()> {
        self.lease.rebuild_running_config_with(clash).await
    }

    async fn run_core_from(&mut self, config_path: &std::path::Path) -> anyhow::Result<()> {
        self.lease.run_core_from(config_path).await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.lease.stop_core().await
    }

    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()> {
        self.lease.change_core(clash_core).await
    }
}

#[async_trait]
impl CoreLifecyclePort for LegacyCoreBridge {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        Ok(Box::new(LegacyCoreLifecycleLease {
            lease: CoreManager::global().begin_lifecycle().await,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at, run_type) = CoreManager::global().status().await;
        Ok(CoreStatusSnapshot {
            state: state.into_owned(),
            state_changed_at,
            run_type,
        })
    }

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        let core = CoreManager::global();
        let failure =
            core.runtime_transform_failure()
                .map(|failure| RuntimeTransformFailureDiagnostics {
                    attempt_revision: failure.attempt_revision.get(),
                    transform_uid: failure.transform_uid,
                    scope_uid: failure.scope_uid,
                    script_type: failure.script_type,
                    message: failure.message,
                });
        Ok(core
            .runtime_transform_output()
            .map(|(revision, output)| RuntimeTransformDiagnostics {
                revision,
                output,
                failure,
            }))
    }

    async fn on_profile_change(&self) {
        let _ = ConnectionInterruptionService::on_profile_change().await;
    }
}

pub(crate) struct CoreUpdateLease {
    lease: Box<dyn CoreLifecycleLease>,
}

impl CoreUpdateLease {
    pub(crate) async fn stop(&mut self) -> anyhow::Result<()> {
        self.lease.stop().await
    }

    pub(crate) async fn run_core_from(
        &mut self,
        config_path: &std::path::Path,
    ) -> anyhow::Result<()> {
        self.lease.run_core_from(config_path).await
    }
}

impl ChimeraClient {
    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.core.status().await
    }

    pub(crate) fn runtime_transform_diagnostics(
        &self,
    ) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        self.inner.core.runtime_transform_diagnostics()
    }

    pub(crate) async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.change_core(clash_core).await
    }

    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.stop().await
    }

    pub(crate) async fn begin_core_update(&self) -> anyhow::Result<CoreUpdateLease> {
        Ok(CoreUpdateLease {
            lease: self.inner.core.begin().await?,
        })
    }
}

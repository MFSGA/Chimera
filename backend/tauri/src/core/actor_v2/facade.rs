//! Control helpers owned by the lower core-host boundary.
//!
//! This is the staged Chimera counterpart of ref `core/actor_v2/facade.rs`.
//! It centralizes ownership of the legacy `CoreManager` and its lifecycle
//! lock without pretending that Chimera already has ref's submit/wait protocol.

use std::sync::Arc;

use chimera_config::clash::config::ClashConfig;

use super::endpoint::CoreStatusSnapshot;
use crate::{
    client::runtime::{RuntimeSnapshot, RuntimeTransformFailure},
    config::{chimera::ClashCore, clash::ClashInfo},
    core::{
        clash::{
            api::ApiClient,
            core::{CoreManager, RunType},
        },
        connection_interruption::ConnectionInterruptionService,
    },
    enhance::PostProcessingOutput,
};

#[derive(Debug)]
pub(crate) struct CoreFacade {
    manager: Arc<CoreManager>,
}

impl CoreFacade {
    pub(crate) fn new_local() -> Self {
        Self {
            manager: Arc::new(CoreManager::new()),
        }
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        let lease = self.manager.begin_lifecycle().await;
        lease
            .rebuild_running_config_with(clash, target_core, run_type)
            .await
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        let lease = self.manager.begin_lifecycle().await;
        lease.stop_core().await
    }

    pub(crate) async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        let lease = self.manager.begin_lifecycle().await;
        lease.change_core(clash_core).await
    }

    pub(crate) async fn status(&self) -> CoreStatusSnapshot {
        let (state, state_changed_at, run_type) = self.manager.status().await;
        CoreStatusSnapshot {
            state: state.into_owned(),
            state_changed_at,
            run_type,
        }
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.manager.recovery_notify()
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        self.manager.runtime_transform_output()
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.manager.promoted_runtime_snapshot()
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.manager.runtime_transform_failure()
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        self.manager.effective_clash_info()
    }

    pub(crate) async fn on_profile_change(&self, break_when: bool) {
        let result = match ApiClient::new(self.effective_clash_info()) {
            Ok(api) => ConnectionInterruptionService::on_profile_change(&api, break_when).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            tracing::warn!(%error, "failed to interrupt connections after profile change");
        }
    }
}

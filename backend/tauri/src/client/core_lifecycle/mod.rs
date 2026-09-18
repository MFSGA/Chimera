//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Chimera currently routes it to the legacy CoreManager adapter; the public
//! ports allow the actor-backed lifecycle to be introduced without changing
//! application callers or the legacy UI.

pub(crate) mod adapters;
pub(crate) mod ports;

use std::sync::Arc;

use super::ChimeraClient;

#[allow(unused_imports)]
pub(crate) use adapters::{CoreUpdateLease, LegacyCoreBridge, LegacyRunningConfigBridge};
#[allow(unused_imports)]
pub(crate) use ports::{
    CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot, RunningConfigPort,
    RuntimeTransformDiagnostics,
};

impl ChimeraClient {
    pub(crate) fn init_core(&self) -> anyhow::Result<()> {
        self.inner.core.init()
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.core.status().await
    }

    pub(crate) fn runtime_transform_diagnostics(
        &self,
    ) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        self.inner.core.runtime_transform_diagnostics()
    }

    pub(crate) fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        self.inner.core.effective_clash_info()
    }

    pub(crate) fn promoted_runtime_snapshot(
        &self,
    ) -> Option<Arc<crate::client::runtime::RuntimeSnapshot>> {
        self.inner.core.promoted_runtime_snapshot()
    }

    pub(crate) async fn change_core(
        &self,
        clash_core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.change_core(clash_core).await
    }

    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.stop().await
    }

    pub(crate) async fn begin_core_update(&self) -> anyhow::Result<CoreUpdateLease<'_>> {
        Ok(CoreUpdateLease {
            lease: self.inner.core.begin().await?,
        })
    }
}

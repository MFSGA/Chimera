//! Legacy adapters retained behind the client core-lifecycle ports.

use anyhow::Context;
use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use serde_yaml::Mapping;
use std::sync::Arc;

use super::ports::{
    BinaryInstaller, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary, RunningConfigPort,
    RuntimeTransformDiagnostics, RuntimeTransformFailureDiagnostics, ServiceLifecyclePort,
    ServiceTransitionLease,
};
use crate::{
    client::runtime::RuntimeSnapshot,
    config::chimera::ClashCore,
    core::{
        actor_v2::CoreFacade,
        clash::{api::ClashRuntimeConfig, core::RunType},
    },
};

pub(crate) struct FsBinaryInstaller;

#[async_trait]
impl BinaryInstaller for FsBinaryInstaller {
    async fn install(&self, artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
        match tokio::fs::copy(&artifact.source, &artifact.destination).await {
            Ok(size) => {
                tracing::debug!(
                    source = ?artifact.source,
                    destination = ?artifact.destination,
                    size,
                    "installed core binary"
                );
                Ok(())
            }
            Err(error) => {
                tracing::warn!(%error, "core copy failed; requesting elevated installation");
                let source = artifact.source.clone();
                let destination = artifact.destination.clone();
                let staging = artifact.staging.clone();
                let status = tokio::task::spawn_blocking(move || {
                    let _staging = staging;
                    #[cfg(target_os = "windows")]
                    {
                        let source = source
                            .to_str()
                            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 core source path"))?;
                        let destination = destination
                            .to_str()
                            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 core destination path"))?;
                        Ok::<_, anyhow::Error>(
                            runas::Command::new("cmd")
                                .args(&[
                                    "/C",
                                    "copy",
                                    "/Y",
                                    source,
                                    destination.trim_start_matches(r"\\?\"),
                                ])
                                .status()?,
                        )
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        Ok::<_, anyhow::Error>(
                            runas::Command::new("cp")
                                .arg("-f")
                                .arg(source)
                                .arg(destination)
                                .status()?,
                        )
                    }
                })
                .await??;
                anyhow::ensure!(status.success(), "failed to install core binary: {status}");
                Ok(())
            }
        }
    }
}

pub(crate) struct LegacyServiceBridge;

struct LegacyServiceTransition {
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

#[async_trait]
impl ServiceLifecyclePort for LegacyServiceBridge {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        crate::core::service::control::status().await
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(LegacyServiceTransition {
            _guard: crate::core::service::HOST_TRANSITION_LOCK.lock().await,
        }))
    }
}

#[async_trait]
impl ServiceTransitionLease for LegacyServiceTransition {
    async fn install_daemon(&mut self) -> anyhow::Result<()> {
        crate::core::service::control::install_service_daemon().await
    }

    async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        crate::core::service::control::uninstall_service().await
    }

    async fn update_daemon(&mut self) -> anyhow::Result<()> {
        crate::core::service::control::update_service().await
    }

    async fn start_daemon(&mut self) -> anyhow::Result<()> {
        crate::core::service::control::start_service_daemon().await
    }

    async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        crate::core::service::control::restart_service_daemon().await
    }

    async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        crate::core::service::control::stop_service().await
    }

    async fn confirm_ready(&mut self, timeout: std::time::Duration) -> anyhow::Result<()> {
        crate::core::service::ipc::wait_until_ready(timeout).await?;
        Ok(())
    }

    async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        let observation = crate::core::service::ipc::refresh_state_now()
            .await
            .context("failed to verify Chimera Service after stop")?;
        if observation.status == chimera_ipc::types::ServiceStatus::Running {
            anyhow::bail!("Chimera Service still reports running after stop");
        }
        crate::core::service::ipc::mark_disconnected_now();
        Ok(())
    }
}

pub(crate) struct LegacyRunningConfigBridge {
    core: Arc<dyn CoreLifecyclePort>,
}

impl LegacyRunningConfigBridge {
    pub(crate) fn new(core: Arc<dyn CoreLifecyclePort>) -> Self {
        Self { core }
    }

    fn api_client(&self) -> anyhow::Result<crate::core::clash::api::ApiClient> {
        crate::core::clash::api::ApiClient::new(self.core.effective_clash_info())
    }
}

#[async_trait]
impl RunningConfigPort for LegacyRunningConfigBridge {
    async fn read(&self) -> anyhow::Result<ClashRuntimeConfig> {
        self.api_client()?.get_configs().await
    }

    async fn patch(&self, patch: &Mapping) -> anyhow::Result<()> {
        self.api_client()?.patch_configs(patch).await
    }
}

pub(crate) struct LegacyCoreBridge {
    facade: Arc<CoreFacade>,
}

impl LegacyCoreBridge {
    pub(crate) fn new(facade: Arc<CoreFacade>) -> Self {
        Self { facade }
    }

    /// Compatibility boundary for the staged lifecycle migration.
    ///
    /// The concrete CoreManager is owned by the lower core facade; this adapter
    /// only maps that lower host boundary into the client lifecycle port.
    fn facade(&self) -> &Arc<CoreFacade> {
        &self.facade
    }
}

#[async_trait]
impl CoreLifecyclePort for LegacyCoreBridge {
    async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        self.facade().reconcile(clash, target_core, run_type).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.facade().stop().await
    }

    async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        self.facade().change_core(clash_core).await
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(self.facade().status().await)
    }

    fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
        Some(self.facade().recovery_notify())
    }

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        let core = self.facade();
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

    fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.facade().promoted_runtime_snapshot()
    }

    fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        self.facade().effective_clash_info()
    }

    async fn on_profile_change(&self, break_when: bool) {
        self.facade().on_profile_change(break_when).await;
    }
}

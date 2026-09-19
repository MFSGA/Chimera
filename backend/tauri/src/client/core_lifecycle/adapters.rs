//! Legacy adapters retained behind the client core-lifecycle ports.

use anyhow::Context;
use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use serde_yaml::Mapping;
use std::sync::Arc;

use super::ports::{
    BinaryInstaller, CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
    RunningConfigPort, RuntimeTransformDiagnostics, RuntimeTransformFailureDiagnostics,
    ServiceLifecyclePort, ServiceTransitionLease,
};
use crate::{
    client::runtime::RuntimeSnapshot,
    config::chimera::ClashCore,
    core::{
        clash::{
            api::ClashRuntimeConfig,
            core::{CoreLifecycleLease as CoreManagerLifecycleLease, CoreManager, RunType},
        },
        connection_interruption::ConnectionInterruptionService,
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
    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(LegacyServiceTransition {
            _guard: crate::core::service::HOST_TRANSITION_LOCK.lock().await,
        }))
    }
}

#[async_trait]
impl ServiceTransitionLease for LegacyServiceTransition {
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
    manager: Arc<CoreManager>,
}

impl LegacyCoreBridge {
    pub(crate) fn new() -> Self {
        Self {
            manager: Arc::new(CoreManager::new()),
        }
    }

    /// Compatibility boundary for the staged lifecycle migration.
    ///
    /// New lifecycle implementations can replace this adapter without
    /// changing client callers.
    fn manager(&self) -> &Arc<CoreManager> {
        &self.manager
    }
}

struct LegacyCoreLifecycleLease<'a> {
    lease: CoreManagerLifecycleLease<'a>,
}

#[async_trait]
impl CoreLifecycleLease for LegacyCoreLifecycleLease<'_> {
    async fn rebuild_running_config(
        &mut self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        self.lease
            .rebuild_running_config_with(clash, target_core, run_type)
            .await
    }

    async fn run_core_from(
        &mut self,
        config_path: &std::path::Path,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        self.lease
            .run_core_from(config_path, target_core, run_type)
            .await
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
    fn init(&self) -> anyhow::Result<()> {
        self.manager().init()
    }

    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
        Ok(Box::new(LegacyCoreLifecycleLease {
            lease: self.manager().begin_lifecycle().await,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at, run_type) = self.manager().status().await;
        Ok(CoreStatusSnapshot {
            state: state.into_owned(),
            state_changed_at,
            run_type,
        })
    }

    async fn recover(&self) -> anyhow::Result<()> {
        self.manager().recover_core_once().await
    }

    fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
        Some(self.manager().recovery_notify())
    }

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        let core = self.manager();
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
        self.manager().promoted_runtime_snapshot()
    }

    fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        self.manager().effective_clash_info()
    }

    async fn on_profile_change(&self, break_when: bool) {
        let result =
            match crate::core::clash::api::ApiClient::new(self.manager().effective_clash_info()) {
                Ok(api) => ConnectionInterruptionService::on_profile_change(&api, break_when).await,
                Err(error) => Err(error),
            };
        if let Err(error) = result {
            tracing::warn!(%error, "failed to interrupt connections after profile change");
        }
    }
}

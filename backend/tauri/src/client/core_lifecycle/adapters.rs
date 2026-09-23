//! Legacy adapters retained behind the client core-lifecycle ports.

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use serde_yaml::Mapping;
use std::sync::Arc;

use super::ports::{
    BinaryInstaller, CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
    RunningConfigPort, RuntimeTransformDiagnostics, RuntimeTransformFailureDiagnostics,
};

pub(crate) struct FsBinaryInstaller;

#[async_trait]
impl BinaryInstaller for FsBinaryInstaller {
    async fn install(&self, artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
        if let Err(error) = tokio::fs::copy(&artifact.source, &artifact.destination).await {
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
        }
        Ok(())
    }
}
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

pub(crate) struct LegacyRunningConfigBridge;

#[async_trait]
impl RunningConfigPort for LegacyRunningConfigBridge {
    async fn read(&self) -> anyhow::Result<ClashRuntimeConfig> {
        crate::core::clash::api::get_configs().await
    }

    async fn patch(&self, patch: &Mapping) -> anyhow::Result<()> {
        crate::core::clash::api::patch_configs(patch).await
    }
}

pub(crate) struct LegacyCoreBridge;

struct LegacyCoreLifecycleLease {
    lease: CoreManagerLifecycleLease<'static>,
}

#[async_trait]
impl CoreLifecycleLease for LegacyCoreLifecycleLease {
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

    async fn recover(&self) -> anyhow::Result<()> {
        CoreManager::global().recover_core_once().await
    }

    fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
        Some(CoreManager::global().recovery_notify())
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

    fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        CoreManager::global().promoted_runtime_snapshot()
    }

    async fn on_profile_change(&self, break_when: bool) {
        let _ = ConnectionInterruptionService::on_profile_change(break_when).await;
    }
}

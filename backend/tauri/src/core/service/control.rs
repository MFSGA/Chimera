//! Legacy Tauri read-only Service helpers pending full CoreActor ownership migration.
//!
//! Production Service mutations use `chimera_os::service::OsServiceProcessControl`,
//! constructed in `setup.rs` with the shared `chimera_paths::PathResolver`.

use std::{sync::Arc, time::Duration};

use chimera_contracts::service::{ServiceCommandError, ServiceProcessControl};
use chimera_core::actor_v2::service_process::parse_service_status;
use chimera_os::service;

use super::SERVICE_PATH;
use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};

const SERVICE_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Legacy IPC preview until all caller paths are injected from the app resolver.
pub async fn get_service_install_args() -> anyhow::Result<Vec<std::ffi::OsString>> {
    let data = app_data_dir()?;
    let config = app_config_dir()?;
    let install = app_install_dir()?;
    Ok(service::get_service_install_args(&data, &config, &install).await?)
}

/// Legacy read-only status entry point; no OS service mutations occur here.
pub(crate) async fn probe_service_status()
-> Result<chimera_ipc::types::StatusInfo<'static>, ServiceCommandError> {
    let raw = tokio::time::timeout(
        SERVICE_STATUS_TIMEOUT,
        service::status(SERVICE_PATH.as_path()),
    )
    .await
    .map_err(|_| ServiceCommandError::TimedOut {
        limit_ms: SERVICE_STATUS_TIMEOUT.as_millis() as u64,
    })??;
    parse_service_status(&raw)
}

/// Chimera's legacy IPC health loop must be retired after a successful OS stop
/// or uninstall. Keep this notification at the shell boundary; all actual
/// service process commands remain owned by the upstream-shaped OS adapter.
pub(crate) struct HealthMonitorStopAdapter {
    process: Arc<dyn ServiceProcessControl>,
    on_stopped: Arc<dyn Fn() + Send + Sync>,
}

impl HealthMonitorStopAdapter {
    pub(crate) fn new(process: Arc<dyn ServiceProcessControl>) -> Self {
        Self {
            process,
            on_stopped: Arc::new(super::ipc::stop_health_check),
        }
    }
}

#[async_trait::async_trait]
impl ServiceProcessControl for HealthMonitorStopAdapter {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        self.process.probe().await
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        self.process.install().await
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        self.process.uninstall().await?;
        (self.on_stopped)();
        Ok(())
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        self.process.start_daemon().await
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        self.process.stop_daemon().await?;
        (self.on_stopped)();
        Ok(())
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        self.process.update().await
    }

    async fn restart_daemon(&self) -> Result<(), ServiceCommandError> {
        self.process.restart_daemon().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeServiceProcess {
        fail_stop: bool,
        fail_uninstall: bool,
    }

    #[async_trait::async_trait]
    impl ServiceProcessControl for FakeServiceProcess {
        async fn probe(&self) -> Result<String, ServiceCommandError> {
            Ok(String::new())
        }
        async fn install(&self) -> Result<(), ServiceCommandError> {
            Ok(())
        }
        async fn uninstall(&self) -> Result<(), ServiceCommandError> {
            if self.fail_uninstall {
                Err(ServiceCommandError::TaskCancelled)
            } else {
                Ok(())
            }
        }
        async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
            Ok(())
        }
        async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
            if self.fail_stop {
                Err(ServiceCommandError::TaskCancelled)
            } else {
                Ok(())
            }
        }
        async fn update(&self) -> Result<(), ServiceCommandError> {
            Ok(())
        }
        async fn restart_daemon(&self) -> Result<(), ServiceCommandError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn legacy_health_watcher_stops_only_after_confirmed_os_success() {
        let stops = Arc::new(AtomicUsize::new(0));
        for (fail_stop, fail_uninstall) in [(true, true), (false, false)] {
            let mut adapter = HealthMonitorStopAdapter::new(Arc::new(FakeServiceProcess {
                fail_stop,
                fail_uninstall,
            }));
            let observed = stops.clone();
            adapter.on_stopped = Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            });
            let before = stops.load(Ordering::SeqCst);
            assert!(adapter.probe().await.is_ok());
            assert!(adapter.install().await.is_ok());
            assert!(adapter.start_daemon().await.is_ok());
            assert!(adapter.update().await.is_ok());
            assert!(adapter.restart_daemon().await.is_ok());
            assert_eq!(stops.load(Ordering::SeqCst), before);
            assert_eq!(adapter.stop_daemon().await.is_ok(), !fail_stop);
            assert_eq!(adapter.uninstall().await.is_ok(), !fail_uninstall);
            assert_eq!(
                stops.load(Ordering::SeqCst),
                before + if fail_stop { 0 } else { 2 }
            );
        }
    }
}

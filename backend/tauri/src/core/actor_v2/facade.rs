//! Control helpers owned by the lower core-host boundary.
//!
//! This is the staged Chimera counterpart of ref `core/actor_v2/facade.rs`.
//! It centralizes ownership of the legacy `CoreManager`, routes mutations
//! through explicit Local/Service endpoint handles, and keeps one transaction
//! owner while the daemon wire is still on its legacy protocol.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use super::{
    endpoint::{
        ControlEndpoint, CoreCommand, CoreStatusSnapshot, EndpointHandle, ExecutionHost,
        LocalEndpoint, OperationPhase, ServiceEndpoint,
    },
    service_actor::{OsServiceHostAdapter, ServiceClient},
};
use crate::{
    client::runtime::{RuntimeSnapshot, RuntimeTransformFailure},
    config::{chimera::ClashCore, clash::ClashInfo},
    core::{
        clash::{api::ApiClient, core::RunType},
        connection_interruption::ConnectionInterruptionService,
    },
    enhance::PostProcessingOutput,
};
use anyhow::Context;
use chimera_config::clash::config::ClashConfig;

const SERVICE_RESTART_BUDGET: u8 = 3;
const LOCAL_OPERATION_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceUninstallPlan {
    AlreadyAbsent,
    Uninstall,
    StopThenUninstall,
}

fn service_uninstall_plan(
    status: chimera_ipc::types::ServiceStatus,
    core_state: Option<&chimera_ipc::api::status::CoreState>,
) -> anyhow::Result<ServiceUninstallPlan> {
    use chimera_ipc::{api::status::CoreState, types::ServiceStatus};

    match status {
        ServiceStatus::NotInstalled => Ok(ServiceUninstallPlan::AlreadyAbsent),
        ServiceStatus::Stopped => Ok(ServiceUninstallPlan::Uninstall),
        ServiceStatus::Running => match core_state {
            Some(CoreState::Stopped(_)) => Ok(ServiceUninstallPlan::StopThenUninstall),
            Some(CoreState::Running) => anyhow::bail!(
                "the service daemon still owns a running core; hand off to Local before uninstall"
            ),
            None => anyhow::bail!(
                "the service daemon is running but core ownership cannot be determined; stop it before uninstall"
            ),
        },
    }
}

pub(crate) struct CoreFacade {
    local_endpoint: Arc<LocalEndpoint>,
    service_endpoint: EndpointHandle,
    outcome_uncertain: Arc<AtomicBool>,
    service_client: tokio::sync::OnceCell<ServiceClient>,
    service_restart_attempts: Arc<AtomicU8>,
    service_restart_exhausted: Arc<AtomicBool>,
}

pub(crate) struct ServiceTransition {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    client: ServiceClient,
}

impl CoreFacade {
    pub(crate) fn new_local() -> Self {
        let local_endpoint = Arc::new(LocalEndpoint::new());
        let service_endpoint: EndpointHandle =
            Arc::new(ServiceEndpoint::new(local_endpoint.clone()));
        Self {
            local_endpoint,
            service_endpoint,
            outcome_uncertain: Arc::new(AtomicBool::new(false)),
            service_client: tokio::sync::OnceCell::new(),
            service_restart_attempts: Arc::new(AtomicU8::new(0)),
            service_restart_exhausted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    pub(crate) fn operation_info(&self, id: u64) -> Option<super::endpoint::OperationInfo> {
        self.local_endpoint
            .operation_info(super::endpoint::OperationId::from_raw(id))
    }

    pub(crate) fn operation_history(&self) -> Vec<super::endpoint::OperationInfo> {
        self.local_endpoint.operation_history()
    }

    async fn service_client(&self) -> anyhow::Result<&ServiceClient> {
        let outcome_uncertain = self.outcome_uncertain.clone();
        let restart_attempts = self.service_restart_attempts.clone();
        let restart_exhausted = self.service_restart_exhausted.clone();
        self.service_client
            .get_or_try_init(|| async move {
                ServiceClient::spawn(
                    Arc::new(OsServiceHostAdapter),
                    outcome_uncertain,
                    restart_attempts,
                    restart_exhausted,
                    SERVICE_RESTART_BUDGET,
                )
                .await
            })
            .await
    }

    fn endpoint_for_run_type(&self, run_type: RunType) -> EndpointHandle {
        match run_type {
            RunType::Service => self.service_endpoint.clone(),
            RunType::Normal | RunType::Elevated => self.local_endpoint.clone(),
        }
    }

    async fn active_endpoint(&self) -> anyhow::Result<EndpointHandle> {
        let status = self.local_endpoint.status().await?;
        Ok(self.endpoint_for_run_type(status.run_type))
    }

    async fn run_mutation(
        &self,
        endpoint: EndpointHandle,
        command: CoreCommand,
    ) -> anyhow::Result<()> {
        if self.outcome_uncertain() {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }
        let admitted = endpoint.submit(command).await?;
        let id = admitted.id;
        match endpoint.wait_operation(id, LOCAL_OPERATION_WAIT).await {
            Some(info) if info.phase == OperationPhase::Succeeded => Ok(()),
            Some(info) if info.phase == OperationPhase::Failed => {
                Err(anyhow::anyhow!(info.error.unwrap_or_else(|| {
                    "lower core operation failed without an error".to_string()
                })))
            }
            Some(info) if info.phase == OperationPhase::Uncertain => {
                self.outcome_uncertain.store(true, Ordering::Release);
                Err(anyhow::anyhow!(info.error.unwrap_or_else(|| {
                    "lower core operation reached an uncertain terminal state".to_string()
                })))
            }
            Some(_) | None => {
                self.outcome_uncertain.store(true, Ordering::Release);
                anyhow::bail!(
                    "lower core operation {} did not reach a terminal state within the wait budget",
                    id.get()
                )
            }
        }
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        let expected_applied = self
            .local_endpoint
            .status()
            .await?
            .applied
            .map(|identity| identity.revision);
        let endpoint = self.endpoint_for_run_type(run_type);
        debug_assert_eq!(
            endpoint.host(),
            if run_type == RunType::Service {
                ExecutionHost::Service
            } else {
                ExecutionHost::Local
            }
        );
        self.run_mutation(
            endpoint,
            CoreCommand::Reconcile {
                clash,
                target_core,
                run_type,
                expected_applied,
            },
        )
        .await
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        let endpoint = self.active_endpoint().await?;
        self.run_mutation(endpoint, CoreCommand::Stop).await
    }

    pub(crate) async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        let endpoint = self.active_endpoint().await?;
        self.run_mutation(endpoint, CoreCommand::ChangeCore(clash_core))
            .await
    }

    pub(crate) async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.local_endpoint.status().await
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.local_endpoint.recovery_notify()
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        self.local_endpoint.runtime_transform_output()
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.local_endpoint.promoted_runtime_snapshot()
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.local_endpoint.runtime_transform_failure()
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        self.local_endpoint.effective_clash_info()
    }

    pub(crate) async fn service_status_receiver(
        &self,
    ) -> anyhow::Result<tokio::sync::watch::Receiver<super::service_actor::ServiceHostStatus>> {
        Ok(self.service_client().await?.subscribe())
    }

    pub(crate) fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        if let Some(client) = self.service_client.get() {
            client.observe(info);
        }
    }

    pub(crate) fn observe_service_probe_failure(&self) {
        if let Some(client) = self.service_client.get() {
            client.observe_probe_failure();
        }
    }

    pub(crate) async fn report_service_endpoint_down(&self) -> anyhow::Result<()> {
        self.service_client().await?.report_endpoint_down();
        Ok(())
    }

    pub(crate) async fn begin_service_transition(&self) -> anyhow::Result<ServiceTransition> {
        if self.outcome_uncertain() {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }
        let client = self.service_client().await?.clone();
        Ok(ServiceTransition {
            _guard: crate::core::service::HOST_TRANSITION_LOCK.lock().await,
            client,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_selection_maps_run_type_to_explicit_host_handle() {
        let facade = CoreFacade::new_local();
        assert_eq!(
            facade.endpoint_for_run_type(RunType::Normal).host(),
            ExecutionHost::Local
        );
        assert_eq!(
            facade.endpoint_for_run_type(RunType::Service).host(),
            ExecutionHost::Service
        );
        assert_eq!(
            facade.endpoint_for_run_type(RunType::Elevated).host(),
            ExecutionHost::Local
        );
    }

    #[test]
    fn uninstall_guard_requires_proof_that_service_owns_no_running_core() {
        use chimera_ipc::{api::status::CoreState, types::ServiceStatus};

        assert_eq!(
            service_uninstall_plan(ServiceStatus::NotInstalled, None).unwrap(),
            ServiceUninstallPlan::AlreadyAbsent
        );
        assert_eq!(
            service_uninstall_plan(ServiceStatus::Stopped, None).unwrap(),
            ServiceUninstallPlan::Uninstall
        );
        assert_eq!(
            service_uninstall_plan(ServiceStatus::Running, Some(&CoreState::Stopped(None)))
                .unwrap(),
            ServiceUninstallPlan::StopThenUninstall
        );
        assert!(service_uninstall_plan(ServiceStatus::Running, Some(&CoreState::Running)).is_err());
        assert!(service_uninstall_plan(ServiceStatus::Running, None).is_err());
    }
}

impl ServiceTransition {
    pub(crate) async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.client.install().await
    }

    pub(crate) async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        let info = self
            .client
            .probe()
            .await
            .context("failed to determine service core ownership before uninstall")?;
        let core_state = info.server.as_ref().map(|server| &server.core_infos.state);
        match service_uninstall_plan(info.status, core_state)? {
            ServiceUninstallPlan::AlreadyAbsent => Ok(()),
            ServiceUninstallPlan::Uninstall => self.client.uninstall().await,
            ServiceUninstallPlan::StopThenUninstall => {
                self.client.stop().await?;
                let stopped = self.client.probe().await?;
                anyhow::ensure!(
                    stopped.status != chimera_ipc::types::ServiceStatus::Running,
                    "service daemon did not prove it stopped; uninstall was refused"
                );
                self.client.uninstall().await
            }
        }
    }

    pub(crate) async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.client.update().await
    }

    pub(crate) async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.client.start().await
    }

    pub(crate) async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.client.restart().await
    }

    pub(crate) async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        self.client.stop().await
    }

    pub(crate) async fn confirm_ready(&mut self, timeout: Duration) -> anyhow::Result<()> {
        crate::core::service::ipc::wait_until_ready(timeout).await?;
        self.client.probe().await?;
        Ok(())
    }

    pub(crate) async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        let observation = crate::core::service::ipc::refresh_state_now()
            .await
            .context("failed to verify Chimera Service after stop")?;
        if observation.status == chimera_ipc::types::ServiceStatus::Running {
            anyhow::bail!("Chimera Service still reports running after stop");
        }
        self.client.probe().await?;
        crate::core::service::ipc::mark_disconnected_now();
        Ok(())
    }
}

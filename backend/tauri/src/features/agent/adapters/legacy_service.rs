use chimera_ipc::types::ServiceStatus;

use crate::core::service;

use super::super::{
    model::AgentServiceState,
    ports::{ServiceControlPort, ServiceLifecycleStatus},
};

// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: service lifecycle and IPC health are still owned by module-level legacy state.
// Remove when: ServiceClient is injected through NyanpasuClient.
pub(crate) struct LegacyServiceControl;

impl LegacyServiceControl {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ServiceControlPort for LegacyServiceControl {
    async fn status(&self) -> anyhow::Result<ServiceLifecycleStatus> {
        let status = service::control::status().await?;
        Ok(ServiceLifecycleStatus {
            state: match status.status {
                ServiceStatus::NotInstalled => AgentServiceState::NotInstalled,
                ServiceStatus::Stopped => AgentServiceState::Stopped,
                ServiceStatus::Running => AgentServiceState::Running,
            },
            runtime_compatible: matches!(status.status, ServiceStatus::Running)
                .then(|| service::is_service_runtime_compatible(&status)),
        })
    }

    fn ipc_connected(&self) -> bool {
        service::ipc::get_ipc_state().is_connected()
    }

    async fn start(&self) -> anyhow::Result<()> {
        service::control::start_service().await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        service::control::stop_service().await
    }

    async fn restart(&self) -> anyhow::Result<()> {
        service::control::restart_service().await
    }
}

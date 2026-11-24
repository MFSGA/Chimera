//! Conditional Core retirement and selected-host ownership operations.
use crate::client::application_workflow::startup::{ServiceEvidence, StartupOwnerFailure};
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::core_lifecycle::ports::CoreStatusSnapshot;
use std::sync::atomic::Ordering;

impl CoreLifecycleWorkflow {
    /// A refused Service submission may have already applied its Core but
    /// failed *afterwards* while promoting RuntimeProduct. The startup owner
    /// may release Service only after a stable, non-synthetic Stopped state
    /// with no accepted Core receipt. This independent proof also protects
    /// other errors that arrived after a concurrent owner transition.
    pub(in crate::client) async fn startup_service_refusal_stopped(&self) -> bool {
        chimera_core::client::reconciler::proof::startup_service_refusal_stopped(self.core.as_ref())
            .await
    }

    pub(in crate::client) async fn startup_service_command_settled(&self) -> bool {
        self.service.command_settled().await
    }

    pub(in crate::client) fn startup_failed_generation(&self) -> u64 {
        self.service_failed_generation_shared
            .load(Ordering::Acquire)
    }

    pub(in crate::client) fn set_startup_failed_generation(&self, generation: u64) {
        self.service_failed_generation_shared
            .store(generation, Ordering::Release);
    }

    pub(in crate::client) async fn startup_select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        self.core.select_execution_host(host).await?;
        self.follow_service_recovery_host(host);
        Ok(())
    }

    pub(in crate::client) async fn confirm_local_owner_stopped(
        &self,
        current: &CoreStatusSnapshot,
    ) -> Result<(), StartupOwnerFailure> {
        chimera_core::client::core_lifecycle::ownership::confirm_local_owner_stopped(
            self.core.as_ref(),
            current,
        )
        .await
    }
    pub(in crate::client) async fn confirm_local_stopped_baseline(
        &self,
        stopped: &CoreStatusSnapshot,
    ) -> Result<(), String> {
        chimera_core::client::core_lifecycle::ownership::confirm_local_stopped_baseline(
            self.core.as_ref(),
            stopped,
        )
        .await
    }
    pub(in crate::client) async fn recover_service_error_stop(
        &self,
        service: ServiceEvidence,
        previous_generation: u64,
    ) -> Result<(ServiceEvidence, u64), StartupOwnerFailure> {
        chimera_core::client::core_lifecycle::ownership::recover_service_error_stop(
            self.core.as_ref(),
            || self.service_evidence(),
            service,
            previous_generation,
        )
        .await
    }
    pub(in crate::client) async fn retire_service_startup_owner(
        &self,
        service: &ServiceEvidence,
        require_ready_stopped: bool,
    ) -> Result<ServiceEvidence, StartupOwnerFailure> {
        chimera_core::client::core_lifecycle::ownership::retire_service_startup_owner(
            self.core.as_ref(),
            || self.service_evidence(),
            service,
            require_ready_stopped,
        )
        .await
    }
}

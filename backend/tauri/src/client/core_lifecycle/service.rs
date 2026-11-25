//! Lower Service operations and bounded endpoint recovery.
use crate::client::application_workflow::startup::ServiceEvidence;
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::core_lifecycle::ports::ServiceTransitionLease;
use chimera_core::actor_v2::endpoint::ExecutionHost;
use std::sync::atomic::Ordering;

impl CoreLifecycleWorkflow {
    pub(in crate::client) fn service_restart_policy(
        &self,
    ) -> chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
        self.service.restart_policy()
    }

    pub(in crate::client) fn follow_service_recovery_host(&self, host: ExecutionHost) {
        self.service_recovery.lock().follow_host(host);
    }

    pub(in crate::client) async fn refresh_service_recovery_host(&self) {
        chimera_core::client::core_lifecycle::service::refresh_service_recovery_host(
            self.core.as_ref(),
            &self.service_recovery,
        )
        .await
    }

    pub(in crate::client) async fn service_endpoint_recovery_due(&self) -> bool {
        chimera_core::client::core_lifecycle::service::service_endpoint_recovery_due(
            self.core.as_ref(),
            &self.service_recovery,
            &self.shutdown,
        )
        .await
    }

    pub(in crate::client) fn service_recovery_due_now(&self) -> bool {
        chimera_core::client::core_lifecycle::service::service_recovery_due_now(
            self.core.as_ref(),
            &self.service_recovery,
            &self.shutdown,
        )
    }

    pub(in crate::client) fn service_recovery_blocks_handoff(&self) -> bool {
        let endpoint_degraded = self.core.service_endpoint_degraded();
        let recovery = self.service_recovery.lock();
        recovery.blocks_handoff(endpoint_degraded)
    }

    pub(in crate::client) fn take_service_recovery_reconnected(&self) -> bool {
        self.service_recovery_reconnected
            .swap(false, Ordering::AcqRel)
    }

    pub(in crate::client) async fn settle_service_recovery_action(&self) -> bool {
        chimera_core::client::core_lifecycle::service::settle_service_recovery_action(
            self.core.as_ref(),
        )
        .await
    }

    pub(in crate::client) async fn report_service_endpoint_down(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<bool> {
        if !self.core.service_endpoint_recovery_supported() {
            self.service.report_endpoint_down(transition).await?;
            return Ok(false);
        }
        chimera_core::client::core_lifecycle::service::report_service_endpoint_down(
            self.core.as_ref(),
            &self.service_recovery,
            &self.shutdown,
        )
        .await
    }

    pub(in crate::client) async fn release_service_owner(&self) -> Result<ServiceEvidence, String> {
        chimera_core::client::core_lifecycle::ownership::release_service_owner(
            self.core.as_ref(),
            || self.service_evidence(),
        )
        .await
    }

    pub(in crate::client) async fn try_release_service_owner(
        &self,
    ) -> Result<ServiceEvidence, String> {
        let _transition = self.service.try_begin_transition().await.map_err(|error| {
            format!("could not acquire the Service transition lease: {error:#}")
        })?;
        let result = self.release_service_owner().await;
        self.refresh_service_recovery_host().await;
        result
    }

    pub(in crate::client) async fn try_begin_service_transition(
        &self,
    ) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.service.try_begin_transition().await
    }

    pub(in crate::client) async fn service_evidence(&self) -> (ServiceEvidence, u64) {
        chimera_core::client::core_lifecycle::service::service_evidence(
            self.service.as_ref(),
            self.service_status.as_ref(),
        )
        .await
    }
}

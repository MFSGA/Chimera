//! Desktop lifecycle host adapter to the shared Service owner.
use crate::client::application_workflow::startup::ServiceEvidence;
use crate::client::core_lifecycle::CoreLifecycleWorkflow;

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn service_endpoint_recovery_due(&self) -> bool {
        self.shared_state().service_endpoint_recovery_due().await
    }
    pub(in crate::client) async fn try_release_service_owner(
        &self,
    ) -> Result<ServiceEvidence, String> {
        self.shared_state().try_release_service_owner().await
    }
    pub(in crate::client) async fn service_evidence(&self) -> (ServiceEvidence, u64) {
        self.shared_state().service_evidence().await
    }
    pub(in crate::client) async fn release_service_owner(&self) -> Result<ServiceEvidence, String> {
        self.shared_state().release_service_owner().await
    }
    pub(in crate::client) fn service_recovery_blocks_handoff(&self) -> bool {
        self.shared_state().service_recovery_blocks_handoff()
    }
}

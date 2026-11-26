use crate::client::core_lifecycle::CoreLifecycleWorkflow;
pub use chimera_core::client::reconciler::release::*;

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn service_release_observation(&self) -> ReleaseObservation {
        chimera_core::client::reconciler::release::service_release_observation(
            self.shared_state(),
            self,
        )
        .await
    }

    pub(in crate::client) async fn maintain_service_release(&self) -> anyhow::Result<()> {
        chimera_core::client::reconciler::release::maintain_release_for_host(
            self.shared_state(),
            self,
        )
        .await
    }
}

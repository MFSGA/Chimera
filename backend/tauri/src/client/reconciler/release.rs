use crate::client::core_lifecycle::CoreLifecycleWorkflow;
pub use chimera_core::client::reconciler::release::*;
use chimera_core::client::reconciler::service_evidence::ServiceEvidence;

impl CoreLifecycleWorkflow {
    /// Independent ref release-owed maintenance: only a verified Local
    /// runtime may retire a daemon whose *stopped* Core is still owned by this
    /// instance. Running, foreign, quarantined and unreadable Service owners
    /// are never grounds for stopping the daemon.
    pub(in crate::client) async fn service_release_observation(
        &self,
    ) -> crate::client::reconciler::release::ReleaseObservation {
        use crate::client::reconciler::release::{ReleaseObservation, observation};

        if self.application.get_typed().enable_service_mode {
            return ReleaseObservation::Cleared;
        }
        let Ok(core) = self.core.status().await else {
            return ReleaseObservation::Unproven;
        };
        let verified_local = core.run_type == crate::core::RunType::Normal
            && self.verified_applied_runtime(&core).await;
        if !verified_local {
            return ReleaseObservation::Unproven;
        }
        let (service, _) = self.service_evidence().await;
        observation(false, verified_local, &service)
    }

    pub(in crate::client) async fn maintain_service_release(&self) -> anyhow::Result<()> {
        chimera_core::client::reconciler::release::maintain_service_release(
            self.core.as_ref(),
            &self.service_recovery,
            || self.service_release_observation(),
            || self.service_evidence(),
            || self.try_begin_service_transition(),
        )
        .await?;
        self.refresh_service_recovery_host().await;
        Ok(())
    }
}

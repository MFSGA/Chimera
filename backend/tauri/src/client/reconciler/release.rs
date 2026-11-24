use crate::client::{
    core_lifecycle::CoreLifecycleWorkflow, reconciler::decision::same_core_status,
};
pub use chimera_core::client::reconciler::release::*;
use chimera_core::client::reconciler::service_evidence::ServiceEvidence;
use chimera_ipc::api::status::CoreState;

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
        use crate::client::reconciler::release::{
            ReleaseObservation, observation, same_release_owner,
        };

        let before = self.core.status().await?;
        anyhow::ensure!(
            self.service_release_observation().await == ReleaseObservation::Owed,
            "Service daemon release is not authorized by a verified Local owner"
        );
        let (service_before, generation_before) = self.service_evidence().await;
        anyhow::ensure!(
            observation(false, true, &service_before) == ReleaseObservation::Owed,
            "Service daemon stopped-owner evidence was lost before acquiring the transition lease"
        );
        // Nonblocking lock acquisition preserves ServiceActor -> CoreLifecycle
        // ordering: a UI command can hold this lock while awaiting our mailbox.
        let mut lease = self.try_begin_service_transition().await?;
        let local_after_lease = self.core.status().await?;
        anyhow::ensure!(
            same_core_status(&before, &local_after_lease)
                && matches!(local_after_lease.state, CoreState::Running),
            "Local Core instance changed before the queued Service release was admitted"
        );
        anyhow::ensure!(
            self.service_release_observation().await == ReleaseObservation::Owed,
            "Service owner evidence changed after acquiring the transition lease"
        );
        let (service_after_lease, generation_after_lease) = self.service_evidence().await;
        anyhow::ensure!(
            same_release_owner(
                &service_before,
                generation_before,
                &service_after_lease,
                generation_after_lease,
            ),
            "Service daemon owner or readiness generation changed before the queued release"
        );
        // The lease proves the stop still targets the same stopped, locally
        // owned daemon. A newer readiness generation must use a new release
        // decision rather than reusing an older convergence observation.
        self.service_recovery.lock().suppress();
        lease.stop_daemon().await?;
        lease.confirm_stopped().await?;
        let (after, _) = self.service_evidence().await;
        anyhow::ensure!(
            matches!(after, ServiceEvidence::Absent { .. }),
            "Service stop was acknowledged but a fresh probe did not prove daemon shutdown: {after:?}"
        );
        let current = self.core.status().await?;
        anyhow::ensure!(
            same_core_status(&before, &current) && matches!(current.state, CoreState::Running),
            "Local Core changed while retiring the Service daemon"
        );
        self.refresh_service_recovery_host().await;
        Ok(())
    }
}

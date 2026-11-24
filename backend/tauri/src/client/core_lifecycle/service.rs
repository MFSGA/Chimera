//! Lower Service operations and bounded endpoint recovery.
use crate::client::application_workflow::startup::ServiceEvidence;
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::core_lifecycle::ports::ServiceTransitionLease;
use chimera_core::actor_v2::endpoint::ExecutionHost;
use chimera_core::actor_v2::service_status::ServiceHostStatus;
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
        let Ok(status) = self.core.status().await else {
            return;
        };
        let host = if status.run_type == crate::core::RunType::Service {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        };
        self.follow_service_recovery_host(host);
    }

    pub(in crate::client) async fn service_endpoint_recovery_due(&self) -> bool {
        if !self.service_recovery_due_now() || !self.settle_service_recovery_action().await {
            return false;
        }
        self.service_recovery_due_now()
    }

    pub(in crate::client) fn service_recovery_due_now(&self) -> bool {
        if !self.core.service_endpoint_recovery_supported() {
            return false;
        }
        self.service_recovery.lock().due(
            self.core.service_endpoint_degraded(),
            self.shutdown.is_cancelled(),
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
        let probe = self.service.probe().await;
        if let Some(status) = self.service.actor_status() {
            // Production uses ref ServiceActor for both the ownership evidence
            // and the readiness generation. Do NOT derive another generation
            // from CoreLifecycle's independent probe cache; it can disagree
            // across a daemon handoff or failed startup attempt.
            let generation = status.ready_generation;
            let evidence = if probe.is_ok() {
                ServiceEvidence::from_status(&status)
            } else {
                // An unanswered ref Actor probe is not a stop or Ready proof,
                // even if a previous watch snapshot was healthy.
                ServiceEvidence::Unreadable {
                    phase: chimera_core::actor_v2::service_policy::ServicePhase::Unknown,
                }
            };
            return (evidence, generation);
        }
        let Ok(info) = probe else {
            let generation = self
                .service_status
                .as_ref()
                .map(|status| status.borrow().ready_generation)
                .unwrap_or_default();
            return (
                ServiceEvidence::Unreadable {
                    phase: chimera_core::actor_v2::service_policy::ServicePhase::Unknown,
                },
                generation,
            );
        };
        // Legacy-only tests keep their injected probe/watch and restart-policy
        // semantics. No second read is performed against a production host.
        let runtime_owned = self.service.runtime_owned(&info);
        let previous = self
            .service_status
            .as_ref()
            .map(|status| status.borrow().clone())
            .unwrap_or_else(ServiceHostStatus::probing);
        let status =
            previous.observe_probe(info, runtime_owned, Some(self.service.restart_policy()));
        let generation = status.ready_generation;
        (ServiceEvidence::from_status(&status), generation)
    }
}

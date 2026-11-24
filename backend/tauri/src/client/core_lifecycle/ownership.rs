//! Conditional Core retirement and selected-host ownership operations.
use crate::client::application_workflow::startup::{
    ServiceEvidence, StartupOwnerFailure, startup_stop_error_is_idempotent,
};
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::{
    core_lifecycle::ports::CoreStatusSnapshot, reconciler::decision::same_core_status,
};
use chimera_ipc::api::status::CoreState;
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
        let before_stop = self.core.status().await.map_err(|error| {
            StartupOwnerFailure::WaitingDependency(format!(
                "could not refresh the Local Core owner: {error:#}"
            ))
        })?;
        if !same_core_status(current, &before_stop)
            || !matches!(before_stop.state, CoreState::Running)
        {
            return Err(StartupOwnerFailure::WaitingDependency(
                "the Local Core owner changed before it could be stopped".into(),
            ));
        }
        let stop_result = self.core.stop().await;
        if self.core.outcome_uncertain() {
            return Err(StartupOwnerFailure::RecoveryRequired(
                "the Local Core stop outcome is uncertain".into(),
            ));
        }
        let stopped = match self.core.status().await {
            Ok(stopped) => stopped,
            Err(error) => {
                return Err(match stop_result {
                    Err(stop_error) => StartupOwnerFailure::stop_rejected(
                        stop_error,
                        self.core.outcome_uncertain(),
                    ),
                    Ok(()) => StartupOwnerFailure::WaitingDependency(format!(
                        "could not confirm the Local Core stop: {error:#}"
                    )),
                });
            }
        };
        let stop_confirmed = stopped.run_type == current.run_type
            && stopped.state_changed_at > 0
            && matches!(&stopped.state, CoreState::Stopped(_));
        if stop_confirmed {
            if let Err(error) = stop_result
                && !startup_stop_error_is_idempotent(&error)
            {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
        } else {
            if let Err(error) = stop_result {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
            return Err(StartupOwnerFailure::WaitingDependency(format!(
                "the Local Core did not publish a confirmed stopped state: {stopped:?}"
            )));
        }
        self.confirm_local_stopped_baseline(&stopped)
            .await
            .map_err(StartupOwnerFailure::WaitingDependency)
    }

    pub(in crate::client) async fn confirm_local_stopped_baseline(
        &self,
        stopped: &CoreStatusSnapshot,
    ) -> Result<(), String> {
        if stopped.state_changed_at == 0 {
            return Ok(());
        }
        let baseline = self
            .core
            .observe_runtime_baseline()
            .await
            .map_err(|error| {
                format!("the Local Core stop lacks authoritative state evidence: {error:#}")
            })?;
        if !matches!(
            baseline,
            crate::client::application_workflow::mutation::KnownRuntimeState::Stopped
        ) {
            return Err(format!(
                "the Local Core runtime is not proven stopped after retirement: {baseline:?}"
            ));
        }
        let confirmed = self
            .core
            .status()
            .await
            .map_err(|error| format!("could not recheck the stopped Local Core: {error:#}"))?;
        if !same_core_status(stopped, &confirmed)
            || !matches!(&confirmed.state, CoreState::Stopped(_))
        {
            return Err("the Local Core state changed after its stop was confirmed".into());
        }
        Ok(())
    }

    pub(in crate::client) async fn recover_service_error_stop(
        &self,
        service: ServiceEvidence,
        previous_generation: u64,
    ) -> Result<(ServiceEvidence, u64), StartupOwnerFailure> {
        if !matches!(
            &service,
            ServiceEvidence::CoreStoppedWithReason {
                runtime_owned: true,
                ..
            }
        ) {
            return Ok((service, previous_generation));
        }

        self.core
            .recover_service_quarantine()
            .await
            .map_err(|error| {
                StartupOwnerFailure::stop_rejected(error, self.core.outcome_uncertain())
            })?;

        let (service, fresh_generation) = self.service_evidence().await;
        if fresh_generation != previous_generation || !service.proves_runtime_owner() {
            return Err(StartupOwnerFailure::RecoveryRequired(
                "the Service runtime identity changed during quarantine recovery; startup will not hand off another host".into(),
            ));
        }
        let service = match service {
            ServiceEvidence::CoreStoppedWithReason {
                ready,
                phase,
                runtime_owned: true,
                ..
            } => {
                // Recover returns only after the manager has cleared its
                // quarantined epochs; some harmless stop reasons remain in
                // the status snapshot when there was no quarantine.
                ServiceEvidence::CoreStopped {
                    ready,
                    phase,
                    runtime_owned: true,
                }
            }
            service => service,
        };
        Ok((service, fresh_generation))
    }

    pub(in crate::client) async fn retire_service_startup_owner(
        &self,
        service: &ServiceEvidence,
        require_ready_stopped: bool,
    ) -> Result<ServiceEvidence, StartupOwnerFailure> {
        let expected_owner = service.expected_owner().map_err(|reason| {
            StartupOwnerFailure::RecoveryRequired(format!(
                "the Service Core owner cannot be fenced for retirement: {reason}"
            ))
        })?;
        let stop_result = self.core.retire_service_core(expected_owner).await;
        if self.core.outcome_uncertain() {
            return Err(StartupOwnerFailure::RecoveryRequired(
                "the Service Core stop outcome is uncertain".into(),
            ));
        }
        let (after_stop, _) = self.service_evidence().await;
        let confirmed = if require_ready_stopped {
            matches!(
                &after_stop,
                ServiceEvidence::CoreStopped { ready: true, .. }
            )
        } else {
            after_stop.proves_no_owned_core()
        };
        if confirmed {
            if let Err(error) = stop_result
                && !startup_stop_error_is_idempotent(&error)
            {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
            return Ok(after_stop);
        }
        if matches!(&after_stop, ServiceEvidence::Unreadable { .. }) {
            if let Err(error) = stop_result {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
            return Err(StartupOwnerFailure::WaitingDependency(format!(
                "the Service Core stop could not be confirmed by a fresh probe: {after_stop:?}"
            )));
        }
        if let Err(error) = stop_result {
            return Err(StartupOwnerFailure::stop_rejected(
                error,
                self.core.outcome_uncertain(),
            ));
        }
        Err(StartupOwnerFailure::WaitingDependency(format!(
            "the Service Core stop was accepted but a fresh probe did not confirm release: {after_stop:?}"
        )))
    }
}

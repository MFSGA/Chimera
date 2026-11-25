//! Ref startup convergence: stable S2/S3/S4 observations and execution.
//! The existing CoreLifecycle mailbox remains the sole mutation owner.
use std::sync::Arc;

use chimera_contracts::{execution_host::ExecutionHost, runtime_error::RuntimeError};
use chimera_core::{
    actor_v2::CoreStatusProjection,
    client::reconciler::plan::{Conclusion, ServiceEvidence, Trigger},
};
use chimera_core_manager::OperationId;
use chimera_ipc::api::status::CoreState;

use crate::client::{
    convergence::ConvergenceHealth,
    core_lifecycle::{CoreLifecycleWorkflow, ports::CoreStatusSnapshot},
    reconciler::{attempt::CommittedTargetAdvanced, decision::same_core_status},
};

pub use chimera_core::client::reconciler::startup::*;

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn planned_committed_round(
        &self,
        target: &str,
        host: ExecutionHost,
        trigger: Trigger,
    ) -> anyhow::Result<CommittedRound> {
        chimera_core::client::reconciler::committed_round::planned_committed_round(
            self, target, host, trigger,
        )
        .await
    }
}

use chimera_config::application::ChimeraAppConfig;

impl CoreLifecycleWorkflow {
    /// Startup S2/S3 established and, if necessary, retired the Local Core
    /// before reaching here. Reuse the exact committed-source preparation
    /// instead of the old inline YAML construction whenever the sole Core
    /// owner and authoritative source reader are available.
    pub(in crate::client) async fn try_startup_prepared_selected(
        &self,
        app: &ChimeraAppConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        if self.profiles.is_none()
            || self.checked_gate.get().is_none()
            || self.source_authority.is_none()
        {
            return None;
        }
        let clash = match self.clash.get() {
            Ok(clash) => clash,
            Err(error) => return Some(Err(error)),
        };
        let (target, app, clash, ..) = match self
            .capture_reconcile_committed_target(app.clone(), clash)
            .await
        {
            Ok(committed) => committed,
            Err(error) => return Some(Err(error)),
        };
        match run_type {
            crate::core::RunType::Normal => {
                self.try_reconcile_prepared_local(&target, &app, &clash, run_type)
                    .await
            }
            crate::core::RunType::Service => {
                self.try_reconcile_prepared_service(&target, &app, &clash)
                    .await
            }
            crate::core::RunType::Elevated => None,
        }
    }

    /// Recovery after S2 proved the selected owner. Reuse the normal ref
    /// committed S3/S4 planner, without a new lease or host selection.
    /// None is reserved for the pre-canonical compatibility path.
    pub(in crate::client) async fn recover_committed_startup(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> Option<anyhow::Result<()>> {
        if self.profiles.is_none()
            || self.checked_gate.get().is_none()
            || self.source_authority.is_none()
            || observed.run_type == crate::core::RunType::Elevated
            || observed.state_changed_at == 0
        {
            return None;
        }
        let host = crate::client::reconciler::proof::host_for(observed.run_type);
        let result = async {
            let clash = self.clash.get()?;
            let (target, app, clash, ..) = self
                .capture_reconcile_committed_target(self.application.get_typed(), clash)
                .await?;
            if !same_core_status(observed, &self.core.status().await?) {
                return Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the S2-proven owner changed before startup convergence",
                    )
                    .into(),
                );
            }
            let result = match self
                .planned_committed_round(&target, host, Trigger::Startup)
                .await?
            {
                CommittedRound::Settled(result) => result,
                CommittedRound::Apply => {
                    self.progress
                        .lock()
                        .allow(&target, host, Trigger::Startup)?;
                    self.progress.lock().stage(target.clone(), host);
                    self.try_reconcile_prepared_selected(&target, &app, &clash, observed.run_type)
                        .await
                        .unwrap_or_else(|| {
                            Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "the S4-admitted startup candidate lost its owner or source proof",
                        ).into()
                    )
                        })
                }
                CommittedRound::LegacyRecovery => Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "canonical startup recovery has no settled S3/S4 plan",
                    )
                    .into(),
                ),
            };
            if self.startup_target_identity().await? != target {
                return Err(CommittedTargetAdvanced.into());
            }
            result
        }
        .await;
        if result.is_err() {
            self.progress.lock().invalidate_selection();
        }
        self.refresh_service_recovery_host().await;
        Some(result)
    }

    pub(in crate::client) async fn startup_reconcile_application(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        let selected = if service_connected {
            crate::core::RunType::Service
        } else {
            crate::core::RunType::Normal
        };
        let result = if let Some(result) = self.try_startup_prepared_selected(&app, selected).await
        {
            result
        } else if self.canonical_convergence_session() {
            Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "startup lost the prepared candidate or selected owner proof",
                )
                .into(),
            )
        } else {
            #[cfg(test)]
            {
                self.reconcile_application(app, service_connected).await
            }
            #[cfg(not(test))]
            {
                Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "startup requires the installed Reconciler preparation session",
                    )
                    .into(),
                )
            }
        };
        self.refresh_service_recovery_host().await;
        result
    }

    pub(in crate::client) async fn startup_reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
        expected_target: Option<&str>,
    ) -> anyhow::Result<()> {
        let result = self
            .reconcile_application_after_service_release(app, expected_target)
            .await;
        self.refresh_service_recovery_host().await;
        result
    }

    /// Upstream Step::FallBackToLocal, adapted to Chimera's existing host
    /// transition owner. A rejected Service target may fall back only after a
    /// stable Stopped observation and an instance-bound Service release. The
    /// Local apply re-captures committed inputs rather than replaying the old
    /// pre-commit candidate. No second host controller is created here.
    pub(in crate::client) async fn fallback_after_service_refusal(
        &self,
        target: &str,
        refusal: anyhow::Error,
    ) -> anyhow::Result<()> {
        chimera_core::client::reconciler::convergence_executor::fallback_after_service_refusal(
            self, target, refusal,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_start_and_core_change_join_the_canonical_convergence_round() {
        for trigger in [Trigger::Start, Trigger::CoreChanged, Trigger::Startup] {
            assert!(uses_committed_convergence(trigger, true));
            assert!(!uses_committed_convergence(trigger, false));
        }
        for trigger in [Trigger::SourceCommitted, Trigger::Retry] {
            assert!(uses_committed_convergence(trigger, true));
            assert!(uses_committed_convergence(trigger, false));
        }
        for trigger in [
            Trigger::Stop,
            Trigger::ServiceOperation,
            Trigger::PrepareService,
        ] {
            assert!(!uses_committed_convergence(trigger, true));
        }
    }

    #[test]
    fn ref_startup_projection_distinguishes_all_reconciler_conclusions() {
        assert!(matches!(
            StartupOutcome::from_conclusion(&Conclusion::Converged),
            StartupOutcome::Ready
        ));
        assert!(matches!(
            StartupOutcome::from_conclusion(&Conclusion::Inactive),
            StartupOutcome::Ready
        ));
        let waiting = Arc::new(RuntimeError::new("service is not ready"));
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Waiting {cause: waiting}),
            StartupOutcome::ReadyDegraded {health: ConvergenceHealth::WaitingDependency, reason} if reason.contains("service is not ready"))
        );
        let refused = Arc::new(RuntimeError::CoreRejectedConfig {
            core_kind: None,
            message: "candidate invalid".into(),
        });
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::NotInEffect {cause: refused}),
            StartupOutcome::ReadyDegraded {health: ConvergenceHealth::Blocked, reason} if reason.contains("candidate invalid"))
        );
        let unknown = Arc::new(RuntimeError::new("result could not be observed"));
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Unknown {cause: unknown}),
            StartupOutcome::RecoveryRequired {reason} if reason.contains("result could not be observed"))
        );
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Closed),
            StartupOutcome::RecoveryRequired {reason} if reason == RuntimeError::ShuttingDown.to_string())
        );
    }

    #[test]
    fn ref_startup_cause_keeps_the_exact_typed_reason() {
        let cause = cause("service owner not established".into());
        assert!(matches!(
            cause.as_ref(),
            RuntimeError::ConfigurationNotInEffect { .. }
        ));
        assert!(cause.to_string().contains("service owner not established"));
    }
}

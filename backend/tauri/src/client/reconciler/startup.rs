//! Ref startup convergence: stable S2/S3/S4 observations and execution.
//! The existing CoreLifecycle mailbox remains the sole mutation owner.

use chimera_contracts::execution_host::ExecutionHost;
use chimera_core::client::reconciler::plan::Trigger;

use crate::client::core_lifecycle::{CoreLifecycleWorkflow, ports::CoreStatusSnapshot};

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
    pub(in crate::client) async fn try_startup_prepared_selected(
        &self,
        app: &ChimeraAppConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        chimera_core::client::reconciler::startup_convergence::try_startup_prepared_selected(
            self, app, run_type,
        )
        .await
    }

    pub(in crate::client) async fn recover_committed_startup(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> Option<anyhow::Result<()>> {
        chimera_core::client::reconciler::startup_convergence::recover_committed_startup(
            self, observed,
        )
        .await
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

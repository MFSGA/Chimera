//! StartupReconcile and owner reestablishment orchestration for ApplicationWorkflow.
//!
//! CoreLifecycleWorkflow supplies typed Core/Service adapter operations.
//! Startup evidence and owner decisions live here so setup cannot silently use
//! ordinary reconciliation when another host may still own a core.

use super::workflow::ApplicationWorkflow;
#[cfg(test)]
use crate::client::convergence::ConvergenceHealth;
use crate::client::core_lifecycle::CoreLifecycleWorkflow;

#[cfg(test)]
pub use chimera_core::client::reconciler::startup_policy::StartupRetryTarget;
pub use chimera_core::client::reconciler::startup_policy::{
    ServiceEvidence, StartupOutcome, StartupReport, StartupWorkflowState,
    reconciler_service_evidence, startup_failure_is_retryable,
    startup_failure_waits_for_dependency,
};

#[async_trait::async_trait]
impl chimera_core::client::reconciler::startup_transaction::StartupTransactionHost
    for CoreLifecycleWorkflow
{
    async fn unresolved_startup_target_identity(&self) -> Option<String> {
        self.unresolved_startup_target_identity().await
    }
}

impl ApplicationWorkflow {
    #[cfg(test)]
    pub(in crate::client) async fn startup_reconcile(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn crate::client::core_lifecycle::ServiceTransitionLease>,
    ) -> StartupReport {
        let lifecycle = self.lifecycle_handle();
        self.progress
            .startup_reconcile(operation_id, lifecycle.as_ref())
            .await
    }
    #[cfg(test)]
    pub(in crate::client) async fn recover_startup(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn crate::client::core_lifecycle::ServiceTransitionLease>,
    ) -> StartupReport {
        let lifecycle = self.lifecycle_handle();
        self.progress
            .recover_startup(operation_id, lifecycle.as_ref())
            .await
    }
    #[cfg(test)]
    pub(in crate::client) async fn retry_startup(
        &mut self,
        operation_id: u64,
        identity: String,
        _transition: Box<dyn crate::client::core_lifecycle::ServiceTransitionLease>,
    ) -> StartupReport {
        let lifecycle = self.lifecycle_handle();
        self.progress
            .retry_startup(operation_id, identity, lifecycle.as_ref())
            .await
    }

    pub(in crate::client) async fn service_host_change_due(&self) -> bool {
        self.progress
            .service_host_change_due(self.lifecycle())
            .await
    }
}

#[cfg(test)]
pub(in crate::client) async fn update_startup_retry_target(
    lifecycle: &CoreLifecycleWorkflow,
    mut report: StartupReport,
    automatic: bool,
    allow_receipt_reuse: bool,
    retry_target: &mut Option<StartupRetryTarget>,
) -> StartupReport {
    match &report.outcome {
        StartupOutcome::Ready => {
            *retry_target = None;
            return report;
        }
        StartupOutcome::RecoveryRequired { .. } | StartupOutcome::Unsettled { .. } => {
            if let Some(target) = retry_target.as_mut() {
                target.health = ConvergenceHealth::RecoveryRequired;
                target.next_attempt = None;
            }
            return report;
        }
        StartupOutcome::ReadyDegraded { .. } => {}
    }

    let identity = match lifecycle.startup_target_identity().await {
        Ok(identity) => identity,
        Err(error) => {
            let reason = format!(
                "{}; committed runtime target identity is unavailable: {error:#}",
                report
                    .outcome
                    .failure_reason()
                    .unwrap_or("startup did not converge")
            );
            if let Some(identity) = lifecycle.unresolved_startup_target_identity().await {
                if retry_target
                    .as_ref()
                    .is_none_or(|target| target.identity != identity)
                {
                    *retry_target = Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
                }
                if let Some(target) = retry_target.as_mut() {
                    report.outcome = target.schedule_dependency(reason);
                    return report;
                }
            }
            report.outcome = StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason,
            };
            return report;
        }
    };
    if retry_target
        .as_ref()
        .is_none_or(|target| target.identity != identity)
    {
        *retry_target = Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
    }
    let target = retry_target
        .as_mut()
        .expect("startup retry target was initialized");
    target.allow_receipt_reuse = allow_receipt_reuse;
    let reason = report
        .outcome
        .failure_reason()
        .unwrap_or("startup did not converge")
        .to_owned();
    report.outcome = match report.outcome {
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::WaitingDependency | ConvergenceHealth::RecoveryRequired,
            ..
        } => target.schedule_dependency(reason),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::RetryScheduled,
            ..
        } => target.schedule_application(reason, automatic),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::Blocked,
            ..
        }
        | StartupOutcome::ReadyDegraded { .. } => target.block(reason),
        StartupOutcome::Ready
        | StartupOutcome::RecoveryRequired { .. }
        | StartupOutcome::Unsettled { .. } => {
            unreachable!("handled above")
        }
    };
    report
}

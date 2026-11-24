//! StartupReconcile and owner reestablishment orchestration for ApplicationWorkflow.
//!
//! CoreLifecycleWorkflow supplies typed Core/Service adapter operations.
//! Startup evidence and owner decisions live here so setup cannot silently use
//! ordinary reconciliation when another host may still own a core.

use super::workflow::ApplicationWorkflow;
use crate::client::{
    application_workflow::{
        attempt::AttemptStage,
        mutation::{DeferredTarget, ReestablishCause, TargetOrigin},
    },
    convergence::ConvergenceHealth,
    core_lifecycle::{CoreLifecycleWorkflow, ServiceTransitionLease},
};

#[cfg(test)]
pub use chimera_core::client::reconciler::startup_policy::StartupRetryTarget;
pub use chimera_core::client::reconciler::startup_policy::{
    ServiceEvidence, StartupOutcome, StartupReport, StartupWorkflowState,
    reconciler_service_evidence, startup_failure_is_retryable,
    startup_failure_waits_for_dependency,
};

impl ApplicationWorkflow {
    async fn startup_reconcile_once(
        &mut self,
        operation_id: u64,
        allow_receipt_reuse: bool,
    ) -> StartupReport {
        let lifecycle = self.lifecycle_handle();
        chimera_core::client::reconciler::startup_executor::StartupExecutor::new(
            lifecycle.as_ref(),
            &self.progress.startup,
        )
        .run(operation_id, allow_receipt_reuse)
        .await
    }

    fn begin_reestablish_attempt(&mut self, cause: ReestablishCause) {
        chimera_core::client::reconciler::reestablish::begin_reestablish_attempt(
            &mut self.progress.live,
            &mut self.progress.deferred,
            cause,
        );
    }

    async fn update_reestablish_attempt(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        report: StartupReport,
        automatic: bool,
        cause: ReestablishCause,
    ) -> StartupReport {
        let report = chimera_core::client::reconciler::reestablish::update_reestablish_attempt(
            &mut self.progress.live,
            &mut self.progress.deferred,
            || lifecycle.startup_target_identity(),
            || lifecycle.unresolved_startup_target_identity(),
            || lifecycle.outcome_uncertain(),
            report,
            automatic,
            cause,
        )
        .await;
        if matches!(
            &report.outcome,
            StartupOutcome::RecoveryRequired { .. } | StartupOutcome::Unsettled { .. }
        ) {
            self.progress.publish_recovery_context().await;
        }
        report
    }

    pub(in crate::client) async fn startup_reconcile(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        if let Some(report) = &self.progress.startup_report {
            return report.clone();
        }
        self.progress.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Startup);
        if let Some(attempt) = &mut self.progress.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Startup)
            .await;
        self.progress.startup_report = Some(report.clone());
        report
    }

    pub(in crate::client) async fn recover_startup(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        self.progress.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Recovery);
        if let Some(attempt) = &mut self.progress.live {
            attempt.stage = AttemptStage::Recovering;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, true).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Recovery)
            .await;
        if self.progress.startup_report.is_none() {
            self.progress.startup_report = Some(report.clone());
        }
        report
    }

    pub(in crate::client) async fn retry_startup(
        &mut self,
        operation_id: u64,
        identity: String,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        let target = self.progress.deferred.as_ref().filter(|target| {
            target.is_reestablish()
                && target.identity == identity
                && target.health == ConvergenceHealth::Pending
        });
        let Some(target) = target else {
            return StartupReport {
                operation_id,
                observation: None,
                outcome: StartupOutcome::Ready,
            };
        };
        let TargetOrigin::Reestablish(cause) = target.origin else {
            unreachable!("filtered reestablish target")
        };
        let allow_receipt_reuse = target.allows_receipt_reuse();

        self.progress.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(cause);
        if let Some(attempt) = &mut self.progress.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self
            .startup_reconcile_once(operation_id, allow_receipt_reuse)
            .await;
        self.update_reestablish_attempt(&lifecycle, report, true, cause)
            .await
    }

    pub(in crate::client) async fn service_host_change_due(&self) -> bool {
        if self.progress.startup_report.is_none()
            || self.progress.live.is_some()
            || self.progress.startup.owner_unproven()
        {
            return false;
        }
        if self.progress.deferred.as_ref().is_some_and(|target| {
            target.is_reestablish()
                && (matches!(
                    target.health,
                    crate::client::convergence::ConvergenceHealth::Blocked
                        | crate::client::convergence::ConvergenceHealth::RecoveryRequired
                ) || target
                    .next_attempt
                    .is_some_and(|next_attempt| next_attempt > tokio::time::Instant::now()))
        }) {
            return false;
        }
        chimera_core::client::reconciler::startup_executor::StartupExecutor::new(
            self.lifecycle(),
            &self.progress.startup,
        )
        .service_host_change_due()
        .await
    }

    pub(in crate::client) async fn follow_service(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> anyhow::Result<()> {
        if !self.service_host_change_due().await {
            return Ok(());
        }
        self.progress.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::ServiceReadiness);
        if let Some(attempt) = &mut self.progress.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(
                &lifecycle,
                report,
                false,
                ReestablishCause::ServiceReadiness,
            )
            .await;
        super::workflow::startup_report_result(&report)
    }

    pub(super) async fn refresh_startup_retry_target_after_reconcile(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        result: &anyhow::Result<()>,
    ) {
        let Some(target) = self
            .progress
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if result.is_ok() {
            self.progress.clear_startup_retry_target();
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };

        let error = result
            .as_ref()
            .expect_err("failed reconciliation has an error");
        let outcome = if lifecycle.outcome_uncertain() {
            StartupOutcome::RecoveryRequired {
                reason: format!("runtime target reconciliation is uncertain: {error:#}"),
            }
        } else if startup_failure_waits_for_dependency(error) {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::WaitingDependency,
                reason: format!(
                    "runtime target reconciliation is waiting for a dependency: {error:#}"
                ),
            }
        } else if startup_failure_is_retryable(error) {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::RetryScheduled,
                reason: format!("runtime target reconciliation failed transiently: {error:#}"),
            }
        } else {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason: format!("runtime target reconciliation failed: {error:#}"),
            }
        };
        let report = StartupReport {
            operation_id: 0,
            observation: None,
            outcome,
        };
        update_reestablish_target(lifecycle, report, false, cause, &mut self.progress.deferred)
            .await;
    }

    pub(super) async fn reschedule_startup_retry_after_committed_config_patch(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
    ) {
        chimera_core::client::reconciler::reestablish::reschedule_after_source_commit(
            &mut self.progress.deferred,
            || lifecycle.startup_target_identity(),
            || lifecycle.unresolved_startup_target_identity(),
        )
        .await;
    }
}

async fn update_reestablish_target(
    lifecycle: &CoreLifecycleWorkflow,
    report: StartupReport,
    automatic: bool,
    cause: ReestablishCause,
    deferred: &mut Option<DeferredTarget>,
) -> StartupReport {
    chimera_core::client::reconciler::reestablish::update_reestablish_target(
        || lifecycle.startup_target_identity(),
        || lifecycle.unresolved_startup_target_identity(),
        report,
        automatic,
        cause,
        deferred,
    )
    .await
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

//! Startup/reestablishment transaction orchestration within the shared owner.
use super::{
    reestablish::{AttemptStage, DeferredTarget, ReestablishCause, TargetOrigin},
    startup_executor::StartupHost,
    startup_policy::{
        StartupOutcome, StartupReport, startup_failure_is_retryable,
        startup_failure_waits_for_dependency,
    },
    workflow_progress::WorkflowProgress,
};
use crate::client::{convergence::ConvergenceHealth, core_lifecycle::ports::CoreLifecyclePort};

#[async_trait::async_trait]
pub trait StartupTransactionHost: StartupHost {
    async fn unresolved_startup_target_identity(&self) -> Option<String>;
}

pub fn startup_report_result(report: &StartupReport) -> anyhow::Result<()> {
    match report.outcome.failure_reason() {
        Some(reason) => Err(anyhow::anyhow!(reason.to_owned())),
        None => Ok(()),
    }
}

impl WorkflowProgress {
    async fn startup_reconcile_once(
        &mut self,
        lifecycle: &impl StartupTransactionHost,
        operation_id: u64,
        allow_receipt_reuse: bool,
    ) -> StartupReport {
        super::startup_executor::StartupExecutor::new(lifecycle, &self.startup)
            .run(operation_id, allow_receipt_reuse)
            .await
    }

    fn begin_reestablish_attempt(&mut self, cause: ReestablishCause) {
        super::reestablish::begin_reestablish_attempt(&mut self.live, &mut self.deferred, cause);
    }

    async fn update_reestablish_attempt(
        &mut self,
        lifecycle: &impl StartupTransactionHost,
        report: StartupReport,
        automatic: bool,
        cause: ReestablishCause,
    ) -> StartupReport {
        let report = super::reestablish::update_reestablish_attempt(
            &mut self.live,
            &mut self.deferred,
            || lifecycle.startup_target_identity(),
            || lifecycle.unresolved_startup_target_identity(),
            || lifecycle.core().outcome_uncertain(),
            report,
            automatic,
            cause,
        )
        .await;
        if matches!(
            &report.outcome,
            StartupOutcome::RecoveryRequired { .. } | StartupOutcome::Unsettled { .. }
        ) {
            self.publish_recovery_context().await;
        }
        report
    }

    pub async fn startup_reconcile(
        &mut self,
        operation_id: u64,
        lifecycle: &impl StartupTransactionHost,
    ) -> StartupReport {
        if let Some(report) = &self.startup_report {
            return report.clone();
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Startup);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let report = self
            .startup_reconcile_once(lifecycle, operation_id, false)
            .await;
        let report = self
            .update_reestablish_attempt(lifecycle, report, false, ReestablishCause::Startup)
            .await;
        self.startup_report = Some(report.clone());
        report
    }

    pub async fn recover_startup(
        &mut self,
        operation_id: u64,
        lifecycle: &impl StartupTransactionHost,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Recovery);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::Recovering;
        }
        let report = self
            .startup_reconcile_once(lifecycle, operation_id, true)
            .await;
        let report = self
            .update_reestablish_attempt(lifecycle, report, false, ReestablishCause::Recovery)
            .await;
        if self.startup_report.is_none() {
            self.startup_report = Some(report.clone());
        }
        report
    }

    pub async fn retry_startup(
        &mut self,
        operation_id: u64,
        identity: String,
        lifecycle: &impl StartupTransactionHost,
    ) -> StartupReport {
        let target = self.deferred.as_ref().filter(|target| {
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

        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(cause);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let report = self
            .startup_reconcile_once(lifecycle, operation_id, allow_receipt_reuse)
            .await;
        self.update_reestablish_attempt(lifecycle, report, true, cause)
            .await
    }

    pub async fn service_host_change_due(&self, lifecycle: &impl StartupTransactionHost) -> bool {
        if self.startup_report.is_none() || self.live.is_some() || self.startup.owner_unproven() {
            return false;
        }
        if self.deferred.as_ref().is_some_and(|target| {
            target.is_reestablish()
                && (matches!(
                    target.health,
                    ConvergenceHealth::Blocked | ConvergenceHealth::RecoveryRequired
                ) || target
                    .next_attempt
                    .is_some_and(|next_attempt| next_attempt > tokio::time::Instant::now()))
        }) {
            return false;
        }
        super::startup_executor::StartupExecutor::new(lifecycle, &self.startup)
            .service_host_change_due()
            .await
    }

    pub async fn follow_service(
        &mut self,
        operation_id: u64,
        lifecycle: &impl StartupTransactionHost,
    ) -> anyhow::Result<()> {
        if !self.service_host_change_due(lifecycle).await {
            return Ok(());
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::ServiceReadiness);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let report = self
            .startup_reconcile_once(lifecycle, operation_id, false)
            .await;
        let report = self
            .update_reestablish_attempt(
                lifecycle,
                report,
                false,
                ReestablishCause::ServiceReadiness,
            )
            .await;
        startup_report_result(&report)
    }

    pub async fn refresh_startup_retry_target_after_reconcile(
        &mut self,
        lifecycle: &impl StartupTransactionHost,
        result: &anyhow::Result<()>,
    ) {
        let Some(target) = self
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if result.is_ok() {
            self.clear_startup_retry_target();
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };

        let error = result
            .as_ref()
            .expect_err("failed reconciliation has an error");
        let outcome = if lifecycle.core().outcome_uncertain() {
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
        update_reestablish_target(lifecycle, report, false, cause, &mut self.deferred).await;
    }

    pub async fn reschedule_startup_retry_after_committed_config_patch(
        &mut self,
        lifecycle: &impl StartupTransactionHost,
    ) {
        super::reestablish::reschedule_after_source_commit(
            &mut self.deferred,
            || lifecycle.startup_target_identity(),
            || lifecycle.unresolved_startup_target_identity(),
        )
        .await;
    }
}

async fn update_reestablish_target(
    lifecycle: &impl StartupTransactionHost,
    report: StartupReport,
    automatic: bool,
    cause: ReestablishCause,
    deferred: &mut Option<DeferredTarget>,
) -> StartupReport {
    super::reestablish::update_reestablish_target(
        || lifecycle.startup_target_identity(),
        || lifecycle.unresolved_startup_target_identity(),
        report,
        automatic,
        cause,
        deferred,
    )
    .await
}

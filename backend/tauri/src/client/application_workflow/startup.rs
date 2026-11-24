//! StartupReconcile and owner reestablishment orchestration for ApplicationWorkflow.
//!
//! CoreLifecycleWorkflow supplies typed Core/Service adapter operations.
//! Startup evidence and owner decisions live here so setup cannot silently use
//! ordinary reconciliation when another host may still own a core.

use super::workflow::ApplicationWorkflow;
use crate::client::{
    application_workflow::{
        attempt::{AttemptOrigin, AttemptStage, LiveAttempt},
        mutation::{DeferredTarget, ReestablishCause, TargetOrigin},
    },
    convergence::ConvergenceHealth,
    core_lifecycle::{CoreLifecycleWorkflow, ServiceTransitionLease},
};
use chimera_core_manager::OperationId;

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
            &self.startup,
        )
        .run(operation_id, allow_receipt_reuse)
        .await
    }

    fn begin_reestablish_attempt(&mut self, cause: ReestablishCause) {
        if let Some(attempt) = &mut self.live {
            if let Some(target) = attempt.target_mut() {
                target.origin = TargetOrigin::Reestablish(cause);
                attempt.stage = if cause == ReestablishCause::Recovery {
                    AttemptStage::Recovering
                } else {
                    AttemptStage::Preparing
                };
                attempt.unresolved = None;
                return;
            }
            debug_assert!(false, "an unresolved attempt is never replaced");
            return;
        }

        let mut target = self.deferred.take().unwrap_or_else(|| {
            DeferredTarget::reestablish(OperationId::generate(), String::new(), cause)
        });
        target.origin = TargetOrigin::Reestablish(cause);
        let operation_id = target.operation_id;
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
    }

    async fn update_reestablish_attempt(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        report: StartupReport,
        automatic: bool,
        cause: ReestablishCause,
    ) -> StartupReport {
        let target = self
            .live
            .as_ref()
            .and_then(|attempt| match &attempt.origin {
                AttemptOrigin::CommittedTarget(target) => Some(target.as_ref().clone()),
                AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => None,
            })
            .expect("startup reestablishment runs on its committed target");
        let mut deferred = Some(target);
        let report =
            update_reestablish_target(lifecycle, report, automatic, cause, &mut deferred).await;
        if let Some(target) = deferred
            && let Some(live_target) = self.live.as_mut().and_then(LiveAttempt::target_mut)
        {
            *live_target = target;
        }

        match &report.outcome {
            StartupOutcome::Ready => {
                self.live = None;
                self.deferred = None;
            }
            StartupOutcome::RecoveryRequired { reason } | StartupOutcome::Unsettled { reason } => {
                if let Some(attempt) = &mut self.live {
                    attempt.unresolved = Some(reason.clone());
                }
                self.deferred = None;
                self.publish_recovery_context().await;
            }
            StartupOutcome::ReadyDegraded { .. } => {
                self.conclude_attempt(None);
            }
        }
        report
    }

    pub(in crate::client) async fn startup_reconcile(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        if let Some(report) = &self.startup_report {
            return report.clone();
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Startup);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Startup)
            .await;
        self.startup_report = Some(report.clone());
        report
    }

    pub(in crate::client) async fn recover_startup(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Recovery);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::Recovering;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, true).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Recovery)
            .await;
        if self.startup_report.is_none() {
            self.startup_report = Some(report.clone());
        }
        report
    }

    pub(in crate::client) async fn retry_startup(
        &mut self,
        operation_id: u64,
        identity: String,
        _transition: Box<dyn ServiceTransitionLease>,
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
        let lifecycle = self.lifecycle_handle();
        let report = self
            .startup_reconcile_once(operation_id, allow_receipt_reuse)
            .await;
        self.update_reestablish_attempt(&lifecycle, report, true, cause)
            .await
    }

    pub(in crate::client) async fn service_host_change_due(&self) -> bool {
        if self.startup_report.is_none() || self.live.is_some() || self.startup.owner_unproven() {
            return false;
        }
        if self.deferred.as_ref().is_some_and(|target| {
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
            &self.startup,
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
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::ServiceReadiness);
        if let Some(attempt) = &mut self.live {
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
        update_reestablish_target(lifecycle, report, false, cause, &mut self.deferred).await;
    }

    pub(super) async fn reschedule_startup_retry_after_committed_config_patch(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
    ) {
        let Some(target) = self
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if target.health == ConvergenceHealth::RecoveryRequired {
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };
        let identity = match lifecycle.startup_target_identity().await {
            Ok(identity) => identity,
            Err(_) => match lifecycle.unresolved_startup_target_identity().await {
                Some(identity) => identity,
                None => return,
            },
        };
        let target = self
            .deferred
            .as_mut()
            .filter(|target| target.is_reestablish())
            .expect("reestablish target remains attached to the workflow");
        if target.identity != identity {
            *target = DeferredTarget::reestablish(OperationId::generate(), identity, cause);
        } else if target.attempts_remaining == 0 {
            return;
        }
        target.health = ConvergenceHealth::RetryScheduled;
        target.next_attempt = Some(tokio::time::Instant::now());
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

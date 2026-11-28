//! Shared Completed message decisions. The sole mailbox still owns its
//! worker join, host notifications, timers and RPC replies.
use super::{
    attempt::{CommittedTargetAdvanced, OwnershipTransitionPending, ServiceFallbackDeferred},
    committed::SourceVersions,
    plan::{RunIntent, Trigger},
    round::Round,
    workflow::{RuntimeReconcilerState, committed_target_advanced, source_committed_during_round},
};

/// An obsolete Checked apply can have changed the actual Core process while
/// another Source committed. Do not publish its stale port binding as the
/// current desired Effects state; the sole Actor must first reconcile latest.
/// Service/Shutdown observations remain independent of runtime convergence.
pub fn refresh_effect_ports(
    reconciled: bool,
    service_observed: bool,
    shutdown: bool,
    source_advanced: bool,
) -> bool {
    (service_observed || shutdown) || (reconciled && !source_advanced)
}

pub fn checked_clash_notice(
    checked_clash: bool,
    result: &anyhow::Result<()>,
    verified_running: bool,
) -> Option<Result<(), String>> {
    checked_clash.then(|| {
        result
            .as_ref()
            .map_err(|error| format!("{error:#}"))
            .and_then(|_| {
                verified_running.then_some(()).ok_or_else(|| {
                    "Clash source committed, but active Core apply was not verified".into()
                })
            })
    })
}

/// One completion fence, sampled synchronously before observing the live
/// Core. A source notice can supersede even an earlier refused operation.
pub struct CompletionFence {
    pub result: anyhow::Result<()>,
    pub source_advanced: bool,
    pub final_uncertain: bool,
}

impl CompletionFence {
    pub fn observe(
        result: anyhow::Result<()>,
        trigger: Trigger,
        started: &SourceVersions,
        latest: &SourceVersions,
        previously_uncertain: bool,
        workflow_panicked: bool,
        lower_outcome_uncertain: bool,
    ) -> Self {
        let superseded = source_committed_during_round(trigger, started, latest);
        let result = if superseded && result.is_ok() {
            Err(anyhow::Error::new(CommittedTargetAdvanced)
                .context("new committed source notice arrived during the Core operation"))
        } else {
            result
        };
        let source_advanced = superseded || committed_target_advanced(&result);
        Self {
            result,
            source_advanced,
            final_uncertain: previously_uncertain || workflow_panicked || lower_outcome_uncertain,
        }
    }
}

/// The observed terminal of the same worker that owned the mailbox round.
pub struct CompletedRound {
    pub active: super::mailbox::ActiveOperation,
    pub result: anyhow::Result<()>,
    pub source_advanced: bool,
    pub round: Round,
    /// Outer None means do not publish a new binding, inner None means no
    /// verified runtime binding exists.
    pub runtime_binding: Option<Option<chimera_config::runtime::executor::ResolvedPortBindings>>,
}

/// Revoke superseded selections, join the original worker, and observe the
/// current Core before reporting a terminal. Host adapters supply only the
/// existing staged receipt and effect ports, not a second runtime owner.
#[allow(clippy::too_many_arguments)]
pub async fn observe_completed<P, H, F>(
    mailbox: &mut super::mailbox::Mailbox<P>,
    runtime: &RuntimeReconcilerState,
    host: &H,
    id: u64,
    result: anyhow::Result<()>,
    workflow_panicked: bool,
    lower_outcome_uncertain: bool,
    reconciled: bool,
    service_observed: bool,
    shutdown: bool,
    take_prepared_outcome: impl FnOnce() -> Option<super::mutation::ApplyOutcome>,
    take_effect: impl FnOnce() -> Option<super::apply::CheckedConnectionEffect>,
    effect_port_binding: impl FnOnce() -> F,
) -> Option<CompletedRound>
where
    H: super::apply_executor::ApplyHost + ?Sized,
    F: std::future::Future<
            Output = Option<chimera_config::runtime::executor::ResolvedPortBindings>,
        >,
{
    let active = mailbox
        .active
        .as_ref()
        .filter(|active| active.response.id == id)?;
    let CompletionFence {
        result,
        source_advanced,
        final_uncertain,
    } = CompletionFence::observe(
        result,
        active.trigger,
        &active.observed_sources,
        &runtime.seen_sources,
        mailbox.uncertain,
        workflow_panicked,
        lower_outcome_uncertain,
    );
    let prepared_outcome = take_prepared_outcome();
    if source_advanced && active.checked_apply {
        host.progress().lock().invalidate_selection();
    }
    let runtime_binding =
        if refresh_effect_ports(reconciled, service_observed, shutdown, source_advanced) {
            Some(effect_port_binding().await)
        } else {
            None
        };
    #[cfg(feature = "e2e")]
    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_binding_observed", "CoreLifecycle completion stage");
    let active = mailbox.join_completed(id, shutdown).await;
    #[cfg(feature = "e2e")]
    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_worker_joined", "CoreLifecycle completion stage");
    let round = Round::observe(
        host.core(),
        || host.startup_target_identity(),
        host.progress(),
        host.convergence_authority(),
        take_effect,
        &result,
        active.trigger,
        reconciled,
        active.checked_apply,
        source_advanced,
        final_uncertain,
        prepared_outcome.as_ref(),
    )
    .await;
    Some(CompletedRound {
        active,
        result,
        source_advanced,
        round,
        runtime_binding,
    })
}

/// Publish terminal evidence and settle the original response before handling
/// its retry. This preserves the observable ordering of the existing actor.
#[allow(clippy::too_many_arguments)]
pub fn settle_completed<P>(
    mailbox: &mut super::mailbox::Mailbox<P>,
    runtime: &mut RuntimeReconcilerState,
    progress: &parking_lot::Mutex<super::state::RuntimeProgress>,
    reconciler: &tokio::sync::watch::Sender<super::status::ReconcilerStatus>,
    round: Round,
    response: super::mailbox::Response,
    trigger: Trigger,
    observed_sources: &SourceVersions,
    reconciled: bool,
    shutdown: bool,
    result: anyhow::Result<()>,
) -> (bool, DeferredRetryEvidence) {
    let failed = result.is_err();
    if runtime.observe_completion_maintenance(trigger, reconciled, &result) {
        super::dispatch::publish_maintenance(runtime, reconciler);
    }
    round.settle(
        progress,
        reconciler,
        response.id,
        trigger,
        observed_sources,
        runtime.run_intent,
        reconciled,
    );
    let retry = DeferredRetryEvidence::from_result(&result);
    mailbox.finish_operation(response, shutdown, result);
    (failed, retry)
}

/// Reuse the workflow's existing deferred target after its command reply.
pub async fn settle_deferred_retry(
    progress: Option<&mut super::workflow_progress::WorkflowProgress>,
    deferred_retry: Option<String>,
    reconciled: bool,
    failed: bool,
    uncertain: bool,
    retry: DeferredRetryEvidence,
) -> Option<(String, tokio::time::Instant)> {
    let progress = progress?;
    if let Some(digest) = deferred_retry {
        let deadline = progress
            .finish_deferred_retry(
                &digest,
                !failed,
                retry.retryable,
                retry.waiting_dependency,
                uncertain,
                retry.error,
            )
            .await?;
        Some((digest, deadline))
    } else {
        if reconciled && !failed {
            progress.runtime_reconciled();
        }
        None
    }
}

/// Classify the actual terminal error before the RPC result is consumed.
pub struct DeferredRetryEvidence {
    pub error: Option<String>,
    pub retryable: bool,
    pub waiting_dependency: bool,
}

impl DeferredRetryEvidence {
    pub fn from_result(result: &anyhow::Result<()>) -> Self {
        let error = result.as_ref().err();
        Self {
            error: error.map(ToString::to_string),
            retryable: error.is_some_and(super::check_record::safe_to_retry_deferred_runtime),
            waiting_dependency: error
                .is_some_and(super::check_record::deferred_runtime_waiting_dependency),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildAction {
    None,
    RecoverStartup,
    ScheduleRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffContinuation {
    None,
    ClearMaintenance,
    MarkDirty,
}

impl RuntimeReconcilerState {
    pub fn observe_round_connection(&mut self, round: &Round) {
        if round.connection_effect_observed {
            self.connection_interruption_maintenance = round
                .connection_degradation
                .as_ref()
                .map(|degradation| degradation.message.clone());
        }
    }

    pub fn observe_host_completion(
        &mut self,
        trigger: Trigger,
        endpoint_recovery: bool,
        result: &anyhow::Result<()>,
    ) -> bool {
        if endpoint_recovery {
            self.service_recovery_maintenance = result
                .as_ref()
                .err()
                .map(|error| format!("Service endpoint recovery is pending: {error:#}"));
        }
        if trigger == Trigger::ServiceReadiness {
            self.service_handoff_maintenance = result
                .as_ref()
                .err()
                .map(|error| format!("Service owner handoff is pending: {error:#}"));
        }
        endpoint_recovery || trigger == Trigger::ServiceReadiness
    }

    /// A concrete S2 step retains the durable source continuation even if
    /// no additional source notice arrives. Release debt is independent.
    pub fn observe_completion_maintenance(
        &mut self,
        trigger: Trigger,
        reconciled: bool,
        result: &anyhow::Result<()>,
    ) -> bool {
        let needs_owner_handoff = result
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<OwnershipTransitionPending>())
            .is_some_and(|pending| pending.plan.is_some());
        if matches!(
            trigger,
            Trigger::SourceCommitted | Trigger::Retry | Trigger::Start | Trigger::CoreChanged
        ) && needs_owner_handoff
        {
            self.source_after_handoff = true;
        }

        let mut changed = false;
        if let Some(error) = result
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<ServiceFallbackDeferred>())
        {
            self.service_handoff_maintenance = Some(error.to_string());
            changed = true;
        } else if reconciled
            && result.is_ok()
            && self
                .service_handoff_maintenance
                .as_deref()
                .is_some_and(|reason| reason.starts_with("Service-to-Local fallback"))
        {
            self.service_handoff_maintenance = None;
            changed = true;
        }
        if trigger == Trigger::ServiceRelease {
            self.service_release_debt.finish(
                result
                    .as_ref()
                    .map(|_| ())
                    .map_err(|error| format!("{error:#}")),
            );
            changed = true;
        }
        changed
    }

    pub fn retry_completed_source(
        &mut self,
        retry_on_failure: bool,
        failed: bool,
        source_advanced: bool,
        uncertain: bool,
        shutting_down: bool,
    ) -> bool {
        if (retry_on_failure && failed || source_advanced) && !uncertain && !shutting_down {
            if source_advanced {
                self.source_dirty = true;
            }
            true
        } else {
            false
        }
    }

    /// Consume retained source intent only after a verified current round
    /// with a certain, re-established owner. A SourceCommitted round already
    /// loaded the latest inputs, so it must not enqueue an identical round.
    pub fn complete_handoff(
        &mut self,
        trigger: Trigger,
        reconciled: bool,
        failed: bool,
        verified_running: bool,
        uncertain: bool,
        owner_unproven: bool,
        shutting_down: bool,
    ) -> HandoffContinuation {
        if !self.source_after_handoff
            || !reconciled
            || failed
            || !verified_running
            || uncertain
            || owner_unproven
            || self.run_intent != RunIntent::Running
            || shutting_down
        {
            return HandoffContinuation::None;
        }
        self.source_after_handoff = false;
        if trigger == Trigger::SourceCommitted {
            self.service_handoff_maintenance = None;
            HandoffContinuation::ClearMaintenance
        } else {
            self.source_dirty = true;
            HandoffContinuation::MarkDirty
        }
    }
}

pub fn rebuild_after_completion(
    requested: bool,
    shutting_down: bool,
    owner_unproven: bool,
    uncertain: bool,
) -> RebuildAction {
    if !requested || shutting_down {
        RebuildAction::None
    } else if owner_unproven {
        if uncertain {
            RebuildAction::None
        } else {
            RebuildAction::RecoverStartup
        }
    } else {
        RebuildAction::ScheduleRuntime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::reconciler::committed::SourceDomain;

    #[test]
    fn newer_source_fences_success_and_refusal_without_erasing_uncertainty() {
        let started = SourceVersions::default();
        let mut latest = started.clone();
        latest.observe(SourceDomain::Profiles, 1);
        let fence = CompletionFence::observe(
            Ok(()),
            Trigger::SourceCommitted,
            &started,
            &latest,
            false,
            false,
            true,
        );
        assert!(fence.source_advanced && fence.final_uncertain);
        assert!(committed_target_advanced(&fence.result));
        let fence = CompletionFence::observe(
            Err(anyhow::anyhow!("old refusal")),
            Trigger::SourceCommitted,
            &started,
            &latest,
            false,
            false,
            false,
        );
        assert!(fence.source_advanced);
        assert_eq!(fence.result.unwrap_err().to_string(), "old refusal");
    }

    #[test]
    fn handoff_continuation_waits_for_verified_owner_and_consumes_once() {
        let mut state = RuntimeReconcilerState::default();
        state.source_after_handoff = true;
        assert_eq!(
            state.complete_handoff(Trigger::Start, true, false, false, false, false, false),
            HandoffContinuation::None
        );
        assert!(state.source_after_handoff);
        assert_eq!(
            state.complete_handoff(Trigger::Start, true, false, true, false, true, false),
            HandoffContinuation::None
        );
        assert_eq!(
            state.complete_handoff(Trigger::Start, true, false, true, false, false, false),
            HandoffContinuation::MarkDirty
        );
        assert!(state.source_dirty && !state.source_after_handoff);
        assert_eq!(
            state.complete_handoff(Trigger::Start, true, false, true, false, false, false),
            HandoffContinuation::None
        );
    }

    #[test]
    fn latest_source_round_clears_handoff_without_duplicate_retry() {
        let mut state = RuntimeReconcilerState::default();
        state.source_after_handoff = true;
        state.service_handoff_maintenance = Some("old handoff".into());
        assert_eq!(
            state.complete_handoff(
                Trigger::SourceCommitted,
                true,
                false,
                true,
                false,
                false,
                false
            ),
            HandoffContinuation::ClearMaintenance
        );
        assert!(state.service_handoff_maintenance.is_none());
        assert!(!state.source_dirty);
        assert!(!state.retry_completed_source(true, true, true, true, false));
        assert!(state.retry_completed_source(false, false, true, false, false));
        assert!(state.source_dirty);
    }
}

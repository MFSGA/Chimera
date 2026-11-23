//! Profile mutation phases coordinated by the tracked application workflow.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chimera_config::profile::Profiles;
use chimera_core::state::{AbortResourceState, StateDecision};
use chimera_core_manager::{CoreError, CoreErrorKind, OperationId};

use super::{
    attempt::{AppliedVerdict, AttemptOrigin, AttemptStage, LiveAttempt},
    impact::{ActivationIntent, RuntimeImpact, classify_profiles},
    mutation::{
        ApplyFailure, CheckRecord, DomainChange, EvidenceGap, KnownRuntimeState,
        MutationConclusion, MutationOutcomeKind, MutationReceipt, MutationRequest, MutationStage,
        RefusalCause, RetryableCause, RuntimePrepareOutcome,
    },
    policy::{
        BaselineAvailability, CommandClass, CommandPolicy, CoreRunIntent, FailureDisposition,
        TryCauseKind, TryFailureFacts, disposition, policy_for,
    },
    workflow::{ApplicationWorkflow, ProfileRuntime, RecoveryContext},
};
use crate::client::runtime::{Degradation, DegradationPhase};

pub(in crate::client) const MUTATION_DECISION_TIMEOUT: Duration = Duration::from_secs(90);

enum DecisionOutcome {
    Committed,
    Aborted,
    Unresolved(String),
}

impl ApplicationWorkflow {
    /// Runs prepare, awaits the transaction's authoritative decision, and
    /// confirms or restores the Profile runtime before releasing the workflow.
    pub(in crate::client) async fn run_mutation(
        &mut self,
        request: &mut MutationRequest,
        core: &dyn ProfileRuntime,
    ) -> (bool, Result<(), String>) {
        let operation_id = request.operation_id.clone();
        let (previous, candidate) = match &request.change {
            DomainChange::Profiles {
                previous,
                candidate,
            } => (
                previous.as_ref().map(|state| state.as_ref().clone()),
                candidate.as_ref().clone(),
            ),
        };
        let previous = previous.unwrap_or_default();
        let mut baseline = None;
        let mut attempted_runtime = false;
        let impact = classify_profiles(&previous, &candidate, &request.hints);
        let mut policy = policy_for(
            request.class,
            impact,
            &Default::default(),
            CoreRunIntent::Running,
        );
        let mut check = CheckRecord::NotOwed;

        let mut outcome = if request.decision.decision() != StateDecision::Undecided {
            RuntimePrepareOutcome::Rejected {
                cause: ApplyFailure {
                    stage: MutationStage::Preparing,
                    cause: RefusalCause::Evidence(EvidenceGap::BaselineUnconfirmed),
                    message: "Profile source transaction settled before runtime admission".into(),
                },
                restored: None,
            }
        } else {
            match self.recover_previous(core).await {
                Err(error) => {
                    let context = self
                        .recovery_required
                        .lock()
                        .await
                        .clone()
                        .unwrap_or_else(|| RecoveryContext {
                            operation_id: Some(operation_id),
                            runtime_operation: None,
                            domain: crate::client::application_workflow::mutation::ConfigDomain::Profiles,
                            stage: MutationStage::TryingCritical,
                            decision: Some(request.decision.clone()),
                            baseline: None,
                            target: None,
                            error: error.to_string(),
                        });
                    RuntimePrepareOutcome::RecoveryRequired(Box::new(context))
                }
                Ok(()) => {
                    self.live = Some(LiveAttempt::source_decision(
                        operation_id,
                        request.decision.clone(),
                    ));
                    self.advance_attempt(AttemptStage::Preparing);
                    self.prepare_candidate(
                        core,
                        &operation_id,
                        &previous,
                        &candidate,
                        request,
                        &mut policy,
                        &mut check,
                        &mut baseline,
                        &mut attempted_runtime,
                    )
                    .await
                }
            }
        };
        if let RuntimePrepareOutcome::Applied(candidate) = &outcome {
            let releases_service = matches!(
                &baseline,
                Some(KnownRuntimeState::Applied(receipt))
                    if receipt.host
                        == crate::core::actor_v2::control_endpoint::ExecutionHost::Service
            ) && candidate.receipt.host
                == crate::core::actor_v2::control_endpoint::ExecutionHost::Local;
            let verdict = AppliedVerdict::new(candidate, releases_service);
            if let Some(attempt) = self
                .live
                .as_mut()
                .filter(|attempt| attempt.operation_id == operation_id)
            {
                attempt.target = Some(candidate.receipt.clone());
            }
            self.record_attempt_verdict(&operation_id, verdict);
        }
        request.answer(outcome.ack());
        if !matches!(&outcome, RuntimePrepareOutcome::RecoveryRequired(_)) {
            self.advance_attempt(AttemptStage::AwaitDecision);
        }

        let decision =
            match tokio::time::timeout(MUTATION_DECISION_TIMEOUT, request.decision.wait()).await {
                Ok(StateDecision::Committed { .. }) => DecisionOutcome::Committed,
                Ok(StateDecision::Aborted {
                    resources: AbortResourceState::Restored,
                }) => DecisionOutcome::Aborted,
                Ok(StateDecision::Aborted {
                    resources: AbortResourceState::NeedsRecovery(incident),
                }) => DecisionOutcome::Unresolved(format!(
                    "source transaction requires local recovery: {}",
                    incident.message
                )),
                Err(_) => DecisionOutcome::Unresolved(
                    "source transaction did not publish a decision within the workflow budget"
                        .into(),
                ),
                Ok(StateDecision::Undecided) => unreachable!("decision wait returned undecided"),
            };

        if matches!(&decision, DecisionOutcome::Committed)
            && let Some(notifications) = &self.notifications
        {
            notifications.profiles_committed(Vec::new());
        }

        let request_runtime_rebuild = matches!(
            (&outcome, &decision),
            (
                RuntimePrepareOutcome::Deferred { .. },
                DecisionOutcome::Committed
            )
        );
        let mut conclusion = MutationConclusion::Confirmed;
        let settlement = match decision {
            DecisionOutcome::Committed => {
                self.advance_attempt_for(&operation_id, AttemptStage::Confirming);
                match &outcome {
                    RuntimePrepareOutcome::Applied(_) => {
                        let verdict = self
                            .live
                            .as_ref()
                            .filter(|attempt| attempt.operation_id == operation_id)
                            .and_then(|attempt| attempt.verdict.clone())
                            .expect("an applied Try records its verdict before ACK");
                        match self.finish_applied(core, &operation_id, &verdict).await {
                            Ok(()) => Ok(()),
                            Err(error) => {
                                let message = format!(
                                    "Profile source committed, but finalizing the applied runtime failed: {error:#}"
                                );
                                let context = self
                                    .mark_recovery_with_attempt(
                                        operation_id.clone(),
                                        baseline.clone(),
                                        request.decision.clone(),
                                        Some(verdict.receipt.clone()),
                                        MutationStage::Confirming,
                                        message.clone(),
                                    )
                                    .await;
                                outcome =
                                    RuntimePrepareOutcome::RecoveryRequired(Box::new(context));
                                conclusion = MutationConclusion::RecoveryRequired;
                                Err(message)
                            }
                        }
                    }
                    RuntimePrepareOutcome::Saved
                    | RuntimePrepareOutcome::SavedInactive
                    | RuntimePrepareOutcome::Deferred { .. } => Ok(()),
                    RuntimePrepareOutcome::Rejected { cause, .. } => {
                        let message = format!(
                            "Profile source committed despite a rejected runtime preparation: {}",
                            cause.message
                        );
                        let context = self
                            .mark_recovery_with_attempt(
                                operation_id.clone(),
                                baseline.clone(),
                                request.decision.clone(),
                                None,
                                MutationStage::Confirming,
                                message.clone(),
                            )
                            .await;
                        outcome = RuntimePrepareOutcome::RecoveryRequired(Box::new(context));
                        conclusion = MutationConclusion::RecoveryRequired;
                        Err(message)
                    }
                    RuntimePrepareOutcome::RecoveryRequired(context) => {
                        conclusion = MutationConclusion::RecoveryRequired;
                        Err(context.error.clone())
                    }
                }
            }
            DecisionOutcome::Aborted => {
                self.advance_attempt_for(&operation_id, AttemptStage::Cancelling);
                let target = match &outcome {
                    RuntimePrepareOutcome::Applied(candidate) => Some(candidate.receipt.clone()),
                    _ => None,
                };
                let result = self
                    .cancel_mutation(
                        core,
                        &operation_id,
                        attempted_runtime,
                        baseline.as_ref(),
                        request.decision.clone(),
                        target,
                    )
                    .await;
                conclusion = if result.is_ok() {
                    if attempted_runtime {
                        MutationConclusion::Cancelled
                    } else {
                        MutationConclusion::Withdrawn
                    }
                } else {
                    let context = self.recovery_required.lock().await.clone();
                    if let Some(context) = context {
                        outcome = RuntimePrepareOutcome::RecoveryRequired(Box::new(context));
                    }
                    MutationConclusion::RecoveryRequired
                };
                result
            }
            DecisionOutcome::Unresolved(message) => {
                self.advance_attempt_for(&operation_id, AttemptStage::AwaitDecision);
                let target = match &outcome {
                    RuntimePrepareOutcome::Applied(candidate) => Some(candidate.receipt.clone()),
                    _ => None,
                };
                let context = self
                    .mark_recovery_with_attempt(
                        operation_id,
                        baseline.clone(),
                        request.decision.clone(),
                        target,
                        MutationStage::AwaitDecision,
                        message.clone(),
                    )
                    .await;
                outcome = RuntimePrepareOutcome::RecoveryRequired(Box::new(context));
                conclusion = MutationConclusion::RecoveryRequired;
                Err(message)
            }
        };
        if conclusion == MutationConclusion::Confirmed {
            match &outcome {
                RuntimePrepareOutcome::Applied(_) | RuntimePrepareOutcome::SavedInactive => {
                    self.deferred = None;
                }
                RuntimePrepareOutcome::Deferred {
                    baseline,
                    digest,
                    cause,
                } => {
                    let previous = self
                        .deferred
                        .take()
                        .filter(|previous| previous.identity == *digest);
                    let attempts_remaining = previous
                        .as_ref()
                        .map_or(super::mutation::DEFERRED_RETRY_BUDGET, |previous| {
                            previous.attempts_remaining
                        });
                    let next_attempt = (attempts_remaining > 0).then(|| {
                        previous
                            .as_ref()
                            .and_then(|previous| previous.next_attempt)
                            .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(1))
                    });
                    self.deferred = Some(super::mutation::DeferredTarget {
                        operation_id: operation_id.clone(),
                        origin: super::mutation::TargetOrigin::Mutation,
                        identity: digest.clone(),
                        cause: cause.clone(),
                        attempts_remaining,
                        attempts: previous.as_ref().map_or(0, |previous| previous.attempts),
                        waits: previous.as_ref().map_or(0, |previous| previous.waits),
                        health: if attempts_remaining > 0 {
                            crate::client::convergence::ConvergenceHealth::RetryScheduled
                        } else {
                            crate::client::convergence::ConvergenceHealth::Blocked
                        },
                        next_attempt,
                        mutation: Some(super::mutation::MutationDeferredContext {
                            digest: digest.clone(),
                            baseline: baseline.clone(),
                            domain: crate::client::application_workflow::mutation::ConfigDomain::Profiles,
                            decision: request.decision.clone(),
                        }),
                    });
                }
                RuntimePrepareOutcome::Saved
                | RuntimePrepareOutcome::Rejected { .. }
                | RuntimePrepareOutcome::RecoveryRequired(_) => {}
            }
        }
        let detail = match &outcome {
            RuntimePrepareOutcome::Deferred { cause, .. } => Some(cause.message.clone()),
            RuntimePrepareOutcome::Rejected { cause, .. } => Some(cause.message.clone()),
            RuntimePrepareOutcome::RecoveryRequired(context) => Some(context.error.clone()),
            _ => None,
        };
        let degradations = match (&outcome, conclusion) {
            (RuntimePrepareOutcome::Deferred { cause, .. }, MutationConclusion::Confirmed) => {
                vec![Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    code: "runtime_deferred".into(),
                    message: cause.message.clone(),
                    retryable: true,
                }]
            }
            (RuntimePrepareOutcome::RecoveryRequired(context), _) => vec![Degradation {
                phase: DegradationPhase::RuntimeApply,
                code: "runtime_recovery_required".into(),
                message: context.error.clone(),
                retryable: false,
            }],
            _ => Vec::new(),
        };
        self.set_outcome(
            &operation_id,
            MutationReceipt {
                degradations,
                operation_id: operation_id.clone(),
                domain: crate::client::application_workflow::mutation::ConfigDomain::Profiles,
                impact,
                policy,
                check,
                outcome: if conclusion == MutationConclusion::RecoveryRequired {
                    MutationOutcomeKind::RecoveryRequired
                } else {
                    outcome.kind()
                },
                refusal: outcome.refusal(),
                conclusion,
                detail,
            },
        )
        .await;
        request.settle(settlement.clone());
        if conclusion == MutationConclusion::RecoveryRequired {
            let reason = self
                .live
                .as_ref()
                .and_then(|attempt| attempt.unresolved.clone())
                .or_else(|| settlement.as_ref().err().cloned())
                .unwrap_or_else(|| "Profile runtime attempt requires recovery".into());
            self.conclude_attempt(Some(reason));
        } else {
            self.conclude_attempt(None);
        }
        self.publish_recovery_context().await;
        (request_runtime_rebuild, settlement)
    }

    async fn prepare_candidate(
        &mut self,
        core: &dyn ProfileRuntime,
        operation_id: &OperationId,
        previous: &Profiles,
        candidate: &Profiles,
        request: &MutationRequest,
        policy: &mut CommandPolicy,
        check: &mut CheckRecord,
        baseline: &mut Option<KnownRuntimeState>,
        attempted_runtime: &mut bool,
    ) -> RuntimePrepareOutcome {
        let impact = classify_profiles(previous, candidate, &request.hints);
        let explicit_switch = request.class == CommandClass::ExplicitSwitch
            || request.hints.activation != ActivationIntent::None;

        if !explicit_switch && impact == RuntimeImpact::None {
            *policy = policy_for(
                request.class,
                impact,
                &Default::default(),
                CoreRunIntent::Running,
            );
            return RuntimePrepareOutcome::Saved;
        }
        *policy = policy_for(
            request.class,
            impact,
            &Default::default(),
            CoreRunIntent::Running,
        );
        if matches!(
            *policy,
            CommandPolicy::SaveOnly | CommandPolicy::SaveThenNotify
        ) {
            return RuntimePrepareOutcome::Saved;
        }

        if core.status().uncertain {
            let message = "Profile runtime mutation cannot proceed while the core lifecycle outcome is uncertain";
            let context = self
                .mark_recovery_with_attempt(
                    operation_id.clone(),
                    baseline.clone(),
                    request.decision.clone(),
                    None,
                    MutationStage::TryingCritical,
                    message.into(),
                )
                .await;
            return RuntimePrepareOutcome::RecoveryRequired(Box::new(context));
        }

        let observed_baseline = match core.observe_runtime_baseline().await {
            Ok(baseline) => baseline,
            Err(error) => {
                return RuntimePrepareOutcome::Rejected {
                    cause: ApplyFailure {
                        stage: MutationStage::Preparing,
                        cause: RefusalCause::Evidence(EvidenceGap::BaselineUnconfirmed),
                        message: format!(
                            "Profile runtime baseline could not be confirmed: {error}"
                        ),
                    },
                    restored: None,
                };
            }
        };
        let inputs = if matches!(&observed_baseline, KnownRuntimeState::NeverApplied) {
            None
        } else {
            Some(
                match core
                    .capture_profile_inputs(
                        Arc::new(candidate.clone()),
                        request.hints.staged_content.clone(),
                    )
                    .await
                {
                    Ok(inputs) => inputs,
                    Err(error) => {
                        return RuntimePrepareOutcome::Rejected {
                            cause: ApplyFailure {
                                stage: MutationStage::Preparing,
                                cause: RefusalCause::Try(TryCauseKind::Deterministic),
                                message: format!(
                                    "Profile runtime inputs could not be captured: {error}"
                                ),
                            },
                            restored: Some(observed_baseline),
                        };
                    }
                },
            )
        };
        match &observed_baseline {
            KnownRuntimeState::Applied(_) => {
                *baseline = Some(observed_baseline.clone());
                self.record_attempt_baseline(&observed_baseline);
            }
            KnownRuntimeState::Stopped => {
                self.record_attempt_baseline(&observed_baseline);
                *policy = policy_for(
                    request.class,
                    impact,
                    &Default::default(),
                    CoreRunIntent::StoppedByUser,
                );
                match core
                    .validate_profile_runtime(inputs.expect("captured for stopped runtime"))
                    .await
                {
                    Ok(record) => *check = record,
                    Err(error) => {
                        if let Some(failure) =
                            error.downcast_ref::<super::mutation::RuntimeCheckFailure>()
                        {
                            *check = failure.record();
                            let disposition = disposition(&TryFailureFacts {
                                cause: failure.cause(),
                                baseline: BaselineAvailability::Known,
                                policy: *policy,
                                candidate_has_invalid_item: false,
                            });
                            if disposition == FailureDisposition::RecoveryRequired {
                                let message = failure.message.clone();
                                let context = self
                                    .mark_recovery_with_attempt(
                                        operation_id.clone(),
                                        Some(KnownRuntimeState::Stopped),
                                        request.decision.clone(),
                                        None,
                                        MutationStage::TryingCritical,
                                        message,
                                    )
                                    .await;
                                return RuntimePrepareOutcome::RecoveryRequired(Box::new(context));
                            }
                            return RuntimePrepareOutcome::Rejected {
                                cause: ApplyFailure {
                                    stage: MutationStage::TryingCritical,
                                    cause: RefusalCause::Try(failure.cause()),
                                    message: failure.message.clone(),
                                },
                                restored: Some(KnownRuntimeState::Stopped),
                            };
                        }
                        return RuntimePrepareOutcome::Rejected {
                            cause: ApplyFailure {
                                stage: MutationStage::TryingCritical,
                                cause: RefusalCause::Try(TryCauseKind::Deterministic),
                                message: format!(
                                    "Profile runtime validation failed while stopped: {error:#}"
                                ),
                            },
                            restored: Some(KnownRuntimeState::Stopped),
                        };
                    }
                }
                return RuntimePrepareOutcome::SavedInactive;
            }
            KnownRuntimeState::NeverApplied => {
                return RuntimePrepareOutcome::Rejected {
                    cause: ApplyFailure {
                        stage: MutationStage::Preparing,
                        cause: RefusalCause::Evidence(EvidenceGap::NoRestorableBaseline),
                        message: "a core is running without a confirmed runtime receipt, so this Profile change cannot be safely rolled back".into(),
                    },
                    restored: None,
                };
            }
        }

        let inputs = inputs.expect("captured for applied runtime");
        let target_digest = match inputs.target_key() {
            Some(digest) => digest,
            None => {
                return RuntimePrepareOutcome::Rejected {
                    cause: ApplyFailure {
                        stage: MutationStage::Preparing,
                        cause: RefusalCause::Try(TryCauseKind::Deterministic),
                        message: format!("Profile runtime target identity could not be serialized"),
                    },
                    restored: baseline.clone(),
                };
            }
        };

        *attempted_runtime = true;
        self.advance_attempt(AttemptStage::TryingCritical);
        match core.prepare_profile_runtime(inputs, operation_id).await {
            Ok((candidate, record)) => {
                *check = record;
                RuntimePrepareOutcome::Applied(candidate)
            }
            Err(error) if safe_to_defer_runtime_apply(&error) => {
                // QueueFull proves this candidate was not admitted to the core
                // executor. Retry after the Profile source transaction commits.
                let failure = TryFailureFacts {
                    cause: TryCauseKind::Transient,
                    baseline: BaselineAvailability::Known,
                    policy: *policy,
                    candidate_has_invalid_item: false,
                };
                *attempted_runtime = false;
                match disposition(&failure) {
                    FailureDisposition::Deferrable => RuntimePrepareOutcome::Deferred {
                        baseline: baseline.clone().expect("captured before runtime Try"),
                        digest: target_digest.clone(),
                        cause: RetryableCause {
                            stage: MutationStage::TryingCritical,
                            message: format!(
                                "Profile was saved with runtime apply deferred: {error}"
                            ),
                        },
                    },
                    FailureDisposition::Reject => RuntimePrepareOutcome::Rejected {
                        cause: ApplyFailure {
                            stage: MutationStage::TryingCritical,
                            cause: RefusalCause::Try(TryCauseKind::Transient),
                            message: error.to_string(),
                        },
                        restored: baseline.clone(),
                    },
                    FailureDisposition::RecoveryRequired => {
                        let message =
                            format!("Profile runtime failure cannot be safely deferred: {error}");
                        let context = self
                            .mark_recovery_with_attempt(
                                operation_id.clone(),
                                baseline.clone(),
                                request.decision.clone(),
                                None,
                                MutationStage::TryingCritical,
                                message,
                            )
                            .await;
                        RuntimePrepareOutcome::RecoveryRequired(Box::new(context))
                    }
                }
            }
            Err(error) => {
                if let Some(check_failure) =
                    error.downcast_ref::<super::mutation::RuntimeCheckFailure>()
                {
                    *check = check_failure.record();
                    *attempted_runtime = false;
                    let failure = TryFailureFacts {
                        cause: check_failure.cause(),
                        baseline: BaselineAvailability::Known,
                        policy: *policy,
                        candidate_has_invalid_item: false,
                    };
                    return match disposition(&failure) {
                        FailureDisposition::Deferrable => RuntimePrepareOutcome::Deferred {
                            baseline: baseline.clone().expect("captured before runtime Try"),
                            digest: target_digest.clone(),
                            cause: RetryableCause {
                                stage: MutationStage::TryingCritical,
                                message: format!(
                                    "Profile was saved with runtime apply deferred: {}",
                                    check_failure.message
                                ),
                            },
                        },
                        FailureDisposition::Reject => RuntimePrepareOutcome::Rejected {
                            cause: ApplyFailure {
                                stage: MutationStage::TryingCritical,
                                cause: RefusalCause::Try(check_failure.cause()),
                                message: check_failure.message.clone(),
                            },
                            restored: baseline.clone(),
                        },
                        FailureDisposition::RecoveryRequired => {
                            let context = self
                                .mark_recovery_with_attempt(
                                    operation_id.clone(),
                                    baseline.clone(),
                                    request.decision.clone(),
                                    None,
                                    MutationStage::TryingCritical,
                                    check_failure.message.clone(),
                                )
                                .await;
                            RuntimePrepareOutcome::RecoveryRequired(Box::new(context))
                        }
                    };
                }
                let actual = core.observe_runtime_baseline().await;
                if actual.as_ref().is_ok_and(|actual| {
                    runtime_state_matches(
                        baseline.as_ref().expect("captured before runtime Try"),
                        actual,
                    )
                }) && !core.status().uncertain
                {
                    let failure = TryFailureFacts {
                        cause: TryCauseKind::Permanent,
                        baseline: BaselineAvailability::Known,
                        policy: *policy,
                        candidate_has_invalid_item: false,
                    };
                    match disposition(&failure) {
                        FailureDisposition::Reject => RuntimePrepareOutcome::Rejected {
                            cause: ApplyFailure {
                                stage: MutationStage::TryingCritical,
                                cause: RefusalCause::Try(TryCauseKind::Permanent),
                                message: error.to_string(),
                            },
                            restored: baseline.clone(),
                        },
                        FailureDisposition::Deferrable => {
                            unreachable!("permanent failures do not defer")
                        }
                        FailureDisposition::RecoveryRequired => {
                            unreachable!("baseline was confirmed")
                        }
                    }
                } else {
                    let message = format!(
                        "Profile runtime outcome could not be confirmed after Try: {error}"
                    );
                    let context = self
                        .mark_recovery_with_attempt(
                            operation_id.clone(),
                            baseline.clone(),
                            request.decision.clone(),
                            None,
                            MutationStage::TryingCritical,
                            message,
                        )
                        .await;
                    RuntimePrepareOutcome::RecoveryRequired(Box::new(context))
                }
            }
        }
    }

    async fn cancel_mutation(
        &mut self,
        core: &dyn ProfileRuntime,
        operation_id: &OperationId,
        attempted_runtime: bool,
        baseline: Option<&KnownRuntimeState>,
        decision: chimera_core::state::DecisionHandle,
        target: Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>,
    ) -> Result<(), String> {
        if !attempted_runtime {
            self.outcomes.lock().await.remove(operation_id);
            return Ok(());
        }

        let Some(baseline) = baseline else {
            let message = "Profile runtime attempt has no confirmed baseline to restore";
            self.mark_recovery_with_attempt(
                operation_id.clone(),
                None,
                decision,
                target,
                MutationStage::Cancelling,
                message.into(),
            )
            .await;
            return Err(message.into());
        };
        if core
            .observe_runtime_baseline()
            .await
            .is_ok_and(|observed| runtime_state_matches(baseline, &observed))
        {
            core.discard_profile_runtime(operation_id).await;
            self.outcomes.lock().await.remove(operation_id);
            return Ok(());
        }
        match core.restore_runtime_baseline(baseline).await {
            Ok(()) => {
                core.discard_profile_runtime(operation_id).await;
                self.outcomes.lock().await.remove(operation_id);
                Ok(())
            }
            Err(error) => {
                let message = format!("failed to restore the previous Profile runtime: {error}");
                self.mark_recovery_with_attempt(
                    operation_id.clone(),
                    Some(baseline.clone()),
                    decision,
                    target,
                    MutationStage::Cancelling,
                    message.clone(),
                )
                .await;
                self.outcomes.lock().await.remove(operation_id);
                Err(message)
            }
        }
    }

    pub(in crate::client) async fn recover_previous(
        &mut self,
        core: &dyn ProfileRuntime,
    ) -> anyhow::Result<()> {
        self.recover_previous_inner(core, false).await
    }

    pub(in crate::client) async fn recover_after_reconcile(
        &mut self,
        core: &dyn ProfileRuntime,
    ) -> anyhow::Result<()> {
        self.recover_previous_inner(core, true).await
    }

    async fn recover_previous_inner(
        &mut self,
        core: &dyn ProfileRuntime,
        current_target_reconciled: bool,
    ) -> anyhow::Result<()> {
        let Some(attempt) = self
            .live
            .as_ref()
            .filter(|attempt| attempt.unresolved.is_some())
            .cloned()
        else {
            return Ok(());
        };
        anyhow::ensure!(
            !core.status().uncertain,
            "Profile runtime still needs recovery after {:?}: {}",
            attempt.stage,
            attempt.unresolved.as_deref().unwrap_or_default()
        );
        if current_target_reconciled {
            let source_decision = match &attempt.origin {
                AttemptOrigin::SourceDecision { decision } => Some(decision),
                AttemptOrigin::CommittedTarget(target) if target.is_mutation() => Some(
                    &target
                        .mutation
                        .as_ref()
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "committed Profile target has no source decision evidence"
                            )
                        })?
                        .decision,
                ),
                AttemptOrigin::CommittedTarget(_) | AttemptOrigin::Lifecycle => None,
            };
            if let Some(decision) = source_decision
                && let Err(error) = ensure_reconciled_source_decision(decision)
            {
                self.publish_recovery_context().await;
                return Err(error);
            }
            let release_service = source_decision.is_some_and(|decision| {
                matches!(decision.decision(), StateDecision::Committed { .. })
            }) && attempt
                .verdict
                .as_ref()
                .is_some_and(|verdict| verdict.releases_service);
            if release_service {
                let current = match core.observe_runtime_baseline().await {
                    Ok(current) => current,
                    Err(error) => {
                        let message = format!(
                            "could not verify the current runtime before releasing Service: {error:#}"
                        );
                        self.conclude_attempt(Some(message.clone()));
                        self.publish_recovery_context().await;
                        return Err(anyhow::anyhow!(message));
                    }
                };
                if matches!(
                    current,
                    KnownRuntimeState::Applied(ref receipt)
                        if receipt.host
                            == crate::core::actor_v2::control_endpoint::ExecutionHost::Local
                ) && let Err(reason) = core.try_release_service_owner().await
                {
                    let message = format!(
                        "Service owner release after current Profile reconciliation failed: {reason}"
                    );
                    self.conclude_attempt(Some(message.clone()));
                    self.publish_recovery_context().await;
                    return Err(anyhow::anyhow!(message));
                }
            }
            core.discard_profile_runtime(&attempt.operation_id).await;
            self.live = None;
            self.deferred = None;
            self.publish_recovery_context().await;
            return Ok(());
        }
        anyhow::ensure!(
            !matches!(
                &attempt.origin,
                AttemptOrigin::CommittedTarget(target) if target.is_mutation()
            ),
            "a committed Profile target requires explicit runtime reconciliation before recovery"
        );
        if matches!(&attempt.origin, AttemptOrigin::Lifecycle) {
            self.live = None;
            self.publish_recovery_context().await;
            return Ok(());
        }
        self.advance_attempt(AttemptStage::Recovering);
        let decision = match &attempt.origin {
            AttemptOrigin::SourceDecision { decision } => Some(decision),
            AttemptOrigin::CommittedTarget(target) => {
                target.mutation.as_ref().map(|mutation| &mutation.decision)
            }
            AttemptOrigin::Lifecycle => None,
        };
        let decision = decision.map(|decision| decision.decision());
        let recovery_error = attempt.unresolved.as_deref().unwrap_or_default();
        let recovery = async {
            match decision {
                Some(StateDecision::Committed { .. }) => {
                    if let Some(verdict) = attempt.verdict.as_ref() {
                        self.recover_applied(core, &attempt.operation_id, verdict)
                            .await
                            .map_err(|error| {
                                anyhow::anyhow!(
                                    "committed Profile runtime confirmation recovery failed after {:?} ({}): {error:#}",
                                    attempt.stage,
                                    recovery_error
                                )
                            })?;
                    } else if let Some(target) = attempt.target.clone() {
                        core.restore_runtime_baseline(&KnownRuntimeState::Applied(target))
                            .await
                            .map_err(|error| {
                                anyhow::anyhow!(
                                    "Profile runtime target recovery for {:?} failed after {:?} ({}): {error}",
                                    crate::client::application_workflow::mutation::ConfigDomain::Profiles,
                                    attempt.stage,
                                    recovery_error
                                )
                            })?;
                    } else if let Some(baseline) = attempt.baseline.as_ref().or_else(|| {
                        match &attempt.origin {
                            AttemptOrigin::CommittedTarget(target) => {
                                target.mutation.as_ref().map(|mutation| &mutation.baseline)
                            }
                            _ => None,
                        }
                    }) {
                        core.restore_runtime_baseline(baseline)
                            .await
                            .map_err(|error| {
                                anyhow::anyhow!(
                                    "Profile runtime baseline recovery failed after {recovery_error}: {error}"
                                )
                            })?;
                    } else {
                        anyhow::bail!(
                            "committed Profile runtime recovery has no confirmed target or baseline: {recovery_error}"
                        );
                    }
                }
                Some(StateDecision::Aborted {
                    resources: AbortResourceState::Restored,
                }) => {
                    if let Some(baseline) = attempt.baseline.as_ref() {
                        core.restore_runtime_baseline(baseline)
                            .await
                            .map_err(|error| {
                                anyhow::anyhow!(
                                    "Profile runtime baseline recovery failed after {recovery_error}: {error}"
                                )
                            })?;
                    }
                }
                Some(StateDecision::Aborted {
                    resources: AbortResourceState::NeedsRecovery(incident),
                }) => anyhow::bail!(
                    "Profile source transaction still needs recovery ({}): {}",
                    incident.message,
                    recovery_error
                ),
                Some(StateDecision::Undecided) | None => anyhow::bail!(
                    "Profile runtime recovery direction is unresolved: {recovery_error}"
                ),
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = recovery {
            let reason = format!("{error:#}");
            self.conclude_attempt(Some(reason));
            self.publish_recovery_context().await;
            return Err(error);
        }

        core.discard_profile_runtime(&attempt.operation_id).await;
        if let Some(target) = self.live.take().and_then(LiveAttempt::into_target) {
            self.deferred = Some(*target);
        }
        if let Some(deferred) = self
            .deferred
            .as_mut()
            .filter(|deferred| deferred.is_mutation())
        {
            deferred.health = crate::client::convergence::ConvergenceHealth::Blocked;
            deferred.next_attempt = None;
        }
        self.publish_recovery_context().await;
        Ok(())
    }

    async fn finish_applied(
        &mut self,
        core: &dyn ProfileRuntime,
        operation_id: &OperationId,
        applied: &AppliedVerdict,
    ) -> anyhow::Result<()> {
        core.confirm_profile_runtime(operation_id, applied.candidate())
            .await
            .context("could not publish the committed Profile runtime product")?;
        self.deferred = None;
        self.clear_startup_retry_target();
        if let Some(notifications) = &self.notifications {
            notifications.runtime_bound(core.effect_port_binding().await, false);
        }
        if applied.releases_service {
            core.try_release_service_owner()
                .await
                .map_err(anyhow::Error::msg)
                .context("could not confirm release of the previous Service owner")?;
        }
        Ok(())
    }

    async fn recover_applied(
        &mut self,
        core: &dyn ProfileRuntime,
        operation_id: &OperationId,
        applied: &AppliedVerdict,
    ) -> anyhow::Result<()> {
        let expected = KnownRuntimeState::Applied(applied.receipt.clone());
        let current = core.observe_runtime_baseline().await?;
        let (receipt, product) = if runtime_state_matches(&expected, &current) {
            let KnownRuntimeState::Applied(receipt) = current else {
                unreachable!("an applied runtime target matched an applied baseline")
            };
            if receipt.revision == applied.receipt.revision {
                (applied.receipt.clone(), applied.product.clone())
            } else {
                let product = receipt.artifact.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "the current runtime receipt has no product for the committed Profile target"
                    )
                })?;
                (receipt, product)
            }
        } else {
            core.restore_runtime_baseline(&expected)
                .await
                .context("could not restore the committed Profile runtime target")?;
            let restored = core.observe_runtime_baseline().await?;
            anyhow::ensure!(
                runtime_state_matches(&expected, &restored),
                "the restored runtime does not match the committed Profile target"
            );
            let KnownRuntimeState::Applied(receipt) = restored else {
                anyhow::bail!("the restored Profile runtime has no applied receipt");
            };
            let product = receipt.artifact.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "the restored runtime receipt has no product for the committed Profile target"
                )
            })?;
            (receipt, product)
        };
        let applied = applied.with_runtime(receipt, product);
        self.finish_applied(core, operation_id, &applied).await
    }

    async fn set_outcome(&self, operation_id: &OperationId, receipt: MutationReceipt) {
        self.outcomes
            .lock()
            .await
            .insert(operation_id.clone(), receipt);
    }
}

fn ensure_reconciled_source_decision(
    decision: &chimera_core::state::DecisionHandle,
) -> anyhow::Result<()> {
    match decision.decision() {
        StateDecision::Committed { .. }
        | StateDecision::Aborted {
            resources: AbortResourceState::Restored,
        } => Ok(()),
        StateDecision::Undecided => {
            anyhow::bail!(
                "Profile source transaction is undecided; reconcile again after it settles"
            )
        }
        StateDecision::Aborted {
            resources: AbortResourceState::NeedsRecovery(incident),
        } => anyhow::bail!(
            "Profile source transaction still requires resource recovery ({}); its recovery owner remains retained",
            incident.message
        ),
    }
}

fn runtime_state_matches(expected: &KnownRuntimeState, actual: &KnownRuntimeState) -> bool {
    match (expected, actual) {
        (KnownRuntimeState::Applied(expected), KnownRuntimeState::Applied(actual)) => {
            expected.config_digest == actual.config_digest
                && expected.target_core == actual.target_core
                && expected.core_spec.kind == actual.core_spec.kind
                && expected.core_spec.binary_path == actual.core_spec.binary_path
                && expected.core_spec.version == actual.core_spec.version
                && expected.core_spec.features == actual.core_spec.features
                && expected.host == actual.host
                && expected.run_intent == actual.run_intent
                && expected.local_ipc == actual.local_ipc
        }
        (KnownRuntimeState::Stopped, KnownRuntimeState::Stopped)
        | (KnownRuntimeState::NeverApplied, KnownRuntimeState::NeverApplied) => true,
        _ => false,
    }
}

pub(in crate::client) fn safe_to_defer_runtime_apply(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::QueueFull) && error.retryable)
    })
}

pub(in crate::client) fn safe_to_retry_deferred_runtime(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::QueueFull) && error.retryable)
            || cause
                .downcast_ref::<super::mutation::RuntimeCheckFailure>()
                .is_some_and(|failure| failure.cause() == TryCauseKind::Transient)
    })
}

pub(in crate::client) fn deferred_runtime_waiting_dependency(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<super::mutation::RuntimeCheckFailure>()
            .is_some_and(|failure| matches!(failure.record(), CheckRecord::Unserviceable(_)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use camino::Utf8PathBuf;
    use chimera_config::profile::Profiles;
    use chimera_core::state::{
        PersistentStateManagerSetup, ReplaceIfVersionResult, StateParticipant,
    };
    use chimera_core_manager::OperationId;
    use tempfile::{TempDir, tempdir};
    use tokio::sync::{Mutex, oneshot};

    use crate::{
        client::{
            application_workflow::{
                ApplicationWorkflowClient, impact::MutationHints, mutation::KnownRuntimeState,
                participant::ApplicationMutationParticipant, policy::CommandClass,
                workflow::RecoveryContext,
            },
            core_lifecycle::{CoreLifecycleStatus, ports::CoreStatusSnapshot},
            runtime::{
                RuntimeApplyReceipt, RuntimeCommitStatus, RuntimeRevisionAllocator, RuntimeSnapshot,
            },
        },
        core::{actor_v2::control_endpoint::ExecutionHost, clash::core::RunType},
    };

    struct TestProfileRuntime {
        applied: Mutex<Vec<Profiles>>,
        restored: Mutex<Vec<KnownRuntimeState>>,
        confirmed: Mutex<Vec<(OperationId, super::super::mutation::AppliedCandidate)>>,
        discarded: Mutex<Vec<OperationId>>,
        baseline: StdMutex<KnownRuntimeState>,
        block_first: AtomicBool,
        queue_full_first: AtomicBool,
        fail_prepare: AtomicBool,
        fail_confirm: AtomicBool,
        fail_validation: AtomicBool,
        fail_restore: AtomicBool,
        fail_service_release: AtomicBool,
        status_uncertain: AtomicBool,
        validations: AtomicUsize,
        rebuilds: AtomicUsize,
        service_releases: AtomicUsize,
        started: StdMutex<Option<oneshot::Sender<()>>>,
        release: StdMutex<Option<oneshot::Receiver<()>>>,
    }

    impl TestProfileRuntime {
        fn new() -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                restored: Mutex::new(Vec::new()),
                confirmed: Mutex::new(Vec::new()),
                discarded: Mutex::new(Vec::new()),
                baseline: StdMutex::new(KnownRuntimeState::Applied(test_runtime_receipt())),
                block_first: AtomicBool::new(false),
                queue_full_first: AtomicBool::new(false),
                fail_prepare: AtomicBool::new(false),
                fail_confirm: AtomicBool::new(false),
                fail_validation: AtomicBool::new(false),
                fail_restore: AtomicBool::new(false),
                fail_service_release: AtomicBool::new(false),
                status_uncertain: AtomicBool::new(false),
                validations: AtomicUsize::new(0),
                rebuilds: AtomicUsize::new(0),
                service_releases: AtomicUsize::new(0),
                started: StdMutex::new(None),
                release: StdMutex::new(None),
            }
        }

        fn blocking(started: oneshot::Sender<()>, release: oneshot::Receiver<()>) -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                restored: Mutex::new(Vec::new()),
                confirmed: Mutex::new(Vec::new()),
                discarded: Mutex::new(Vec::new()),
                baseline: StdMutex::new(KnownRuntimeState::Applied(test_runtime_receipt())),
                block_first: AtomicBool::new(true),
                queue_full_first: AtomicBool::new(false),
                fail_prepare: AtomicBool::new(false),
                fail_confirm: AtomicBool::new(false),
                fail_validation: AtomicBool::new(false),
                fail_restore: AtomicBool::new(false),
                fail_service_release: AtomicBool::new(false),
                status_uncertain: AtomicBool::new(false),
                validations: AtomicUsize::new(0),
                rebuilds: AtomicUsize::new(0),
                service_releases: AtomicUsize::new(0),
                started: StdMutex::new(Some(started)),
                release: StdMutex::new(Some(release)),
            }
        }

        fn fail_prepare() -> Self {
            let mut runtime = Self::new();
            runtime.fail_prepare.store(true, Ordering::Release);
            runtime
        }

        fn fail_validation() -> Self {
            let mut runtime = Self::new();
            runtime.fail_validation.store(true, Ordering::Release);
            runtime
        }

        fn fail_confirm() -> Self {
            let mut runtime = Self::new();
            runtime.fail_confirm.store(true, Ordering::Release);
            runtime
        }

        fn queue_full_first() -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                restored: Mutex::new(Vec::new()),
                confirmed: Mutex::new(Vec::new()),
                discarded: Mutex::new(Vec::new()),
                baseline: StdMutex::new(KnownRuntimeState::Applied(test_runtime_receipt())),
                block_first: AtomicBool::new(false),
                queue_full_first: AtomicBool::new(true),
                fail_prepare: AtomicBool::new(false),
                fail_confirm: AtomicBool::new(false),
                fail_validation: AtomicBool::new(false),
                fail_restore: AtomicBool::new(false),
                fail_service_release: AtomicBool::new(false),
                status_uncertain: AtomicBool::new(false),
                validations: AtomicUsize::new(0),
                rebuilds: AtomicUsize::new(0),
                service_releases: AtomicUsize::new(0),
                started: StdMutex::new(None),
                release: StdMutex::new(None),
            }
        }

        fn fail_restore() -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                restored: Mutex::new(Vec::new()),
                confirmed: Mutex::new(Vec::new()),
                discarded: Mutex::new(Vec::new()),
                baseline: StdMutex::new(KnownRuntimeState::Applied(test_runtime_receipt())),
                block_first: AtomicBool::new(false),
                queue_full_first: AtomicBool::new(false),
                fail_prepare: AtomicBool::new(false),
                fail_confirm: AtomicBool::new(false),
                fail_validation: AtomicBool::new(false),
                fail_restore: AtomicBool::new(true),
                fail_service_release: AtomicBool::new(false),
                status_uncertain: AtomicBool::new(false),
                validations: AtomicUsize::new(0),
                rebuilds: AtomicUsize::new(0),
                service_releases: AtomicUsize::new(0),
                started: StdMutex::new(None),
                release: StdMutex::new(None),
            }
        }

        fn service_baseline_fails_release() -> Self {
            let runtime = Self::new();
            *runtime
                .baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                KnownRuntimeState::Applied(service_runtime_receipt());
            runtime.fail_service_release.store(true, Ordering::Release);
            runtime
        }
    }

    #[async_trait::async_trait]
    impl ProfileRuntime for TestProfileRuntime {
        fn status(&self) -> CoreLifecycleStatus {
            CoreLifecycleStatus {
                uncertain: self.status_uncertain.load(Ordering::Acquire),
                ..Default::default()
            }
        }

        async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: chimera_ipc::api::status::CoreState::Running,
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        async fn effect_port_binding(
            &self,
        ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
            None
        }

        async fn capture_profile_inputs(
            &self,
            profiles: Arc<Profiles>,
            staged_content: std::collections::BTreeMap<String, String>,
        ) -> anyhow::Result<super::super::inputs::RuntimeInputs> {
            Ok(super::super::inputs::RuntimeInputs {
                app: chimera_config::application::ChimeraAppConfig::default(),
                clash: chimera_config::clash::config::ClashConfig::default(),
                profiles,
                content: super::super::inputs::FrozenProfileContent(
                    staged_content
                        .into_iter()
                        .map(|(path, content)| (path, Ok(content)))
                        .collect(),
                ),
            })
        }

        async fn observe_runtime_baseline(&self) -> anyhow::Result<KnownRuntimeState> {
            Ok(self
                .baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone())
        }

        async fn restore_runtime_baseline(
            &self,
            baseline: &KnownRuntimeState,
        ) -> anyhow::Result<()> {
            self.restored.lock().await.push(baseline.clone());
            if self.fail_restore.load(Ordering::Acquire) {
                anyhow::bail!("injected Profile runtime restore failure");
            }
            *self
                .baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = baseline.clone();
            Ok(())
        }

        async fn validate_profile_runtime(
            &self,
            _inputs: super::super::inputs::RuntimeInputs,
        ) -> anyhow::Result<CheckRecord> {
            self.validations.fetch_add(1, Ordering::SeqCst);
            if self.fail_validation.load(Ordering::Acquire) {
                return Err(super::super::mutation::RuntimeCheckFailure {
                    outcome:
                        crate::client::application_workflow::ports::RuntimeCheckOutcome::Rejected {
                            kind: Some(chimera_core_manager::CoreErrorKind::ConfigCheckFailed),
                            message: "injected invalid Profile config".into(),
                        },
                    message: "injected Profile validation failure".into(),
                }
                .into());
            }
            Ok(CheckRecord::Passed)
        }

        async fn prepare_profile_runtime(
            &self,
            inputs: super::super::inputs::RuntimeInputs,
            _operation_id: &OperationId,
        ) -> anyhow::Result<(super::super::mutation::AppliedCandidate, CheckRecord)> {
            if self.fail_prepare.swap(false, Ordering::AcqRel) {
                anyhow::bail!("injected Profile runtime prepare failure");
            }
            let (_, _, profiles, staged_content) = inputs.into_parts()?;
            self.reconcile_profiles(profiles, staged_content).await?;
            let receipt = candidate_runtime_receipt();
            *self
                .baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                KnownRuntimeState::Applied(receipt.clone());
            Ok((
                super::super::mutation::AppliedCandidate {
                    replaced: true,
                    product: receipt.artifact.clone().expect("test receipt artifact"),
                    receipt,
                },
                CheckRecord::Passed,
            ))
        }

        async fn confirm_profile_runtime(
            &self,
            operation_id: &OperationId,
            candidate: super::super::mutation::AppliedCandidate,
        ) -> anyhow::Result<()> {
            if self.fail_confirm.swap(false, Ordering::AcqRel) {
                anyhow::bail!("injected Profile runtime confirmation failure");
            }
            self.confirmed
                .lock()
                .await
                .push((operation_id.clone(), candidate));
            Ok(())
        }

        async fn try_release_service_owner(&self) -> std::result::Result<(), String> {
            self.service_releases.fetch_add(1, Ordering::SeqCst);
            if self.fail_service_release.swap(false, Ordering::AcqRel) {
                Err("injected Service owner release failure".into())
            } else {
                Ok(())
            }
        }

        async fn discard_profile_runtime(&self, operation_id: &OperationId) {
            self.discarded.lock().await.push(operation_id.clone());
        }

        async fn reconcile_profiles(
            &self,
            profiles: Arc<Profiles>,
            _staged_content: std::collections::BTreeMap<String, String>,
        ) -> anyhow::Result<()> {
            let call = {
                let mut applied = self.applied.lock().await;
                applied.push(profiles.as_ref().clone());
                applied.len()
            };
            if self.block_first.swap(false, Ordering::AcqRel) {
                if let Some(started) = self
                    .started
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    let _ = started.send(());
                }
                let release = self
                    .release
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(release) = release {
                    release
                        .await
                        .map_err(|_| anyhow::anyhow!("test release dropped"))?;
                }
            }
            if call == 1 && self.queue_full_first.swap(false, Ordering::AcqRel) {
                return Err(anyhow::Error::new(CoreError::new(
                    CoreErrorKind::QueueFull,
                    "executor queue is full",
                    true,
                )));
            }
            Ok(())
        }

        fn request_runtime_rebuild(&self) {
            self.rebuilds.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn profile_manager() -> (
        chimera_core::state::PersistentStateManager<
            Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >,
        TempDir,
    ) {
        let directory = tempdir().expect("temporary Profile directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
            .expect("UTF-8 temporary Profile path");
        let manager = PersistentStateManagerSetup::<
            Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(Profiles::default())
        .await
        .expect("Profile state manager");
        (manager, directory)
    }

    async fn workflow(
        runtime: Arc<TestProfileRuntime>,
    ) -> (
        ApplicationWorkflowClient,
        Arc<Mutex<std::collections::HashMap<OperationId, MutationReceipt>>>,
        Arc<Mutex<Option<RecoveryContext>>>,
    ) {
        workflow_with_notifications(runtime, None).await
    }

    async fn workflow_with_notifications(
        runtime: Arc<TestProfileRuntime>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    ) -> (
        ApplicationWorkflowClient,
        Arc<Mutex<std::collections::HashMap<OperationId, MutationReceipt>>>,
        Arc<Mutex<Option<RecoveryContext>>>,
    ) {
        let outcomes = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let recovery_required = Arc::new(Mutex::new(None));
        let workflow = ApplicationWorkflowClient::spawn_with_runtime_and_notifications(
            runtime,
            outcomes.clone(),
            recovery_required.clone(),
            notifications,
        )
        .await
        .expect("application workflow actor");
        (workflow, outcomes, recovery_required)
    }

    #[derive(Default)]
    struct RecordingCommitNotifications {
        profiles: AtomicUsize,
        runtime_bindings: StdMutex<
            Vec<(
                Option<chimera_config::runtime::executor::ResolvedPortBindings>,
                bool,
            )>,
        >,
    }

    impl crate::client::effects::ports::CommitNotifications for RecordingCommitNotifications {
        fn application_committed(
            &self,
            _: crate::client::effects::plan::ApplicationEffectFields,
            _: Vec<crate::client::effects::plan::EffectKind>,
        ) {
            unreachable!("profile mutation does not publish application fields")
        }

        fn clash_committed(&self, _: crate::client::effects::plan::ClashEffectFields) {
            unreachable!("profile mutation does not publish Clash fields")
        }

        fn profiles_committed(&self, _: Vec<chimera_config::profile::ProfileId>) {
            self.profiles.fetch_add(1, Ordering::SeqCst);
        }

        fn runtime_bound(
            &self,
            ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
            refresh: bool,
        ) {
            self.runtime_bindings.lock().unwrap().push((ports, refresh));
        }

        fn publish_full(&self, _: Option<chimera_config::runtime::executor::ResolvedPortBindings>) {
            unreachable!("profile mutation does not publish a full effect snapshot")
        }
    }

    fn candidate_profiles() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.valid.push("test-runtime-field".into());
        profiles
    }

    fn test_runtime_receipt() -> Arc<RuntimeApplyReceipt> {
        runtime_receipt("mode: rule\n")
    }

    fn service_runtime_receipt() -> Arc<RuntimeApplyReceipt> {
        let mut receipt = (*test_runtime_receipt()).clone();
        receipt.host = ExecutionHost::Service;
        Arc::new(receipt)
    }

    fn candidate_runtime_receipt() -> Arc<RuntimeApplyReceipt> {
        runtime_receipt("mode: direct\n")
    }

    fn runtime_receipt(config_text: &str) -> Arc<RuntimeApplyReceipt> {
        let revision = RuntimeRevisionAllocator::default().allocate().unwrap();
        let artifact = Arc::new(RuntimeSnapshot::new_with_transform_output(
            revision,
            crate::config::chimera::ClashCore::Mihomo,
            config_text.as_bytes().to_vec(),
            serde_yaml::Mapping::new(),
            crate::enhance::PostProcessingOutput::default(),
        ));
        Arc::new(RuntimeApplyReceipt {
            revision,
            config_text: Arc::from(config_text),
            config_digest: chimera_core_manager::payload_digest(config_text.as_bytes()),
            target_core: crate::config::chimera::ClashCore::Mihomo,
            core_spec: chimera_core_manager::CoreSpec {
                kind: chimera_core_manager::CoreKind::Mihomo,
                binary_path: Utf8PathBuf::from("/test/mihomo"),
                version: None,
                features: Vec::new(),
            },
            host: ExecutionHost::Local,
            run_intent: CoreRunIntent::Running,
            local_ipc: chimera_core_manager::LocalIpcSettings {
                policy: chimera_core_manager::LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
            applied_revision: chimera_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "test-source-hash".into(),
                effective_hash: "test-effective-hash".into(),
            },
            target: None,
            ports: Default::default(),
            artifact: Some(artifact),
        })
    }

    fn participant(
        operation_id: OperationId,
        decision: chimera_core::state::DecisionHandle,
        workflow: ApplicationWorkflowClient,
        ack_timeout: Duration,
    ) -> StateParticipant<Profiles> {
        ApplicationMutationParticipant::<Profiles>::with_ack_timeout(
            operation_id,
            MutationHints::default(),
            CommandClass::Save,
            decision,
            workflow,
            ack_timeout,
        )
    }

    #[test]
    fn runtime_deferral_requires_a_retryable_queue_full_error() {
        let queue_full = anyhow::Error::new(CoreError::new(
            CoreErrorKind::QueueFull,
            "executor queue is full",
            true,
        ));
        assert!(safe_to_defer_runtime_apply(&queue_full));

        let non_retryable_queue_full = anyhow::Error::new(CoreError::new(
            CoreErrorKind::QueueFull,
            "queue full without retryability",
            false,
        ));
        assert!(!safe_to_defer_runtime_apply(&non_retryable_queue_full));

        let other_retryable_error = anyhow::Error::new(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "backend unavailable",
            true,
        ));
        assert!(!safe_to_defer_runtime_apply(&other_retryable_error));

        let untyped = anyhow::anyhow!("executor queue is full");
        assert!(!safe_to_defer_runtime_apply(&untyped));

        let retryable_check =
            crate::client::application_workflow::mutation::RuntimeCheckFailure::from_core_error(
                CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    "service check endpoint is reconnecting",
                    true,
                ),
            );
        assert!(safe_to_retry_deferred_runtime(&anyhow::Error::new(
            retryable_check.clone()
        )));
        assert!(deferred_runtime_waiting_dependency(&anyhow::Error::new(
            retryable_check
        )));

        let rejected_check =
            crate::client::application_workflow::mutation::RuntimeCheckFailure::from_core_error(
                CoreError::new(
                    CoreErrorKind::ConfigCheckFailed,
                    "invalid Profile target",
                    false,
                ),
            );
        assert!(!safe_to_retry_deferred_runtime(&anyhow::Error::new(
            rejected_check.clone()
        )));
        assert!(!deferred_runtime_waiting_dependency(&anyhow::Error::new(
            rejected_check
        )));
    }

    #[tokio::test]
    async fn committed_profile_mutation_settles_after_runtime_apply() {
        let (mut manager, _directory) = profile_manager().await;
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::new());
        let notifications = Arc::new(RecordingCommitNotifications::default());
        let (workflow, outcomes, _) =
            workflow_with_notifications(runtime.clone(), Some(notifications.clone())).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("Profile source transaction");

        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            candidate.valid
        );
        let applied = runtime.applied.lock().await;
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].valid, candidate.valid);
        let confirmed = runtime.confirmed.lock().await;
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].0, operation_id);
        drop(confirmed);
        assert!(runtime.discarded.lock().await.is_empty());
        assert_eq!(
            outcomes
                .lock()
                .await
                .get(&operation_id)
                .map(MutationReceipt::runtime_status),
            Some(RuntimeCommitStatus::Applied)
        );
        assert_eq!(
            outcomes.lock().await.get(&operation_id).unwrap().check,
            CheckRecord::Passed
        );
        assert_eq!(notifications.profiles.load(Ordering::SeqCst), 1);
        assert_eq!(
            notifications.runtime_bindings.lock().unwrap().as_slice(),
            [(None, false)],
            "a confirmed profile runtime publishes its active core port binding"
        );
    }

    /// Contract: a committed Try whose first Confirm fails keeps its receipt
    /// and product on the live attempt. The next mutation must replay that
    /// candidate before starting, without reapplying the Core target.
    #[tokio::test]
    async fn failed_profile_confirmation_replays_the_committed_candidate_before_next_mutation() {
        let (mut manager, _directory) = profile_manager().await;
        let first_candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::fail_confirm());
        let (workflow, outcomes, recovery_required) = workflow(runtime.clone()).await;
        let first_operation_id = OperationId::generate();
        let first_version = manager.snapshot_handle().load().version;

        let _ = manager
            .replace_if_version_with_participant(
                first_version,
                first_candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(
                            first_operation_id,
                            decision,
                            workflow,
                            Duration::from_secs(1),
                        )
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await;

        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            first_candidate.valid
        );
        assert_eq!(
            outcomes
                .lock()
                .await
                .get(&first_operation_id)
                .map(MutationReceipt::runtime_status),
            Some(RuntimeCommitStatus::RecoveryRequired)
        );
        {
            let recovery = recovery_required.lock().await;
            let context = recovery.as_ref().expect("typed Confirm recovery context");
            assert_eq!(context.stage, MutationStage::Confirming);
            assert!(context.decision.is_some());
            assert_eq!(
                context.target.as_ref().unwrap().config_digest,
                candidate_runtime_receipt().config_digest
            );
        }

        let mut second_candidate = first_candidate.clone();
        second_candidate.valid.push("second-runtime-field".into());
        let second_operation_id = OperationId::generate();
        let second_version = manager.snapshot_handle().load().version;
        let result = manager
            .replace_if_version_with_participant(
                second_version,
                second_candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(
                            second_operation_id,
                            decision,
                            workflow,
                            Duration::from_secs(1),
                        )
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("the next mutation should recover and proceed");

        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert!(recovery_required.lock().await.is_none());
        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            second_candidate.valid
        );
        let confirmed = runtime.confirmed.lock().await;
        assert_eq!(confirmed.len(), 2);
        assert_eq!(confirmed[0].0, first_operation_id);
        assert_eq!(confirmed[1].0, second_operation_id);
        assert_eq!(
            confirmed[0].1.receipt.config_digest,
            candidate_runtime_receipt().config_digest
        );
        assert_eq!(
            confirmed[0].1.product.product_bytes(),
            candidate_runtime_receipt()
                .artifact
                .as_ref()
                .unwrap()
                .product_bytes()
        );
        assert!(
            runtime.restored.lock().await.is_empty(),
            "a settled confirmed target should replay its saved product without reapplying the Core"
        );
    }

    /// Contract: a Service-to-Local Try commits and publishes its product,
    /// then an injected owner-release failure leaves recovery responsible for
    /// retrying both the same verdict and the owner fence before later work.
    #[tokio::test]
    async fn service_release_failure_retains_the_committed_profile_verdict_for_replay() {
        let (mut manager, _directory) = profile_manager().await;
        let first_candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::service_baseline_fails_release());
        let (workflow, outcomes, recovery_required) = workflow(runtime.clone()).await;
        let first_operation_id = OperationId::generate();
        let first_version = manager.snapshot_handle().load().version;

        let _ = manager
            .replace_if_version_with_participant(
                first_version,
                first_candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(
                            first_operation_id,
                            decision,
                            workflow,
                            Duration::from_secs(1),
                        )
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await;

        assert_eq!(
            outcomes
                .lock()
                .await
                .get(&first_operation_id)
                .map(MutationReceipt::runtime_status),
            Some(RuntimeCommitStatus::RecoveryRequired)
        );
        assert_eq!(runtime.service_releases.load(Ordering::SeqCst), 1);
        assert_eq!(
            runtime
                .confirmed
                .lock()
                .await
                .iter()
                .filter(|(operation_id, _)| operation_id == &first_operation_id)
                .count(),
            1,
            "the committed candidate must be published before Service release is attempted"
        );

        let mut second_candidate = first_candidate.clone();
        second_candidate.valid.push("second-runtime-field".into());
        let second_operation_id = OperationId::generate();
        let second_version = manager.snapshot_handle().load().version;
        manager
            .replace_if_version_with_participant(
                second_version,
                second_candidate,
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(
                            second_operation_id,
                            decision,
                            workflow,
                            Duration::from_secs(1),
                        )
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("recovery should retry the Service release before the next mutation");

        assert!(recovery_required.lock().await.is_none());
        assert_eq!(runtime.service_releases.load(Ordering::SeqCst), 2);
        assert_eq!(
            runtime
                .confirmed
                .lock()
                .await
                .iter()
                .filter(|(operation_id, _)| operation_id == &first_operation_id)
                .count(),
            2,
            "recovery must replay the same product before retrying the owner fence"
        );
    }

    #[tokio::test]
    async fn running_core_without_a_receipt_refuses_before_runtime_admission() {
        let (mut manager, _directory) = profile_manager().await;
        let runtime = Arc::new(TestProfileRuntime::new());
        *runtime
            .baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = KnownRuntimeState::NeverApplied;
        let (workflow, outcomes, recovery_required) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate_profiles(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert!(runtime.applied.lock().await.is_empty());
        assert!(runtime.restored.lock().await.is_empty());
        let outcome = outcomes.lock().await.get(&operation_id).cloned().unwrap();
        assert_eq!(outcome.outcome, MutationOutcomeKind::Rejected);
        assert_eq!(outcome.conclusion, MutationConclusion::Withdrawn);
        assert!(matches!(
            outcome.refusal,
            Some(RefusalCause::Evidence(EvidenceGap::NoRestorableBaseline))
        ));
        assert!(recovery_required.lock().await.is_none());
    }

    #[tokio::test]
    async fn unproven_startup_owner_blocks_profile_runtime_admission() {
        let (mut manager, _directory) = profile_manager().await;
        let runtime = Arc::new(TestProfileRuntime::new());
        runtime.status_uncertain.store(true, Ordering::Release);
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate_profiles(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert!(runtime.applied.lock().await.is_empty());
        assert!(runtime.confirmed.lock().await.is_empty());
        assert_eq!(runtime.validations.load(Ordering::Acquire), 0);
        let receipt = outcomes.lock().await.get(&operation_id).cloned().unwrap();
        assert_eq!(receipt.outcome, MutationOutcomeKind::RecoveryRequired);
    }

    #[tokio::test]
    async fn stopped_core_saves_profile_without_starting_runtime() {
        let (mut manager, _directory) = profile_manager().await;
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::new());
        *runtime
            .baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = KnownRuntimeState::Stopped;
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("Profile source transaction should commit while stopped");

        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            candidate.valid
        );
        assert!(runtime.applied.lock().await.is_empty());
        assert!(runtime.restored.lock().await.is_empty());
        assert_eq!(runtime.validations.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcomes
                .lock()
                .await
                .get(&operation_id)
                .map(MutationReceipt::runtime_status),
            Some(RuntimeCommitStatus::SavedInactive)
        );
        assert_eq!(
            outcomes.lock().await.get(&operation_id).unwrap().check,
            CheckRecord::Passed
        );
    }

    #[tokio::test]
    async fn invalid_profile_is_rejected_while_core_is_stopped() {
        let (mut manager, _directory) = profile_manager().await;
        let runtime = Arc::new(TestProfileRuntime::fail_validation());
        *runtime
            .baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = KnownRuntimeState::Stopped;
        let (workflow, outcomes, recovery_required) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate_profiles(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert_eq!(runtime.validations.load(Ordering::SeqCst), 1);
        assert!(runtime.applied.lock().await.is_empty());
        assert!(runtime.restored.lock().await.is_empty());
        let outcome = outcomes.lock().await.get(&operation_id).cloned().unwrap();
        assert_eq!(outcome.outcome, MutationOutcomeKind::Rejected);
        assert_eq!(outcome.conclusion, MutationConclusion::Withdrawn);
        assert_eq!(
            outcome.check,
            CheckRecord::Rejected("injected invalid Profile config".into())
        );
        assert!(recovery_required.lock().await.is_none());
    }

    #[tokio::test]
    async fn aborted_profile_mutation_restores_previous_runtime() {
        let (mut manager, _directory) = profile_manager().await;
        let previous = Profiles::default();
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::new());
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { anyhow::bail!("force source-side failure after prepare") },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert_eq!(manager.snapshot_handle().load().state.valid, previous.valid);
        let applied = runtime.applied.lock().await;
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].valid, candidate.valid);
        drop(applied);
        let restored = runtime.restored.lock().await;
        assert_eq!(restored.len(), 1);
        let KnownRuntimeState::Applied(receipt) = &restored[0] else {
            panic!("the confirmed runtime receipt should be restored");
        };
        assert_eq!(receipt.config_digest, test_runtime_receipt().config_digest);
        assert!(runtime.confirmed.lock().await.is_empty());
        assert_eq!(runtime.discarded.lock().await.as_slice(), &[operation_id]);
        let outcome = outcomes.lock().await.get(&operation_id).cloned().unwrap();
        assert_eq!(outcome.outcome, MutationOutcomeKind::Applied);
        assert_eq!(outcome.conclusion, MutationConclusion::Cancelled);
    }

    #[tokio::test]
    async fn rejected_profile_runtime_that_left_the_baseline_unchanged_skips_restore() {
        let (mut manager, _directory) = profile_manager().await;
        let runtime = Arc::new(TestProfileRuntime::fail_prepare());
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate_profiles(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert!(runtime.applied.lock().await.is_empty());
        assert!(runtime.restored.lock().await.is_empty());
        assert_eq!(runtime.discarded.lock().await.as_slice(), &[operation_id]);
        let outcome = outcomes.lock().await.get(&operation_id).cloned().unwrap();
        assert_eq!(outcome.outcome, MutationOutcomeKind::Rejected);
        assert_eq!(outcome.conclusion, MutationConclusion::Cancelled);
    }

    #[tokio::test]
    async fn safe_queue_full_commits_and_schedules_runtime_retry() {
        let (mut manager, _directory) = profile_manager().await;
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::queue_full_first());
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("safe runtime deferral should commit Profile state");

        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            candidate.valid
        );
        assert_eq!(
            outcomes
                .lock()
                .await
                .get(&operation_id)
                .map(MutationReceipt::runtime_status),
            Some(RuntimeCommitStatus::Deferred)
        );
        assert_eq!(runtime.rebuilds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_profile_rollback_latches_recovery_outcome() {
        let (mut manager, _directory) = profile_manager().await;
        let previous = Profiles::default();
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::fail_restore());
        let (workflow, outcomes, recovery_required) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate,
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { anyhow::bail!("force source-side failure after prepare") },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert_eq!(manager.snapshot_handle().load().state.valid, previous.valid);
        let outcome = outcomes.lock().await.get(&operation_id).cloned().unwrap();
        assert_eq!(outcome.outcome, MutationOutcomeKind::RecoveryRequired);
        assert_eq!(outcome.conclusion, MutationConclusion::RecoveryRequired);
        let recovery = recovery_required.lock().await;
        let context = recovery.as_ref().expect("typed recovery context");
        assert_eq!(context.operation_id.as_ref(), Some(&operation_id));
        assert_eq!(context.stage, MutationStage::Cancelling);
        assert!(context.decision.is_some());
        assert!(matches!(
            context.baseline,
            Some(KnownRuntimeState::Applied(_))
        ));
    }

    #[tokio::test]
    async fn timed_out_prepare_waits_for_try_then_restores_baseline() {
        let (manager, _directory) = profile_manager().await;
        let previous = Profiles::default();
        let candidate = candidate_profiles();
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let runtime = Arc::new(TestProfileRuntime::blocking(started_tx, release_rx));
        let (workflow, _, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let expected_candidate = candidate.clone();
        let version = manager.snapshot_handle().load().version;

        let transaction = tokio::spawn({
            let workflow = workflow.clone();
            async move {
                let mut manager = manager;
                let result = manager
                    .replace_if_version_with_participant(
                        version,
                        candidate.clone(),
                        move |decision| {
                            ApplicationMutationParticipant::<Profiles>::with_ack_timeout(
                                operation_id,
                                MutationHints::default(),
                                CommandClass::Save,
                                decision,
                                workflow,
                                Duration::from_millis(20),
                            )
                        },
                        || async { Ok(()) },
                        || async { Ok(()) },
                    )
                    .await;
                (manager, result)
            }
        });

        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("candidate Try should start")
            .expect("candidate Try start notification");
        tokio::time::sleep(Duration::from_millis(60)).await;
        release_tx.send(()).expect("release runtime Try");
        let (manager, result) = tokio::time::timeout(Duration::from_secs(2), transaction)
            .await
            .expect("transaction should settle after cancellation")
            .expect("source transaction task");

        assert!(result.is_err());
        assert_eq!(manager.snapshot_handle().load().state.valid, previous.valid);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime.restored.lock().await.len() == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("tracked workflow should finish rollback after its waiter timed out");
        let applied = runtime.applied.lock().await;
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].valid, expected_candidate.valid);
        drop(applied);
        assert_eq!(runtime.restored.lock().await.len(), 1);
    }
}

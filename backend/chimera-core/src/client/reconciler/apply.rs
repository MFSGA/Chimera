//! Shared ref S4 execution and post-apply effects.
use super::{
    attempt::OwnershipTransitionPending,
    authority::RuntimeSourceAuthority,
    committed::{Selection, SelectionChange},
    inputs::RuntimeInputs,
    mutation::{AppliedCandidate, ApplyOutcome},
    plan::Trigger,
    preparation::RuntimePreparation,
};
use crate::{
    actor_v2::facade::{ReconcileReport, ReconcileResult},
    client::{
        core_lifecycle::ports::{
            CoreLifecyclePort, PreparedCoreSettlement, PreparedRuntime, RuntimePreparationPort,
        },
        runtime::RuntimeSnapshot,
    },
};
use chimera_contracts::runtime_error::{CoreFailure, RuntimeError};
use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::api::core::v2::{OperationOutputInfo, ReconcileOutcomeKind};
use std::{future::Future, sync::Arc};

/// Ref's exact source-carrying conversion (the Core manager owns retryable
/// and operation_id; the application must never guess either).
fn core_failure(error: CoreError) -> CoreFailure {
    CoreFailure {
        kind: error.kind,
        message: error.message,
        retryable: error.retryable,
        operation_id: error.operation_id.map(|operation| operation.to_string()),
    }
}

/// Pinned ref's `PreparedCandidate` preserves the selected profile/mode and
/// interruption decision; it has not yet submitted anything to the Core.
pub struct PreparedCandidate {
    pub runtime: PreparedRuntime,
    pub selection: Selection,
    pub interruption: Option<SelectionChange>,
}

/// S4's production committed-input preparation: one frozen inputs snapshot
/// determines both the exact PreparedRuntime bytes and its selection/effect
/// policy. This is ref's prepare_candidate without consuming an uncommitted
/// Checked proposal: those already enter the same Core owner via its inbox.
/// The sole CoreLifecycle actor later submits this exact prepared product.
pub async fn prepare_committed_candidate(
    preparation: &RuntimePreparation,
    inputs: RuntimeInputs,
    previous: Option<&Selection>,
    expected_target: &str,
) -> anyhow::Result<(PreparedCandidate, chimera_core_manager::CoreSpec)> {
    anyhow::ensure!(
        inputs.target_key().as_deref() == Some(expected_target),
        "committed runtime inputs changed during prepared build capture"
    );
    let selection = Selection::of(&inputs);
    let interruption = selection.interruption(previous, &inputs.clash);
    let runtime = preparation.prepare_inputs(inputs).await?;
    anyhow::ensure!(
        runtime.target.as_deref() == Some(expected_target),
        "prepared runtime was built for a different source target"
    );
    let spec = preparation.core_spec(&runtime.snapshot.target_core)?;
    Ok((
        PreparedCandidate {
            runtime,
            selection,
            interruption,
        },
        spec,
    ))
}

/// Pinned ref `RuntimeReconciler::prepare_candidate` body with its owner
/// dependencies explicit, pending the single Actor mailbox cutover.
pub async fn prepare_candidate(
    preparation: &RuntimePreparation,
    checked: &mut Option<PreparedRuntime>,
    applied_selection: Option<&Selection>,
    inputs: RuntimeInputs,
) -> Result<PreparedCandidate, ApplyOutcome> {
    let selection = Selection::of(&inputs);
    let interruption = selection.interruption(applied_selection, &inputs.clash);
    let target = inputs.target_key();
    let runtime = match take_checked_for_target(checked, target.as_deref()) {
        Some(prepared) => prepared,
        None => {
            preparation
                .prepare_inputs(inputs)
                .await
                .map_err(|error| ApplyOutcome::Refused {
                    cause: Arc::new(error),
                })?
        }
    };
    preparation
        .core_spec(&runtime.snapshot.target_core)
        .map_err(|source| ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::ResolveCoreBinary { source }),
        })?;
    Ok(PreparedCandidate {
        runtime,
        selection,
        interruption,
    })
}

/// The exact `checked` reuse fence from pinned ref
/// `RuntimeReconciler::prepare_candidate`: use a prechecked document only when
/// its target is present and exactly matches fresh committed input identity.
/// Mismatches leave the checked candidate intact while the caller rebuilds;
/// they never turn a failed/absent check into an implicit success.
pub fn take_checked_for_target(
    checked: &mut Option<crate::client::core_lifecycle::ports::PreparedRuntime>,
    target: Option<&str>,
) -> Option<crate::client::core_lifecycle::ports::PreparedRuntime> {
    if target.is_some()
        && checked
            .as_ref()
            .is_some_and(|prepared| prepared.target.as_deref() == target)
    {
        checked.take()
    } else {
        None
    }
}

/// Ref Step::Apply is permission to submit one exact PreparedCandidate, not
/// permission to rebuild and submit different bytes through the old inline
/// Core path. A missing prepared candidate on a proven ref S4 step is a
/// dependency wait. Only a legacy/unproven planning route may use the old
/// adapter while the remaining migration is unfinished.
pub enum CommittedPreparedDispatch {
    Observed(anyhow::Result<()>),
    LegacyRecovery,
}

pub fn route_committed_prepared(
    ref_apply: bool,
    prepared: Option<anyhow::Result<()>>,
) -> CommittedPreparedDispatch {
    match prepared {
        Some(result) => CommittedPreparedDispatch::Observed(result),
        None if ref_apply => CommittedPreparedDispatch::Observed(Err(
            crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                "ref S4 Apply cannot prove a prepared candidate from the current committed source and selected Core; legacy Core submit is forbidden",
            )
            .into(),
        )),
        None => CommittedPreparedDispatch::LegacyRecovery,
    }
}

/// A one-shot post-Core connection effect waiting for the Actor's final
/// source-mailbox fence and Running verification. This does not own Core and
/// cannot be executed by the worker before Completed is accepted.
pub struct CheckedConnectionEffect {
    pub change: SelectionChange,
    pub replaced: Option<bool>,
    pub prior: anyhow::Result<Option<crate::actor_v2::api::ApiClient>>,
}

impl CheckedConnectionEffect {
    fn degradation(
        change: SelectionChange,
        message: String,
        cause: chimera_contracts::runtime::InterruptFailure,
    ) -> chimera_contracts::runtime::Degradation {
        use chimera_contracts::runtime::{Degradation, DegradationPhase, DegradationReason};
        Degradation {
            phase: DegradationPhase::SystemEffect,
            reason: match change {
                SelectionChange::Mode => DegradationReason::ModeInterruptionFailed { cause },
                SelectionChange::Profile => DegradationReason::ProfileInterruptionFailed { cause },
            },
            message,
            retryable: false,
        }
    }

    pub fn from_error(
        change: SelectionChange,
        error: anyhow::Error,
    ) -> chimera_contracts::runtime::Degradation {
        use crate::actor_v2::api::ApiError;
        use chimera_contracts::runtime::InterruptFailure;
        // Keep REF's typed API failure identity; avoid parsing error strings.
        let cause = error
            .downcast_ref::<ApiError>()
            .map(InterruptFailure::from)
            .unwrap_or(InterruptFailure::Unavailable);
        Self::degradation(change, format!("{error:#}"), cause)
    }

    /// An obsolete or unverified Core result must never close the old API;
    /// preserve its non-retryable one-shot effect as typed degradation.
    pub fn deferred(self, reason: &str) -> chimera_contracts::runtime::Degradation {
        Self::degradation(
            self.change,
            reason.to_owned(),
            chimera_contracts::runtime::InterruptFailure::Unavailable,
        )
    }

    pub async fn finish(self) -> Option<chimera_contracts::runtime::Degradation> {
        match (self.replaced, self.prior) {
            (Some(true), _) => None, // Replaced Core already retired connections.
            (Some(false), Ok(None)) => None, // No previous running Core instance.
            (Some(false), Ok(Some(api))) => {
                let result = crate::connections::interrupt_connections(
                    &api,
                    &crate::connections::ConnectionScope::All,
                )
                .await
                .map_err(anyhow::Error::from);
                result
                    .err()
                    .map(|error| Self::from_error(self.change, error))
            }
            (Some(false), Err(error)) => Some(Self::from_error(self.change, error)),
            (None, _) => Some(Self::degradation(
                self.change,
                "Core replacement outcome is unknown; connection interruption not attempted".into(),
                chimera_contracts::runtime::InterruptFailure::Unavailable,
            )),
        }
    }
}

/// Ref apply_prepared finishes a one-shot Mode/Profile connection change
/// only after the *single* CoreLifecycle mailbox has received its Core
/// terminal and verified the final committed source and Running receipt.
/// The source actor cannot finish this effect and stale apply results must
/// remain typed degradations, not accidentally disconnect a newer Core.
pub async fn finish_connection_effect(
    effect: CheckedConnectionEffect,
    result: &anyhow::Result<()>,
    verified_running: bool,
    source_advanced: bool,
) -> Option<chimera_contracts::runtime::Degradation> {
    if checked_mode_effect_can_finish(result, verified_running, source_advanced) {
        effect.finish().await
    } else {
        Some(effect.deferred(
            "Connection interruption was not settled: checked Core target was superseded or active runtime is unverified",
        ))
    }
}

/// A committed source CAS does not authorize a runtime effect. The Actor
/// must first establish a successful Core terminal, the applied Running
/// instance, and that no later source reached the same mailbox.
pub fn checked_mode_effect_can_finish(
    result: &anyhow::Result<()>,
    verified_running: bool,
    source_advanced: bool,
) -> bool {
    result.is_ok() && verified_running && !source_advanced
}

/// Ref's maintain(Retry) repairs the derived RuntimeProduct only when
/// the original committed target is still active. The authoritative Core
/// receipt remains owned and verified by the existing LocalRuntimeHost;
/// this maintenance must never submit runtime bytes or allocate a revision.
/// No automatic SourceCommitted/Tick replay of a stale pending product.
pub async fn retry_pending_product<Read, ReadFuture>(
    core: &(impl crate::client::core_lifecycle::ports::CoreLifecyclePort + ?Sized),
    trigger: Trigger,
    read_current_target: Read,
) -> anyhow::Result<bool>
where
    Read: FnOnce() -> ReadFuture,
    ReadFuture: Future<Output = anyhow::Result<String>>,
{
    if trigger != Trigger::Retry || core.outcome_uncertain() {
        return Ok(false);
    }
    let Some(pending) = core.pending_runtime_product_target() else {
        return Ok(false);
    };
    if !read_current_target()
        .await
        .is_ok_and(|current| current == pending)
    {
        return Ok(false);
    }
    core.republish_pending_runtime_product(&pending).await
}

/// The sole CoreLifecycle worker delegates its actual prepared submission
/// here, matching ref's RuntimeReconciler::submit_prepared boundary. This
/// function neither spawns an actor nor owns a second runtime session: the
/// caller supplies the existing CoreLifecyclePort and exact prepared bytes.
/// Return the independently observed upstream terminal alongside the legacy
/// Result so the mailbox can record the true Applied/Refused/Unknown outcome.
pub async fn submit_prepared(
    core: &(impl crate::client::core_lifecycle::ports::CoreLifecyclePort + ?Sized),
    prepared: PreparedRuntime,
    spec: chimera_core_manager::CoreSpec,
) -> (
    Option<ApplyOutcome>,
    anyhow::Result<crate::client::core_lifecycle::ports::PreparedCoreSettlement>,
) {
    match core.reconcile_prepared_submission(prepared, spec).await {
        Ok(verdict) => settle_prepared_submission(verdict),
        Err(error) => (None, Err(error)),
    }
}

/// Project ref's prepared Core terminal once, preserving the exact accepted
/// product and rolled-back/unknown evidence without fabricating an Applied
/// candidate from an unverified legacy port.
pub fn settle_prepared_submission(
    verdict: crate::client::core_lifecycle::ports::PreparedSubmission,
) -> (
    Option<ApplyOutcome>,
    anyhow::Result<crate::client::core_lifecycle::ports::PreparedCoreSettlement>,
) {
    use crate::client::core_lifecycle::ports::{PreparedRolledBack, PreparedSubmission};
    let mapped = prepared_submission_outcome(&verdict);
    let result = match verdict {
        PreparedSubmission::Applied(applied) => {
            if let Some(ApplyOutcome::Applied(candidate)) = mapped.as_ref() {
                if applied.replaced != Some(candidate.replaced)
                    || !applied
                        .product
                        .as_ref()
                        .is_some_and(|product| Arc::ptr_eq(product, &candidate.product))
                {
                    return (
                        mapped,
                        Err(anyhow::anyhow!(
                            "upstream AppliedCandidate is not the exact confirmed Core settlement"
                        )),
                    );
                }
            }
            Ok(applied)
        }
        PreparedSubmission::RolledBack(report) => {
            let reason = report
                .failed_apply
                .clone()
                .unwrap_or_else(|| "the host returned no rollback detail".into());
            Err(PreparedRolledBack { report, reason }.into())
        }
        PreparedSubmission::Unknown(error) => Err(error.into()),
    };
    (mapped, result)
}

// Pinned ref/client/reconciler/apply.rs::not_submitted_outcome.
// Only four dependency kinds are waiting, regardless of retryable hint.
pub fn not_submitted_outcome(error: CoreError) -> ApplyOutcome {
    let waiting = matches!(
        error.kind,
        Some(
            CoreErrorKind::RevisionConflict
                | CoreErrorKind::OperationConflict
                | CoreErrorKind::QueueFull
                | CoreErrorKind::BackendUnavailable
        )
    );
    let error = Arc::new(RuntimeError::ApplyRuntime {
        failure: core_failure(error),
    });
    if waiting {
        ApplyOutcome::Waiting(error)
    } else {
        ApplyOutcome::Refused { cause: error }
    }
}

pub fn replaced_runtime(report: &ReconcileReport) -> bool {
    matches!(
        &report.output,
        OperationOutputInfo::Reconciled(outcome)
            if matches!(
                outcome.outcome,
                ReconcileOutcomeKind::Started
                    | ReconcileOutcomeKind::Restarted
                    | ReconcileOutcomeKind::Switched
            )
    )
}

/// Ref's `submit_prepared` outcome projection against Chimera's existing Core
/// facade. This is a classification of evidence, *not* a submission, an ACK,
/// or a producer of the published runtime document.
/// The already-admitted CoreLifecycle route now returns the same typed
/// Applied/RolledBack/Unknown evidence as the upstream facade. Only a fully
/// confirmed host/status report may become an AppliedCandidate; legacy ports
/// with no router report intentionally yield None for their Applied arm.
pub fn prepared_submission_outcome(
    verdict: &crate::client::core_lifecycle::ports::PreparedSubmission,
) -> Option<ApplyOutcome> {
    use crate::client::core_lifecycle::ports::PreparedSubmission;
    match verdict {
        PreparedSubmission::Applied(settlement) => {
            let receipt = settlement.confirmed.as_ref()?;
            settlement
                .validate_identity(receipt.target.as_deref()?)
                .ok()?;
            let report = settlement.report.as_ref()?.as_ref().clone();
            let product = settlement.product.as_ref()?.clone();
            let replaced = settlement.replaced?;
            Some(ApplyOutcome::Applied(AppliedCandidate {
                replaced,
                product,
                report: Box::new(report),
            }))
        }
        PreparedSubmission::RolledBack(outcome) => Some(ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::CoreRolledBack {
                reason: outcome.failed_apply.clone(),
            }),
        }),
        PreparedSubmission::Unknown(error) => Some(ApplyOutcome::Unknown(Arc::new(
            RuntimeError::SubmissionUnobserved {
                failure: core_failure(error.clone()),
            },
        ))),
    }
}

pub fn submission_outcome(result: ReconcileResult, product: Arc<RuntimeSnapshot>) -> ApplyOutcome {
    match result {
        ReconcileResult::Reconciled(report) => ApplyOutcome::Applied(AppliedCandidate {
            replaced: replaced_runtime(&report),
            product,
            report: Box::new(report),
        }),
        ReconcileResult::RolledBack(report) => ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::CoreRolledBack {
                reason: report.failed_apply,
            }),
        },
        ReconcileResult::Unknown(uncertain) => {
            ApplyOutcome::Unknown(Arc::new(RuntimeError::SubmissionUnobserved {
                failure: core_failure(uncertain.error),
            }))
        }
        ReconcileResult::NotSubmitted(error) => not_submitted_outcome(error),
        ReconcileResult::Unchanged(error) => ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::ApplyRuntime {
                failure: core_failure(error),
            }),
        },
    }
}

/// Perform the actual checked submission. A newer source or uncertain
/// Core cannot settle its selection or one-shot connection effect.
pub async fn apply_checked(
    core: &(impl CoreLifecyclePort + ?Sized),
    prepared: PreparedRuntime,
    core_spec: chimera_core_manager::CoreSpec,
    domain: super::committed::SourceDomain,
    version: u64,
    authority: &dyn RuntimeSourceAuthority,
    interrupt_mode: bool,
    previous: Option<&Selection>,
    observe: impl FnOnce(Option<ApplyOutcome>) + Send,
) -> anyhow::Result<(
    Option<Selection>,
    Option<SelectionChange>,
    Option<anyhow::Result<Option<crate::actor_v2::api::ApiClient>>>,
    Option<bool>,
)> {
    let checked_target = prepared
        .target
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("checked runtime has no source target identity"))?
        .to_owned();
    async {
        let interruption = if interrupt_mode {
            // Source-owned raw Clash patch has already proved that its
            // durable transition changes mode and requests interruption.
            // Do not depend on a possibly absent startup selection:
            // the one-shot intent belongs to this exact Checked receipt.
            authority.verify(domain, version, &checked_target).await?;
            Some(SelectionChange::Mode)
        } else {
            authority
                .verify_interruption(domain, version, &checked_target, previous)
                .await?
        };
        // Capture only the pre-apply instance-bound controller. A failed
        // capture is a post-commit effect degradation, not a Core refusal.
        let source = if interruption.is_some() {
            Some(core.capture_interruption_source().await)
        } else {
            None
        };
        authority.verify(domain, version, &checked_target).await?;
        if core.outcome_uncertain() {
            return Err(OwnershipTransitionPending::waiting(
                "the Core outcome became uncertain before checked S4 submission",
            )
            .into());
        }
        let (mapped, result) = submit_prepared(core, prepared, core_spec).await;
        observe(mapped);
        let applied = result?;
        applied.validate_identity(&checked_target)?;
        // No stale source may be promoted into applied_selection, and
        // an obsolete Core result cannot settle current runtime effects.
        let selection = authority
            .verify_selection(domain, version, &checked_target)
            .await?;
        Ok::<_, anyhow::Error>((selection, interruption, source, applied.replaced))
    }
    .await
}

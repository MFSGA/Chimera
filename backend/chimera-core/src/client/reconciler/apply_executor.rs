//! Shared S4 selected-owner execution. The caller is the existing reconciler
//! worker; this module has no mailbox, host selection, or product writer.

use super::{
    apply::{CheckedConnectionEffect, apply_checked, submit_prepared},
    attempt::{CommittedTargetAdvanced, OwnershipTransitionPending},
    authority::RuntimeSourceAuthority,
    check::CandidateCheck,
    decision::{same_active_receipt, same_core_status, service_prepared_owner_proven},
    mutation::ApplyOutcome,
    startup_executor::StartupHost,
};
use crate::client::core_lifecycle::{
    ports::{CoreLifecyclePort, PreparedCoreSettlement, PreparedRuntime},
    status::RunType,
};
use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
use chimera_contracts::execution_host::ExecutionHost;
use chimera_ipc::api::status::CoreState;
use std::sync::Arc;

/// S4 borrows the same owner, progress and observations as S2/S3.
/// The host only stages terminal evidence for its existing Completed message.
pub trait ApplyHost: StartupHost {
    fn convergence_authority(&self) -> Option<&dyn RuntimeSourceAuthority>;
    fn convergence_candidate_check(&self) -> Option<&CandidateCheck>;
    fn stage_prepared_outcome(&self, outcome: Option<ApplyOutcome>);
    fn stage_connection_effect(&self, effect: CheckedConnectionEffect);
}

/// Preserve the Core's exact terminal verdict through the sole lower-host
/// operation registry. RolledBack is a completed refusal with the old
/// revision still running; Unknown has a Core operation ID and must not
/// be submitted again. Legacy ports keep their explicit Unobserved result.
async fn submit_prepared_verdict<H: ApplyHost + ?Sized>(
    lifecycle: &H,
    prepared: PreparedRuntime,
    spec: chimera_core_manager::CoreSpec,
) -> anyhow::Result<PreparedCoreSettlement> {
    let (mapped, result) = submit_prepared(lifecycle.core(), prepared, spec).await;
    // Only the single existing CoreLifecycle worker mutates the runtime.
    // Keep ref's observed terminal for the Actor's final source/receipt
    // fence, including rollback and unknown-operation evidence.
    lifecycle.stage_prepared_outcome(mapped);
    result
}

/// The sole Core owner's checked runtime execution. Carry the pre-apply
/// source-instance API and Core replacement verdict through this one
/// operation; never issue a second Core submit or interrupt a newly
/// started instance.
pub async fn apply_checked_runtime<H: ApplyHost + ?Sized>(
    lifecycle: &H,
    prepared: PreparedRuntime,
    core_spec: chimera_core_manager::CoreSpec,
    domain: crate::client::reconciler::committed::SourceDomain,
    version: u64,
    authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
    interrupt_mode: bool,
) -> anyhow::Result<()> {
    let previous = lifecycle.progress().lock().selection();
    let result = apply_checked(
        lifecycle.core(),
        prepared,
        core_spec,
        domain,
        version,
        authority.as_ref(),
        interrupt_mode,
        previous.as_ref(),
        |mapped| lifecycle.stage_prepared_outcome(mapped),
    )
    .await;
    match result {
        Ok((selection, interruption, source, replaced)) => {
            lifecycle.progress().lock().set_selection(selection);
            // The worker cannot know about source notices that reached
            // the Actor after its final read. Defer the one-shot mode
            // effect to Completed; only that mailbox can settle it.
            if let Some(change) = interruption {
                lifecycle.stage_connection_effect(CheckedConnectionEffect {
                    change,
                    replaced,
                    prior: source.unwrap_or(Ok(None)),
                });
            }
            Ok(())
        }
        Err(error) => {
            lifecycle.progress().lock().invalidate_selection();
            Err(error)
        }
    }
}

pub async fn reconcile_prepared_selected_with_policy<H: ApplyHost + ?Sized>(
    lifecycle: &H,
    target: &str,
    app: &ChimeraAppConfig,
    clash: &ClashConfig,
    run_type: RunType,
    deferred: bool,
) -> Option<anyhow::Result<()>> {
    if lifecycle.core().outcome_uncertain() {
        return None;
    }
    let authority = lifecycle.convergence_authority()?;
    let gate = lifecycle.convergence_candidate_check()?;
    let before = lifecycle.core().status().await.ok()?;
    if before.run_type != run_type {
        return None;
    }
    // An unverified Service probe is not proof of a stopped Core. This
    // check is independent from the Core router's status projection.
    let service_before = if run_type == RunType::Service {
        let (service, generation) = lifecycle.service_evidence().await;
        if !service_prepared_owner_proven(&before, &service) {
            return Some(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "Service owner is not Ready or does not match the selected Core state",
                )
                .into(),
            ));
        }
        Some((service, generation))
    } else {
        None
    };
    let current_receipt = lifecycle.core().active_runtime_receipt().await.ok()?;
    let (receipt, previous) = match &before.state {
        CoreState::Running => {
            let receipt = current_receipt?;
            let expected_host = match run_type {
                RunType::Normal => ExecutionHost::Local,
                RunType::Service => ExecutionHost::Service,
                RunType::Elevated => return None,
            };
            if receipt.host != expected_host {
                return None;
            }
            // Upstream selection.interruption accepts an unknown prior
            // selection. It must not block a valid live apply or invent
            // a Mode/Profile change when the old selection is unknown.
            (Some(receipt), lifecycle.progress().lock().selection())
        }
        CoreState::Stopped(_) if current_receipt.is_none() => (None, None),
        _ => return None,
    };

    let expected = match authority
        .verify_committed_prepared_target(target, previous.as_ref())
        .await
    {
        Ok(Some(expected)) => expected,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let result = async {
        let (candidate, core_spec) = if deferred {
            gate.prepare_deferred_committed_candidate(
                app.clone(), clash.clone(), target, previous.as_ref(),
            ).await?
        } else {
            gate.prepare_committed_candidate(
                app.clone(), clash.clone(), target, previous.as_ref(),
            ).await?
        };
        let current = authority
            .verify_committed_prepared_target(target, previous.as_ref())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "committed source authority cannot prove the prepared target"
                )
            })?;
        // ref's prepared S4 candidate carries selection and interruption
        // from the same frozen inputs as its product. The independent
        // source authority must agree before that candidate can submit.
        if current != expected
            || candidate.selection != expected.0
            || candidate.interruption != expected.1
        {
            return Err(anyhow::Error::new(CommittedTargetAdvanced).context(
                "committed selection or interruption policy advanced during runtime build",
            ));
        }
        let observed = lifecycle.core().status().await?;
        if let Some((service_before, generation_before)) = service_before.as_ref() {
            let (service_after, generation_after) = lifecycle.service_evidence().await;
            if generation_after != *generation_before
                || service_after != *service_before
                || !service_prepared_owner_proven(&observed, &service_after)
            {
                return Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "Service readiness or Core ownership changed while building the committed product",
                    )
                    .into(),
                );
            }
        }
        let latest_receipt = lifecycle.core().active_runtime_receipt().await?;
        let still_receipted = same_active_receipt(receipt.as_ref(), latest_receipt.as_ref());
        if !same_core_status(&before, &observed) || !still_receipted {
            return Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "selected Core owner changed while preparing the committed product",
                )
                .into(),
            );
        }
        let old_api = if candidate.interruption.is_some() {
            Some(lifecycle.core().capture_interruption_source().await)
        } else {
            None
        };
        let final_owner = lifecycle.core().status().await?;
        let final_receipt = lifecycle.core().active_runtime_receipt().await?;
        if lifecycle.core().outcome_uncertain()
            || !same_core_status(&before, &final_owner)
            || !same_active_receipt(receipt.as_ref(), final_receipt.as_ref())
        {
            return Err(OwnershipTransitionPending::waiting(
                "the selected Core changed immediately before S4 submission",
            ).into());
        }
        if authority.verify_committed_prepared_target(target, previous.as_ref()).await?
            .as_ref() != Some(&expected)
        {
            return Err(CommittedTargetAdvanced.into());
        }
        let applied = submit_prepared_verdict(lifecycle, candidate.runtime, core_spec)
            .await?;
        applied.validate_identity(target)?;
        let latest = authority
            .verify_committed_prepared_target(target, previous.as_ref())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("committed source authority was lost after Core apply")
            })?;
        if latest != expected {
            return Err(anyhow::Error::new(CommittedTargetAdvanced)
                .context("committed selection or policy advanced after prepared Core apply"));
        }
        lifecycle.progress().lock().set_selection(Some(candidate.selection));
        if let Some(change) = candidate.interruption {
            lifecycle.stage_connection_effect(CheckedConnectionEffect {
                change,
                replaced: applied.replaced,
                prior: old_api.unwrap_or(Ok(None)),
            });
        }
        Ok(())
    }
    .await;
    if result.is_err() {
        lifecycle.progress().lock().invalidate_selection();
    }
    Some(result)
}

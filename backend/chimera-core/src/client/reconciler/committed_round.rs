//! Shared S2/S3/S4 committed convergence executor.
//! Runs inside the existing reconciler worker, with ref's Facts planner.
//! The host adapter supplies the single Core owner and Service observations.

use super::{
    attempt::CommittedTargetAdvanced,
    decision::{same_core_status, service_prepared_owner_proven, stable_owner_facts},
    plan::Trigger,
    service_evidence::ServiceEvidence as ObservedServiceEvidence,
    startup::CommittedRound,
    startup_executor::StartupHost,
};
use crate::client::core_lifecycle::ports::CoreLifecyclePort;
use chimera_contracts::execution_host::ExecutionHost;
use chimera_ipc::api::status::CoreState;

/// Consume ref's stable S2/S3/S4 decision. A concrete Step::Apply,
/// optionally after a verified S3 Stop, is explicit admission; a settled
/// Idle/Owner/Refused can never fall through to legacy Core submission.
/// An unreadable observation is Waiting in a canonical session, not
/// permission for the compatibility recovery to submit Core.
/// Ownership moves remain with the one startup handoff actor.
pub async fn planned_committed_round<H: StartupHost + ?Sized>(
    lifecycle: &H,
    target: &str,
    host: ExecutionHost,
    trigger: Trigger,
) -> anyhow::Result<CommittedRound> {
    // The worker must never use a legacy adapter to sidestep a
    // lower-Core uncertain operation. A different command's legacy
    // trigger still keeps its existing compatibility recovery path.
    if lifecycle.core().outcome_uncertain() {
        return Ok(CommittedRound::Settled(Err(
            crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                "the Core submit outcome is unknown; recover its owner before another Apply",
            )
            .into(),
        )));
    }
    let canonical_session = lifecycle.canonical_convergence_session();
    if !crate::client::reconciler::startup::uses_committed_convergence(trigger, canonical_session) {
        return Ok(CommittedRound::LegacyRecovery);
    }
    if canonical_session && !lifecycle.startup_service_command_settled().await {
        return Ok(CommittedRound::Settled(Err(
            crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                "a service command started earlier is still running",
            )
            .into(),
        )));
    }
    // Ref converge must re-sample Facts after an S3 Stop. The old
    // decision was about a Running instance and cannot authorize S4.
    // At most one stop/replan is allowed in this CoreLifecycle command.
    let mut stopped_for_replacement = false;
    for _ in 0..2 {
        let observed = match lifecycle.core().status().await {
            Ok(observed) => observed,
            Err(_) if !canonical_session && !stopped_for_replacement => {
                return Ok(CommittedRound::LegacyRecovery);
            }
            Err(_) => {
                return Ok(CommittedRound::Settled(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the Core is unreadable; obtain a stable observation before S4",
                    )
                    .into(),
                )));
            }
        };
        let receipt = match lifecycle.core().active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(_) if !canonical_session && !stopped_for_replacement => {
                return Ok(CommittedRound::LegacyRecovery);
            }
            Err(_) => {
                return Ok(CommittedRound::Settled(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the Core receipt is unreadable; obtain a stable observation before S4",
                    )
                    .into(),
                )));
            }
        };
        if stopped_for_replacement
            && !crate::client::reconciler::decision::s3_replanned_stopped_owner_ready(
                &observed,
                receipt.as_deref(),
                host,
            )
        {
            return Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "S3 Stop did not leave a stable selected Stopped Core with no active receipt",
                )
                .into(),
            )));
        }
        // ref startup::converge derives CoreFact::Running.verified from
        // a matching live receipt, not from its equality to the desired
        // target. The receipt's own target is a separate Facts.applied.
        // active_runtime_receipt performs the complete runtime proof;
        // the status/receipt samples below fence its identity.
        let verified = if canonical_session {
            matches!(observed.state, CoreState::Running)
                && receipt.as_deref().is_some_and(|receipt| {
                    crate::client::reconciler::proof::receipt_matches_owner(
                        receipt,
                        crate::client::reconciler::proof::host_for(observed.run_type),
                    )
                })
        } else if receipt.is_some() && matches!(observed.state, CoreState::Running) {
            // Preserve the old semantics only for pre-canonical ports.
            super::proof::verified_applied_runtime(
                lifecycle.core(),
                || lifecycle.startup_target_identity(),
                &observed,
            )
            .await
        } else {
            false
        };
        let previous = lifecycle.progress().lock().attempt();
        // Read the same ServiceActor-owned evidence as StartupWorkflow. Its
        // epoch/readiness and the selected Core state form ONE ref Facts round
        // instead of permitting S3 to bypass a pending S2 owner transition.
        let (service, service_generation) = lifecycle.service_evidence().await;
        // Both selected Core and Service ownership must form one coherent
        // S2/S3 Facts frame. Async probes can cross a daemon restart even if
        // they return two individually valid Ready snapshots.
        let resampled_core = lifecycle.core().status().await.ok();
        let (resampled_service, resampled_generation) = lifecycle.service_evidence().await;
        let final_core = lifecycle.core().status().await.ok();
        let coherent = resampled_core.as_ref().is_some_and(|current| {
            stable_owner_facts(
                &observed,
                current,
                &service,
                service_generation,
                &resampled_service,
                resampled_generation,
            )
        }) && final_core
            .as_ref()
            .is_some_and(|current| same_core_status(&observed, current));
        if !coherent {
            return Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "Core or Service ownership changed while sampling the committed Facts round",
                )
                .into(),
            )));
        }
        // The accepted receipt (or its absence) is part of the same
        // S2/S3 Facts sample, including before the first S3 Stop.
        match lifecycle.core().active_runtime_receipt().await {
            Ok(current)
                if crate::client::reconciler::decision::same_active_receipt(
                    receipt.as_ref(),
                    current.as_ref(),
                ) => {}
            _ => {
                return Ok(CommittedRound::Settled(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the accepted Core receipt changed or became unreadable while sampling ownership",
                    )
                    .into(),
                )));
            }
        }
        if host == ExecutionHost::Service
            && matches!(service, ObservedServiceEvidence::ForeignCore { .. })
        {
            return Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "Service belongs to another instance; Local owner selection is required",
                )
                .into(),
            )));
        }
        if canonical_session {
            let desired = super::workflow::desired_host(
                lifecycle.startup_application_config().enable_service_mode,
                &service,
                service_generation,
                lifecycle.startup_failed_generation(),
                super::proof::host_for(observed.run_type),
            );
            if desired != host {
                return Ok(CommittedRound::Settled(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the latest Service readiness or host preference changed during S2 sampling",
                    ).into()
                )));
            }
            if host == ExecutionHost::Service
                && super::proof::host_for(observed.run_type) == ExecutionHost::Service
                && !service_prepared_owner_proven(&observed, &service)
            {
                return Ok(CommittedRound::Settled(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the selected Service runtime owner is not proven",
                    )
                    .into(),
                )));
            }
        }
        let step = crate::client::reconciler::decision::plan_committed_target(
            target,
            host,
            &observed,
            super::service_evidence::reconciler_service_evidence(&service),
            lifecycle.service_recovery_blocks_handoff(),
            receipt.as_deref(),
            verified,
            previous,
            trigger,
        );
        use crate::client::reconciler::decision::{CommittedStepAction, classify_committed_step};
        let Some(step) = step else {
            if canonical_session || stopped_for_replacement {
                return Ok(CommittedRound::Settled(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the latest S2/S3/S4 Facts plan is still unproven",
                    )
                    .into(),
                )));
            }
            return Ok(CommittedRound::LegacyRecovery);
        };
        return match classify_committed_step(step) {
            CommittedStepAction::Apply => {
                // Ref plans a snapshot of committed Facts. A newer source
                // could have arrived during the asynchronous Service probe:
                // the stable owner cannot submit that obsolete target.
                if lifecycle.startup_target_identity().await? != target {
                    return Err(CommittedTargetAdvanced.into());
                }
                if lifecycle.core().outcome_uncertain() {
                    return Ok(CommittedRound::Settled(Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "the Core result became uncertain after S4 admission",
                        )
                        .into(),
                    )));
                }
                Ok(CommittedRound::Apply)
            }
            CommittedStepAction::Converged => {
                // A verified receipt belongs to its actual source target.
                // Do not claim a stale captured target converged when
                // another source commit arrived during owner probing.
                if lifecycle.startup_target_identity().await? != target {
                    return Err(CommittedTargetAdvanced.into());
                }
                if lifecycle.core().outcome_uncertain() {
                    return Ok(CommittedRound::Settled(Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "the Core outcome became uncertain after convergence planning",
                        )
                        .into(),
                    )));
                }
                Ok(CommittedRound::Settled(Ok(())))
            }
            CommittedStepAction::Owner(plan) => Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::owner(plan).into(),
            ))),
            CommittedStepAction::Wait(cause) => Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    cause.to_string(),
                )
                .into(),
            ))),
            CommittedStepAction::Refused(cause) => Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::PlannedTargetRefused { cause }.into(),
            ))),
            CommittedStepAction::Blocked(reason) => Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(reason)
                    .into(),
            ))),
            CommittedStepAction::StopForReplacement => {
                // Compatibility ports cannot yet build the canonical
                // PreparedCandidate after stopping an unreceipted Core.
                // Keep their old recovery path until those ports migrate.
                if !canonical_session && receipt.is_none() && !stopped_for_replacement {
                    return Ok(CommittedRound::LegacyRecovery);
                }
                if stopped_for_replacement {
                    return Ok(CommittedRound::Settled(Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "Core still requires S3 Stop after the first confirmed stop",
                        )
                        .into(),
                    )));
                }
                // After a confirmed S3 terminal, ref converge loops through
                // the *new* Core/Service Facts rather than submitting using
                // the previous Running instance's stale plan.
                let startup_service = (trigger == Trigger::Startup
                    && host == ExecutionHost::Service)
                    .then_some((&service, service_generation));
                let stopped =
                    crate::client::core_lifecycle::ownership::stop_planned_unverified_core(
                        lifecycle.core(),
                        || lifecycle.startup_target_identity(),
                        || lifecycle.service_evidence(),
                        || lifecycle.progress().lock().invalidate_selection(),
                        target,
                        &observed,
                        receipt.as_ref(),
                        startup_service,
                    )
                    .await;
                if let Err(error) = stopped {
                    if startup_service.is_some() && !lifecycle.core().outcome_uncertain() {
                        return Err(crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            format!("Service S3 did not establish a stable terminal: {error:#}"),
                        ).into());
                    }
                    return Err(error);
                }
                stopped_for_replacement = true;
                continue;
            }
            CommittedStepAction::FallBackToLocal => {
                let refusal =
                    anyhow::anyhow!("previously refused committed Service target: {target}");
                if trigger == Trigger::Startup {
                    // ApplicationWorkflow already holds the Service lease.
                    // Its single existing handback branch handles this
                    // positively refused target, without recursive locking.
                    Ok(CommittedRound::Settled(Err(refusal)))
                } else {
                    Ok(CommittedRound::Settled(
                        lifecycle
                            .fallback_after_service_refusal(target, refusal)
                            .await,
                    ))
                }
            }
        };
    }
    Ok(CommittedRound::Settled(Err(
        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
            "S3/S4 converge exhausted the bounded owner replan",
        )
        .into(),
    )))
}

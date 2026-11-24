//! Ref startup convergence: stable S2/S3/S4 observations and execution.
//! The existing CoreLifecycle mailbox remains the sole mutation owner.
use std::sync::Arc;

use chimera_contracts::{execution_host::ExecutionHost, runtime_error::RuntimeError};
use chimera_core::{
    actor_v2::CoreStatusProjection,
    client::reconciler::plan::{Conclusion, ServiceEvidence, Trigger},
};
use chimera_core_manager::OperationId;
use chimera_ipc::api::status::CoreState;

use crate::client::{
    application_workflow::startup::{
        ServiceEvidence as ObservedServiceEvidence, startup_stop_error_is_idempotent,
    },
    convergence::ConvergenceHealth,
    core_lifecycle::{CoreLifecycleWorkflow, ports::CoreStatusSnapshot},
    reconciler::{
        attempt::CommittedTargetAdvanced,
        decision::{
            confirmed_planned_stop, same_core_status, service_prepared_owner_proven,
            stable_owner_facts,
        },
    },
};

pub use chimera_core::client::reconciler::startup::*;

impl CoreLifecycleWorkflow {
    /// Ref S3 Stop must observe a settled, stopped Core before S4 can
    /// apply the next committed target. A successful command reply alone
    /// cannot prove that the selected endpoint stopped, nor that the
    /// sources or accepted Core revision stayed fixed across the stop.
    ///
    /// The existing CoreLifecycle actor remains the only executor; no new
    /// Runtime writer or host-selection operation is introduced here.
    async fn stop_planned_unverified_core(
        &self,
        target: &str,
        observed: &CoreStatusSnapshot,
        accepted: Option<&Arc<crate::client::runtime::RuntimeApplyReceipt>>,
        startup_service: Option<(&ObservedServiceEvidence, u64)>,
    ) -> anyhow::Result<()> {
        use crate::client::reconciler::attempt::OwnershipTransitionPending;

        // The old source may have advanced while S2/S3 sampled its owner.
        // Do not stop a real Core for a candidate no longer committed.
        if self.startup_target_identity().await? != target {
            return Err(CommittedTargetAdvanced.into());
        }
        let before = self.convergence_core().status().await?;
        if !same_core_status(observed, &before) || !matches!(before.state, CoreState::Running) {
            return Err(OwnershipTransitionPending::waiting(
                "the selected Core changed before its S3 stop could begin",
            )
            .into());
        }
        let current = self
            .convergence_core()
            .active_runtime_receipt()
            .await
            .map_err(|_| {
                OwnershipTransitionPending::waiting(
                    "the accepted Core receipt became unreadable before S3 Stop",
                )
            })?;
        if accepted.is_some_and(|receipt| {
            receipt.host != crate::client::reconciler::proof::host_for(before.run_type)
        }) || !crate::client::reconciler::decision::same_active_receipt(
            accepted,
            current.as_ref(),
        ) {
            return Err(OwnershipTransitionPending::waiting(
                "the accepted Core revision changed before the S3 stop",
            )
            .into());
        }
        if self.convergence_core().outcome_uncertain() {
            return Err(OwnershipTransitionPending::waiting(
                "the Core outcome became uncertain before S3 Stop",
            )
            .into());
        }
        let stop = if let Some((expected, generation)) = startup_service {
            let (fresh, fresh_generation) = self.service_evidence().await;
            if &fresh != expected
                || fresh_generation != generation
                || !same_core_status(&before, &self.convergence_core().status().await?)
                || self.startup_target_identity().await? != target
                || self.outcome_uncertain()
            {
                return Err(OwnershipTransitionPending::waiting(
                    "the Service owner changed before its conditional S3 retirement",
                )
                .into());
            }
            let final_receipt = self
                .convergence_core()
                .active_runtime_receipt()
                .await
                .map_err(|_| {
                    OwnershipTransitionPending::waiting(
                        "the Service receipt became unreadable before S3 retirement",
                    )
                })?;
            if !crate::client::reconciler::decision::same_active_receipt(
                accepted,
                final_receipt.as_ref(),
            ) {
                return Err(OwnershipTransitionPending::waiting(
                    "the Service receipt changed before its conditional S3 retirement",
                )
                .into());
            }
            self.retire_service_startup_owner(&fresh, true)
                .await
                .map(|_| ())
                .map_err(|failure| {
                    // S3 failure must not become an S4 Service refusal and authorize fallback.
                    anyhow::Error::new(OwnershipTransitionPending::waiting(format!(
                        "Service S3 retirement did not settle: {:?}",
                        failure.outcome()
                    )))
                })
        } else {
            self.convergence_core().stop().await
        };
        if self.convergence_core().outcome_uncertain() {
            anyhow::bail!("S3 Core stop outcome is uncertain; do not apply a second runtime");
        }
        let stopped = self.convergence_core().status().await?;
        if !confirmed_planned_stop(&before, &stopped) {
            // A failed Stop RPC retains its actual error; a successful reply
            // without a stopped observation is a dependency wait.
            if let Err(error) = stop {
                return Err(error);
            }
            return Err(OwnershipTransitionPending::waiting(
                "the Core stop reply had no stable stopped-state proof",
            )
            .into());
        }
        // Ref stop_owner permits NotStarted only after independent stopped
        // proof. Every other error must retain the original refusal.
        if let Err(error) = stop
            && !startup_stop_error_is_idempotent(&error)
        {
            return Err(error);
        }
        let confirmed = self.convergence_core().status().await?;
        if !same_core_status(&stopped, &confirmed) {
            return Err(OwnershipTransitionPending::waiting(
                "the selected Core changed while confirming its S3 stop",
            )
            .into());
        }
        self.convergence_progress().lock().invalidate_selection();
        // The stop is now proven, but the previously captured source target
        // could have been superseded while the external Core action ran.
        // In that case only a latest-source round may perform the next Apply.
        if self.startup_target_identity().await? != target {
            return Err(CommittedTargetAdvanced.into());
        }
        Ok(())
    }

    /// Consume ref's stable S2/S3/S4 decision. A concrete Step::Apply,
    /// optionally after a verified S3 Stop, is explicit admission; a settled
    /// Idle/Owner/Refused can never fall through to legacy Core submission.
    /// An unreadable observation is Waiting in a canonical session, not
    /// permission for the compatibility recovery to submit Core.
    /// Ownership moves remain with the one startup handoff actor.
    pub(in crate::client) async fn planned_committed_round(
        &self,
        target: &str,
        host: ExecutionHost,
        trigger: Trigger,
    ) -> anyhow::Result<CommittedRound> {
        // The worker must never use a legacy adapter to sidestep a
        // lower-Core uncertain operation. A different command's legacy
        // trigger still keeps its existing compatibility recovery path.
        if self.convergence_core().outcome_uncertain() {
            return Ok(CommittedRound::Settled(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "the Core submit outcome is unknown; recover its owner before another Apply",
                )
                .into(),
            )));
        }
        let canonical_session = self.canonical_convergence_session();
        if !crate::client::reconciler::startup::uses_committed_convergence(
            trigger,
            canonical_session,
        ) {
            return Ok(CommittedRound::LegacyRecovery);
        }
        if canonical_session && !self.startup_service_command_settled().await {
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
            let observed = match self.convergence_core().status().await {
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
            let receipt = match self.convergence_core().active_runtime_receipt().await {
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
                self.verified_applied_runtime(&observed).await
            } else {
                false
            };
            let previous = self.convergence_progress().lock().attempt();
            // Read the same ServiceActor-owned evidence as StartupWorkflow. Its
            // epoch/readiness and the selected Core state form ONE ref Facts round
            // instead of permitting S3 to bypass a pending S2 owner transition.
            let (service, service_generation) = self.service_evidence().await;
            // Both selected Core and Service ownership must form one coherent
            // S2/S3 Facts frame. Async probes can cross a daemon restart even if
            // they return two individually valid Ready snapshots.
            let resampled_core = self.convergence_core().status().await.ok();
            let (resampled_service, resampled_generation) = self.service_evidence().await;
            let final_core = self.convergence_core().status().await.ok();
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
            match self.convergence_core().active_runtime_receipt().await {
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
                    self.startup_application_config().enable_service_mode,
                    &service,
                    service_generation,
                    self.startup_failed_generation(),
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
                crate::client::application_workflow::startup::reconciler_service_evidence(&service),
                self.service_recovery_blocks_handoff(),
                receipt.as_deref(),
                verified,
                previous,
                trigger,
            );
            use crate::client::reconciler::decision::{
                CommittedStepAction, classify_committed_step,
            };
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
                    if self.startup_target_identity().await? != target {
                        return Err(CommittedTargetAdvanced.into());
                    }
                    if self.convergence_core().outcome_uncertain() {
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
                    if self.startup_target_identity().await? != target {
                        return Err(CommittedTargetAdvanced.into());
                    }
                    if self.convergence_core().outcome_uncertain() {
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
                    crate::client::reconciler::attempt::OwnershipTransitionPending::owner(plan)
                        .into(),
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
                    let stopped = self
                        .stop_planned_unverified_core(
                            target,
                            &observed,
                            receipt.as_ref(),
                            startup_service,
                        )
                        .await;
                    if let Err(error) = stopped {
                        if startup_service.is_some() && !self.outcome_uncertain() {
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
                            self.fallback_after_service_refusal(target, refusal).await,
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
}

use crate::client::reconciler::attempt::ServiceFallbackDeferred;
use chimera_config::application::ChimeraAppConfig;

impl CoreLifecycleWorkflow {
    /// Startup S2/S3 established and, if necessary, retired the Local Core
    /// before reaching here. Reuse the exact committed-source preparation
    /// instead of the old inline YAML construction whenever the sole Core
    /// owner and authoritative source reader are available.
    pub(in crate::client) async fn try_startup_prepared_selected(
        &self,
        app: &ChimeraAppConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        if self.profiles.is_none()
            || self.checked_gate.get().is_none()
            || self.source_authority.is_none()
        {
            return None;
        }
        let clash = match self.clash.get() {
            Ok(clash) => clash,
            Err(error) => return Some(Err(error)),
        };
        let (target, app, clash, ..) = match self
            .capture_reconcile_committed_target(app.clone(), clash)
            .await
        {
            Ok(committed) => committed,
            Err(error) => return Some(Err(error)),
        };
        match run_type {
            crate::core::RunType::Normal => {
                self.try_reconcile_prepared_local(&target, &app, &clash, run_type)
                    .await
            }
            crate::core::RunType::Service => {
                self.try_reconcile_prepared_service(&target, &app, &clash)
                    .await
            }
            crate::core::RunType::Elevated => None,
        }
    }

    /// Recovery after S2 proved the selected owner. Reuse the normal ref
    /// committed S3/S4 planner, without a new lease or host selection.
    /// None is reserved for the pre-canonical compatibility path.
    pub(in crate::client) async fn recover_committed_startup(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> Option<anyhow::Result<()>> {
        if self.profiles.is_none()
            || self.checked_gate.get().is_none()
            || self.source_authority.is_none()
            || observed.run_type == crate::core::RunType::Elevated
            || observed.state_changed_at == 0
        {
            return None;
        }
        let host = crate::client::reconciler::proof::host_for(observed.run_type);
        let result = async {
            let clash = self.clash.get()?;
            let (target, app, clash, ..) = self
                .capture_reconcile_committed_target(self.application.get_typed(), clash)
                .await?;
            if !same_core_status(observed, &self.core.status().await?) {
                return Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "the S2-proven owner changed before startup convergence",
                    )
                    .into(),
                );
            }
            let result = match self
                .planned_committed_round(&target, host, Trigger::Startup)
                .await?
            {
                CommittedRound::Settled(result) => result,
                CommittedRound::Apply => {
                    self.progress
                        .lock()
                        .allow(&target, host, Trigger::Startup)?;
                    self.progress.lock().stage(target.clone(), host);
                    self.try_reconcile_prepared_selected(&target, &app, &clash, observed.run_type)
                        .await
                        .unwrap_or_else(|| {
                            Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "the S4-admitted startup candidate lost its owner or source proof",
                        ).into()
                    )
                        })
                }
                CommittedRound::LegacyRecovery => Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "canonical startup recovery has no settled S3/S4 plan",
                    )
                    .into(),
                ),
            };
            if self.startup_target_identity().await? != target {
                return Err(CommittedTargetAdvanced.into());
            }
            result
        }
        .await;
        if result.is_err() {
            self.progress.lock().invalidate_selection();
        }
        self.refresh_service_recovery_host().await;
        Some(result)
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
        } else {
            self.reconcile_application(app, service_connected).await
        };
        self.refresh_service_recovery_host().await;
        result
    }

    pub(in crate::client) async fn startup_reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
    ) -> anyhow::Result<()> {
        let result = self.reconcile_application_after_service_release(app).await;
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
        // A Service-to-Local fallback is authorized only by a *refused*
        // Service submission. Unknown and Waiting outcomes cannot prove
        // the Service owner stopped safely and must retain the original
        // reason for the single Actor to retry or recover.
        if !matches!(
            crate::client::reconciler::attempt::legacy_error_verdict(
                &refusal,
                self.core.outcome_uncertain(),
                false,
            ),
            crate::client::reconciler::attempt::LegacyApplyVerdict::Refused(_)
        ) {
            return Err(refusal);
        }
        if !self.startup_service_refusal_stopped().await {
            return Err(refusal);
        }

        // Keep the nonblocking Service transition lease across both release
        // and the Local apply. Dropping it between those operations would let
        // another owner start Service after the release proof but before Local
        // finished, violating ref's single-owner handoff ordering.
        let _transition =
            self.try_begin_service_transition()
                .await
                .map_err(|error| ServiceFallbackDeferred {
                    reason: format!(
                        "{refusal}; cannot acquire Service transition lease: {error:#}"
                    ),
                })?;
        let (_, generation) = self.service_evidence().await;
        let released =
            self.release_service_owner()
                .await
                .map_err(|reason| ServiceFallbackDeferred {
                    reason: format!("{refusal}; {reason}"),
                })?;
        if !released.proves_no_owned_core() {
            return Err(ServiceFallbackDeferred {
                reason: format!(
                    "{refusal}; no fresh evidence that Service released its owned Core"
                ),
            }
            .into());
        }
        // Only suppress Service for this readiness generation after its Core
        // release has been proved. A failed or unknown release MUST NOT make
        // an ordinary Local start eligible.
        if generation != 0 {
            self.set_startup_failed_generation(generation);
        }
        if !self
            .startup_target_identity()
            .await
            .is_ok_and(|current| current == target)
        {
            return Err(CommittedTargetAdvanced.into());
        }

        // This fallback has proved the old Service Core is released. Its
        // Local submission supersedes the staged Service attempt; Completed
        // alone may record the observed Local terminal.
        self.progress
            .lock()
            .stage(target.to_owned(), ExecutionHost::Local);
        let local = self
            .reconcile_application_after_service_release(self.application.get_typed())
            .await;
        self.refresh_service_recovery_host().await;
        if !self
            .startup_target_identity()
            .await
            .is_ok_and(|current| current == target)
        {
            return Err(CommittedTargetAdvanced.into());
        }
        local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_start_and_core_change_join_the_canonical_convergence_round() {
        for trigger in [Trigger::Start, Trigger::CoreChanged, Trigger::Startup] {
            assert!(uses_committed_convergence(trigger, true));
            assert!(!uses_committed_convergence(trigger, false));
        }
        for trigger in [Trigger::SourceCommitted, Trigger::Retry] {
            assert!(uses_committed_convergence(trigger, true));
            assert!(uses_committed_convergence(trigger, false));
        }
        for trigger in [
            Trigger::Stop,
            Trigger::ServiceOperation,
            Trigger::PrepareService,
        ] {
            assert!(!uses_committed_convergence(trigger, true));
        }
    }

    #[test]
    fn ref_startup_projection_distinguishes_all_reconciler_conclusions() {
        assert!(matches!(
            StartupOutcome::from_conclusion(&Conclusion::Converged),
            StartupOutcome::Ready
        ));
        assert!(matches!(
            StartupOutcome::from_conclusion(&Conclusion::Inactive),
            StartupOutcome::Ready
        ));
        let waiting = Arc::new(RuntimeError::new("service is not ready"));
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Waiting {cause: waiting}),
            StartupOutcome::ReadyDegraded {health: ConvergenceHealth::WaitingDependency, reason} if reason.contains("service is not ready"))
        );
        let refused = Arc::new(RuntimeError::CoreRejectedConfig {
            core_kind: None,
            message: "candidate invalid".into(),
        });
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::NotInEffect {cause: refused}),
            StartupOutcome::ReadyDegraded {health: ConvergenceHealth::Blocked, reason} if reason.contains("candidate invalid"))
        );
        let unknown = Arc::new(RuntimeError::new("result could not be observed"));
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Unknown {cause: unknown}),
            StartupOutcome::RecoveryRequired {reason} if reason.contains("result could not be observed"))
        );
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Closed),
            StartupOutcome::RecoveryRequired {reason} if reason == RuntimeError::ShuttingDown.to_string())
        );
    }

    #[test]
    fn ref_startup_cause_keeps_the_exact_typed_reason() {
        let cause = cause("service owner not established".into());
        assert!(matches!(
            cause.as_ref(),
            RuntimeError::ConfigurationNotInEffect { .. }
        ));
        assert!(cause.to_string().contains("service owner not established"));
    }
}

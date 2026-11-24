//! Serialized committed convergence dispatch over the sole lifecycle owner.
use chimera_contracts::execution_host::ExecutionHost;
use chimera_core::client::reconciler::plan::Trigger;

use super::{
    apply::finish_connection_effect,
    attempt::OwnershipTransitionPending,
    proof::host_for,
    startup::{CommittedRound, uses_committed_convergence},
};
use crate::client::core_lifecycle::{Command, CoreLifecycleWorkflow};

pub use chimera_core::client::reconciler::workflow::{
    AdmissionCommand, CheckedSourceNoticeKind, ProfilesSourceAdvanced, RuntimeAdmission,
    RuntimeReconcilerState, checked_mode_effect_deferred, committed_target_advanced,
    deferred_mode_effect_degradation, desired_host, source_committed_during_round,
};
pub fn admission_command(command: &Command) -> AdmissionCommand {
    match command {
        Command::StopCore => AdmissionCommand::Stop,
        Command::StartCore => AdmissionCommand::Start,
        Command::ApplyChecked {
            domain,
            version,
            prepared,
            ..
        } => AdmissionCommand::Checked {
            domain: *domain,
            version: *version,
            has_target: prepared.target.is_some(),
        },
        Command::Reconcile
        | Command::ReconcileSource
        | Command::ReconcileProfiles { .. }
        | Command::RetryDeferredProfiles { .. }
        | Command::RecoverCore
        | Command::RecoverStartup { .. }
        | Command::RetryStartup { .. }
        | Command::FollowService { .. } => AdmissionCommand::Automatic,
        _ => AdmissionCommand::Other,
    }
}

pub(in crate::client) fn runtime_trigger(command: &Command) -> &'static str {
    match command {
        Command::StartCore => "Start",
        Command::StopCore => "Stop",
        Command::Shutdown => "Shutdown",
        Command::Reconcile => "Retry",
        Command::ReconcileSource | Command::ApplyChecked { .. } => "SourceCommitted",
        Command::MaintainServiceRelease => "ServiceRelease",
        Command::StartupReconcile { .. } => "Startup",
        Command::RetryStartup { .. } => "Retry",
        Command::FollowService { .. } => "ServiceReadiness",
        Command::ReplaceCoreBinary(_) => "BinaryReplaced",
        Command::ProfileMutation(_) => "Mutation",
        Command::RecoverCore => "CoreChanged",
        Command::RetryDeferredProfiles { .. } => "Tick",
        _ => "ServiceOperation",
    }
}

impl CoreLifecycleWorkflow {
    /// Treat Service failure for one readiness generation as a Local
    /// preference only after the previous Service Core was safely released.
    async fn committed_run_type(
        &self,
        enable_service_mode: bool,
        trigger: Trigger,
    ) -> anyhow::Result<crate::core::RunType> {
        use crate::core::RunType;
        if self.canonical_convergence_session() && uses_committed_convergence(trigger, true) {
            let observed = self.convergence_core().status().await.map_err(|error| {
                OwnershipTransitionPending::waiting(format!(
                    "cannot observe selected owner: {error:#}"
                ))
            })?;
            let (service, generation) = self.service_evidence().await;
            let failed = self.startup_failed_generation();
            let host = desired_host(
                enable_service_mode,
                &service,
                generation,
                failed,
                host_for(observed.run_type),
            );
            return Ok(match host {
                ExecutionHost::Local => RunType::Normal,
                ExecutionHost::Service => RunType::Service,
            });
        }

        let preferred = RunType::classify(
            enable_service_mode,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        );
        if preferred != RunType::Service {
            return Ok(preferred);
        }
        let failed_generation = self.startup_failed_generation();
        if failed_generation == 0 {
            return Ok(preferred);
        }
        let (_, current_generation) = self.service_evidence().await;
        Ok(if failed_generation == current_generation {
            RunType::Normal
        } else {
            preferred
        })
    }

    pub(in crate::client) async fn reconcile_with_trigger(
        &self,
        trigger: Trigger,
    ) -> anyhow::Result<()> {
        // Ref maintain(Retry) lives with S4 apply outcome maintenance.
        // Only repair the accepted receipt's derived file, and never replay
        // old Core bytes or publish a product after its source has advanced.
        if crate::client::reconciler::apply::retry_pending_product(
            self.convergence_core(),
            trigger,
            || self.startup_target_identity(),
        )
        .await?
        {
            return Ok(());
        }
        let clash = self.convergence_clash()?;
        let app = self.startup_application_config();
        let target_core = app.core;
        if let Some(before) = self.convergence_profiles_version() {
            // Capture both the authoritative committed document and its managed
            // file bytes under the same publication fence, as in ref.
            let (target, app, clash, current, content) =
                self.capture_reconcile_committed_target(app, clash).await?;
            let captured = self
                .convergence_profiles_version()
                .ok_or_else(|| anyhow::anyhow!("Profiles owner disappeared during convergence"))?;
            if captured != before {
                return Err(ProfilesSourceAdvanced {
                    before,
                    after: captured,
                }
                .into());
            }
            let run_type = self
                .committed_run_type(app.enable_service_mode, trigger)
                .await?;
            let host = match run_type {
                crate::core::RunType::Service => ExecutionHost::Service,
                crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                    ExecutionHost::Local
                }
            };
            let planned = self.planned_committed_round(&target, host, trigger).await?;
            let ref_apply = matches!(&planned, CommittedRound::Apply);
            let handled_by_plan = matches!(&planned, CommittedRound::Settled(_));
            let applied = match planned {
                CommittedRound::Settled(result) => {
                    if result.is_err() {
                        self.convergence_progress().lock().invalidate_selection();
                    }
                    result
                }
                CommittedRound::Apply | CommittedRound::LegacyRecovery => {
                    let admitted = self
                        .convergence_progress()
                        .lock()
                        .allow(&target, host, trigger);
                    match admitted {
                        Ok(()) => {
                            // The actual Core attempt is staged, never recorded
                            // as Applied/Refused before Completed's final fence.
                            self.convergence_progress()
                                .lock()
                                .stage(target.clone(), host);
                            let prepared = match run_type {
                                crate::core::RunType::Normal => {
                                    self.try_reconcile_prepared_local(
                                        &target, &app, &clash, run_type,
                                    )
                                    .await
                                }
                                crate::core::RunType::Service => {
                                    self.try_reconcile_prepared_service(&target, &app, &clash)
                                        .await
                                }
                                crate::core::RunType::Elevated => None,
                            };
                            use crate::client::reconciler::apply::{
                                CommittedPreparedDispatch, route_committed_prepared,
                            };
                            match route_committed_prepared(ref_apply, prepared) {
                                CommittedPreparedDispatch::Observed(result) => result,
                                CommittedPreparedDispatch::LegacyRecovery => {
                                    // Only a missing/unproven legacy S3/S4
                                    // plan may use inline Core reconciliation.
                                    self.convergence_progress().lock().invalidate_selection();
                                    self.convergence_core()
                                        .reconcile_profiles(
                                            clash,
                                            app.core,
                                            run_type,
                                            current,
                                            app,
                                            content,
                                            Some(target.clone()),
                                        )
                                        .await
                                }
                            }
                        }
                        Err(previous_refusal) => Err(previous_refusal.into()),
                    }
                }
            };
            let applied = if host == ExecutionHost::Service && !handled_by_plan {
                match applied {
                    Ok(()) => Ok(()),
                    Err(refusal) => self.fallback_after_service_refusal(&target, refusal).await,
                }
            } else {
                applied
            };
            let after = self
                .convergence_profiles_version()
                .ok_or_else(|| anyhow::anyhow!("Profiles owner disappeared during convergence"))?;
            if after != captured {
                return Err(ProfilesSourceAdvanced {
                    before: captured,
                    after,
                }
                .into());
            }
            applied
        } else {
            let run_type = self
                .committed_run_type(app.enable_service_mode, trigger)
                .await?;
            let target = self.startup_target_identity().await?;
            let host = match run_type {
                crate::core::RunType::Service => ExecutionHost::Service,
                crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                    ExecutionHost::Local
                }
            };
            let planned = self.planned_committed_round(&target, host, trigger).await?;
            let handled_by_plan = matches!(&planned, CommittedRound::Settled(_));
            let result = match planned {
                CommittedRound::Settled(result) => {
                    self.convergence_progress().lock().invalidate_selection();
                    result
                }
                CommittedRound::Apply | CommittedRound::LegacyRecovery => {
                    let admitted = self
                        .convergence_progress()
                        .lock()
                        .allow(&target, host, trigger);
                    match admitted {
                        Ok(()) => {
                            self.convergence_progress().lock().invalidate_selection();
                            self.convergence_progress()
                                .lock()
                                .stage(target.clone(), host);
                            self.convergence_core()
                                .reconcile(clash, target_core, run_type)
                                .await
                        }
                        Err(previous_refusal) => Err(previous_refusal.into()),
                    }
                }
            };
            if host == ExecutionHost::Service && !handled_by_plan {
                match result {
                    Ok(()) => Ok(()),
                    Err(refusal) => self.fallback_after_service_refusal(&target, refusal).await,
                }
            } else {
                result
            }
        }
    }
}

/// One completed runtime round, observed only by the existing mailbox after
/// the worker has joined. This object does not own or mutate a Core session.
pub(in crate::client) struct Round {
    pub verified_running: bool,
    verified_stopped: bool,
    pub connection_effect_observed: bool,
    pub connection_degradation: Option<chimera_contracts::runtime::Degradation>,
    verdict: super::attempt::LegacyApplyVerdict,
}

impl Round {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::client) async fn observe(
        lifecycle: &CoreLifecycleWorkflow,
        result: &anyhow::Result<()>,
        trigger: &str,
        reconciled: bool,
        checked_apply: bool,
        source_advanced: bool,
        final_uncertain: bool,
        prepared_outcome: Option<&super::mutation::ApplyOutcome>,
    ) -> Self {
        // A successful command alone is not proof that the Core
        // reached Running or Stopped. Publish Converged/Inactive
        // only after sampling the runtime owner.
        let observed_core = if result.is_ok() && (reconciled || trigger == "Stop") {
            lifecycle.startup_core_status().await.ok()
        } else {
            None
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_core_status_observed", "CoreLifecycle completion stage");
        let verified_running = if reconciled && result.is_ok() {
            if let Some(observed) = observed_core.as_ref() {
                lifecycle.verified_applied_runtime(observed).await
            } else {
                false
            }
        } else {
            false
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_running_verified", "CoreLifecycle completion stage");
        let verified_stopped = trigger == "Stop"
            && observed_core.as_ref().is_some_and(|snapshot| {
                matches!(
                    snapshot.state,
                    chimera_ipc::api::status::CoreState::Stopped(_)
                )
            });
        // The upstream reconciler records selection after a
        // confirmed runtime apply, not when a source saves.
        // Legacy startup, readiness and Retry use a Result<()>
        // Core port; recover only if both Core and source target
        // still match. Checked Apply has already done its own
        // exact-version selection check and must not be reset.
        if reconciled && !checked_apply {
            if result.is_ok() && verified_running && !source_advanced && !final_uncertain {
                lifecycle.recover_applied_selection_from_receipt().await;
            } else {
                lifecycle.invalidate_applied_selection();
            }
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_selection_recovery_finished", "CoreLifecycle completion stage");
        // Unlike the worker, Completed owns the final observed
        // source-version fence and Running evidence. The previous
        // Core's connections may only be interrupted here; a
        // superseded checked product is never an effect success.
        let mut round_connection_degradation = None;
        let mut connection_effect_observed = false;
        if let Some(effect) = lifecycle.take_checked_connection_effect() {
            let issue = finish_connection_effect(
                effect,
                result,
                verified_running && !final_uncertain,
                source_advanced,
            )
            .await;
            if let Some(ref degradation) = issue {
                tracing::warn!(reason = ?degradation.reason, message = %degradation.message, "checked connection effect degraded");
            }
            connection_effect_observed = true;
            round_connection_degradation = issue;
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_connection_effect_finished", "CoreLifecycle completion stage");

        let verdict = super::attempt::observed_apply_verdict(
            result,
            final_uncertain,
            source_advanced,
            prepared_outcome,
        );
        Self {
            verified_running,
            verified_stopped,
            connection_effect_observed,
            connection_degradation: round_connection_degradation,
            verdict,
        }
    }

    /// One atomic status publication and one attempt settlement, from the
    /// exact same terminal. Never derive Applied from a successful reply.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::client) fn settle(
        self,
        lifecycle: &CoreLifecycleWorkflow,
        status: &tokio::sync::watch::Sender<super::status::ReconcilerStatus>,
        operation_id: u64,
        trigger: &str,
        observed_sources: &super::committed::SourceVersions,
        intent: chimera_core::client::reconciler::plan::RunIntent,
        reconciled: bool,
    ) {
        lifecycle.settle_observed_attempt(&self.verdict, self.verified_running);
        status.send_modify(|status| {
            status.finish_with_verdict(
                operation_id,
                trigger,
                observed_sources,
                intent,
                reconciled,
                self.verified_running,
                self.verified_stopped,
                &self.verdict,
            );
            if let Some(degradation) = self.connection_degradation {
                status.record_system_degradation(degradation);
            }
        });
    }
}

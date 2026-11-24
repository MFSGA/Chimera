//! Serialized committed convergence dispatch over the sole lifecycle owner.
use chimera_contracts::execution_host::ExecutionHost;
use chimera_core::client::reconciler::plan::Trigger;

use super::{
    attempt::OwnershipTransitionPending,
    proof::host_for,
    startup::{CommittedRound, uses_committed_convergence},
};
use crate::client::core_lifecycle::CoreLifecycleWorkflow;

pub use chimera_core::client::core_lifecycle::command::{admission_command, runtime_trigger};
pub use chimera_core::client::reconciler::workflow::{
    AdmissionCommand, CheckedSourceNoticeKind, ProfilesSourceAdvanced, RuntimeAdmission,
    RuntimeReconcilerState, checked_mode_effect_deferred, committed_target_advanced,
    deferred_mode_effect_degradation, desired_host, source_committed_during_round,
};

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

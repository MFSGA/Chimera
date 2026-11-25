//! The single reconciler's committed-source and deferred recovery execution.
//! Host adapters provide authoritative source snapshots and platform leases;
//! no independent owner, queue, or configuration publication is introduced.
use super::{
    attempt::{CommittedTargetAdvanced, OwnershipTransitionPending, ServiceFallbackDeferred},
    inputs::RuntimeInputs,
    plan::Trigger,
    proof::host_for,
    startup::{CommittedRound, uses_committed_convergence},
    startup_executor::StartupHost,
    workflow::{ProfilesSourceAdvanced, desired_host},
};
use crate::client::core_lifecycle::{
    ports::{CoreLifecyclePort, ServiceTransitionLease},
    status::RunType,
};
use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
use chimera_contracts::execution_host::ExecutionHost;

#[async_trait::async_trait]
pub trait ConvergenceHost: StartupHost {
    fn convergence_clash(&self) -> anyhow::Result<ClashConfig>;
    fn convergence_profiles_version(&self) -> Option<u64>;
    fn legacy_service_connected(&self) -> bool;
    async fn capture_committed_profile_inputs_for(
        &self,
        app: ChimeraAppConfig,
        clash: ClashConfig,
    ) -> anyhow::Result<RuntimeInputs>;
    async fn try_reconcile_prepared_local(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &ClashConfig,
        run_type: RunType,
    ) -> Option<anyhow::Result<()>>;
    async fn try_reconcile_prepared_service(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &ClashConfig,
    ) -> Option<anyhow::Result<()>>;
    async fn try_reconcile_deferred_prepared(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &ClashConfig,
        run_type: RunType,
    ) -> Option<anyhow::Result<()>>;
    async fn legacy_retry_deferred_profiles(
        &self,
        inputs: RuntimeInputs,
        expected_digest: &str,
    ) -> anyhow::Result<()>;
    async fn try_begin_service_transition(&self)
    -> anyhow::Result<Box<dyn ServiceTransitionLease>>;
    async fn refresh_service_recovery_host(&self);
}

/// Treat Service failure for one readiness generation as a Local
/// preference only after the previous Service Core was safely released.
pub async fn committed_run_type(
    host: &(impl ConvergenceHost + ?Sized),
    enable_service_mode: bool,
    trigger: Trigger,
) -> anyhow::Result<RunType> {
    if host.canonical_convergence_session() && uses_committed_convergence(trigger, true) {
        let observed = host.core().status().await.map_err(|error| {
            OwnershipTransitionPending::waiting(format!("cannot observe selected owner: {error:#}"))
        })?;
        let (service, generation) = host.service_evidence().await;
        let failed = host.startup_failed_generation();
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

    let preferred = RunType::classify(enable_service_mode, host.legacy_service_connected());
    if preferred != RunType::Service {
        return Ok(preferred);
    }
    let failed_generation = host.startup_failed_generation();
    if failed_generation == 0 {
        return Ok(preferred);
    }
    let (_, current_generation) = host.service_evidence().await;
    Ok(if failed_generation == current_generation {
        RunType::Normal
    } else {
        preferred
    })
}

pub async fn reconcile_with_trigger(
    host: &(impl ConvergenceHost + ?Sized),
    trigger: Trigger,
) -> anyhow::Result<()> {
    // Ref maintain(Retry) lives with S4 apply outcome maintenance.
    // Only repair the accepted receipt's derived file, and never replay
    // old Core bytes or publish a product after its source has advanced.
    if crate::client::reconciler::apply::retry_pending_product(host.core(), trigger, || {
        host.startup_target_identity()
    })
    .await?
    {
        return Ok(());
    }
    let clash = host.convergence_clash()?;
    let app = host.startup_application_config();
    let target_core = app.core;
    if let Some(before) = host.convergence_profiles_version() {
        // Capture both the authoritative committed document and its managed
        // file bytes under the same publication fence, as in ref.
        let (target, app, clash, current, content) = {
            let inputs = host
                .capture_committed_profile_inputs_for(app, clash)
                .await?;
            let target = inputs.checked_target_key()?;
            let (app, clash, current, content) = inputs.into_parts()?;
            (target, app, clash, current, content)
        };
        let captured = host
            .convergence_profiles_version()
            .ok_or_else(|| anyhow::anyhow!("Profiles owner disappeared during convergence"))?;
        if captured != before {
            return Err(ProfilesSourceAdvanced {
                before,
                after: captured,
            }
            .into());
        }
        let run_type = committed_run_type(host, app.enable_service_mode, trigger).await?;
        let execution_host = match run_type {
            RunType::Service => ExecutionHost::Service,
            RunType::Normal | RunType::Elevated => ExecutionHost::Local,
        };
        let planned =
            super::committed_round::planned_committed_round(host, &target, execution_host, trigger)
                .await?;
        let ref_apply = matches!(&planned, CommittedRound::Apply);
        let handled_by_plan = matches!(&planned, CommittedRound::Settled(_));
        let applied = match planned {
            CommittedRound::Settled(result) => {
                if result.is_err() {
                    host.progress().lock().invalidate_selection();
                }
                result
            }
            CommittedRound::Apply | CommittedRound::LegacyRecovery => {
                let admitted = host
                    .progress()
                    .lock()
                    .allow(&target, execution_host, trigger);
                match admitted {
                    Ok(()) => {
                        // The actual Core attempt is staged, never recorded
                        // as Applied/Refused before Completed's final fence.
                        host.progress().lock().stage(target.clone(), execution_host);
                        let prepared = match run_type {
                            RunType::Normal => {
                                host.try_reconcile_prepared_local(&target, &app, &clash, run_type)
                                    .await
                            }
                            RunType::Service => {
                                host.try_reconcile_prepared_service(&target, &app, &clash)
                                    .await
                            }
                            RunType::Elevated => None,
                        };
                        use crate::client::reconciler::apply::{
                            CommittedPreparedDispatch, route_committed_prepared,
                        };
                        match route_committed_prepared(ref_apply, prepared) {
                            CommittedPreparedDispatch::Observed(result) => result,
                            CommittedPreparedDispatch::LegacyRecovery => {
                                // Only a missing/unproven legacy S3/S4
                                // plan may use inline Core reconciliation.
                                host.progress().lock().invalidate_selection();
                                host.core()
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
        let applied = if execution_host == ExecutionHost::Service && !handled_by_plan {
            match applied {
                Ok(()) => Ok(()),
                Err(refusal) => host.fallback_after_service_refusal(&target, refusal).await,
            }
        } else {
            applied
        };
        let after = host
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
        let run_type = committed_run_type(host, app.enable_service_mode, trigger).await?;
        let target = host.startup_target_identity().await?;
        let execution_host = match run_type {
            RunType::Service => ExecutionHost::Service,
            RunType::Normal | RunType::Elevated => ExecutionHost::Local,
        };
        let planned =
            super::committed_round::planned_committed_round(host, &target, execution_host, trigger)
                .await?;
        let handled_by_plan = matches!(&planned, CommittedRound::Settled(_));
        let result = match planned {
            CommittedRound::Settled(result) => {
                host.progress().lock().invalidate_selection();
                result
            }
            CommittedRound::Apply | CommittedRound::LegacyRecovery => {
                let admitted = host
                    .progress()
                    .lock()
                    .allow(&target, execution_host, trigger);
                match admitted {
                    Ok(()) => {
                        host.progress().lock().invalidate_selection();
                        host.progress().lock().stage(target.clone(), execution_host);
                        host.core().reconcile(clash, target_core, run_type).await
                    }
                    Err(previous_refusal) => Err(previous_refusal.into()),
                }
            }
        };
        if execution_host == ExecutionHost::Service && !handled_by_plan {
            match result {
                Ok(()) => Ok(()),
                Err(refusal) => host.fallback_after_service_refusal(&target, refusal).await,
            }
        } else {
            result
        }
    }
}
pub async fn retry_deferred_profiles(
    host: &(impl ConvergenceHost + ?Sized),
    expected_digest: &str,
) -> anyhow::Result<()> {
    let inputs = host
        .capture_committed_profile_inputs_for(
            host.startup_application_config(),
            host.convergence_clash()?,
        )
        .await?;
    if inputs.checked_target_key()? != expected_digest {
        return Ok(());
    }
    if host.canonical_convergence_session() {
        // Deferred retries never revive an intentionally stopped owner.
        // The mailbox has already admitted Running intent; re-observe the
        // actual host here before planning a retry.
        let status = host.core().status().await?;
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            return Err(OwnershipTransitionPending::waiting(
                "deferred Profile retry awaits a running Core",
            )
            .into());
        }
        let execution_host = host_for(status.run_type);
        match super::committed_round::planned_committed_round(
            host,
            expected_digest,
            execution_host,
            Trigger::Retry,
        )
        .await?
        {
            CommittedRound::Settled(result) => result,
            CommittedRound::Apply => {
                host.progress()
                    .lock()
                    .allow(expected_digest, execution_host, Trigger::Retry)?;
                host.progress()
                    .lock()
                    .stage(expected_digest.to_owned(), execution_host);
                host.try_reconcile_deferred_prepared(
                    expected_digest,
                    &inputs.app,
                    &inputs.clash,
                    status.run_type,
                )
                .await
                .unwrap_or_else(|| {
                    Err(OwnershipTransitionPending::waiting(
                        "deferred Profile retry lost its selected owner or source proof",
                    )
                    .into())
                })
            }
            CommittedRound::LegacyRecovery => Err(OwnershipTransitionPending::waiting(
                "deferred Profile retry requires a canonical S4 plan",
            )
            .into()),
        }?;
        return Ok(());
    }
    host.legacy_retry_deferred_profiles(inputs, expected_digest)
        .await
}

pub async fn fallback_after_service_refusal(
    host: &(impl ConvergenceHost + ?Sized),
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
            host.core().outcome_uncertain(),
            false,
        ),
        crate::client::reconciler::attempt::LegacyApplyVerdict::Refused(_)
    ) {
        return Err(refusal);
    }
    if !super::proof::startup_service_refusal_stopped(host.core()).await {
        return Err(refusal);
    }

    // Keep the nonblocking Service transition lease across both release
    // and the Local apply. Dropping it between those operations would let
    // another owner start Service after the release proof but before Local
    // finished, violating ref's single-owner handoff ordering.
    let _transition =
        host.try_begin_service_transition()
            .await
            .map_err(|error| ServiceFallbackDeferred {
                reason: format!("{refusal}; cannot acquire Service transition lease: {error:#}"),
            })?;
    if host.startup_target_identity().await? != target {
        return Err(CommittedTargetAdvanced.into());
    }
    if !super::proof::startup_service_refusal_stopped(host.core()).await {
        return Err(ServiceFallbackDeferred {
                reason: format!("{refusal}; Service stopped-owner proof changed while acquiring the transition lease"),
            }.into());
    }
    let (_, generation) = host.service_evidence().await;
    let released =
        host.release_service_owner()
            .await
            .map_err(|reason| ServiceFallbackDeferred {
                reason: format!("{refusal}; {reason}"),
            })?;
    if !released.proves_no_owned_core() {
        return Err(ServiceFallbackDeferred {
            reason: format!("{refusal}; no fresh evidence that Service released its owned Core"),
        }
        .into());
    }
    // Only suppress Service for this readiness generation after its Core
    // release has been proved. A failed or unknown release MUST NOT make
    // an ordinary Local start eligible.
    if generation != 0 {
        host.set_startup_failed_generation(generation);
    }
    if !host
        .startup_target_identity()
        .await
        .is_ok_and(|current| current == target)
    {
        return Err(CommittedTargetAdvanced.into());
    }

    // This fallback has proved the old Service Core is released. Its
    // Local submission supersedes the staged Service attempt; Completed
    // alone may record the observed Local terminal.
    let local = host
        .startup_reconcile_application_after_service_release(
            host.startup_application_config(),
            Some(target),
        )
        .await;
    host.refresh_service_recovery_host().await;
    if !host
        .startup_target_identity()
        .await
        .is_ok_and(|current| current == target)
    {
        return Err(CommittedTargetAdvanced.into());
    }
    local
}

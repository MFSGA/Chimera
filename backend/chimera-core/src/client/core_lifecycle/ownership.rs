//! Shared conditional retirement and quarantine recovery.
use super::{ports::CoreLifecyclePort, status::CoreStatusSnapshot};
use crate::client::reconciler::{decision::same_core_status, service_evidence::ServiceEvidence};
use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::api::status::CoreState;

#[derive(Debug)]
pub enum StartupOwnerFailure {
    WaitingDependency(String),
    Blocked(String),
    RecoveryRequired(String),
}

pub fn startup_stop_error_is_idempotent(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::NotStarted))
    })
}

fn stop_rejected(
    core: &(impl CoreLifecyclePort + ?Sized),
    error: anyhow::Error,
    uncertain: bool,
) -> StartupOwnerFailure {
    let reason = format!("the Core owner could not be stopped: {error:#}");
    if uncertain {
        StartupOwnerFailure::RecoveryRequired(reason)
    } else if crate::client::reconciler::startup_policy::startup_failure_is_retryable(&error) {
        StartupOwnerFailure::WaitingDependency(reason)
    } else {
        StartupOwnerFailure::Blocked(reason)
    }
}
pub async fn confirm_local_owner_stopped(
    core: &(impl CoreLifecyclePort + ?Sized),
    current: &CoreStatusSnapshot,
) -> Result<(), StartupOwnerFailure> {
    let before_stop = core.status().await.map_err(|error| {
        StartupOwnerFailure::WaitingDependency(format!(
            "could not refresh the Local Core owner: {error:#}"
        ))
    })?;
    if !same_core_status(current, &before_stop) || !matches!(before_stop.state, CoreState::Running)
    {
        return Err(StartupOwnerFailure::WaitingDependency(
            "the Local Core owner changed before it could be stopped".into(),
        ));
    }
    let stop_result = core.stop().await;
    if core.outcome_uncertain() {
        return Err(StartupOwnerFailure::RecoveryRequired(
            "the Local Core stop outcome is uncertain".into(),
        ));
    }
    let stopped = match core.status().await {
        Ok(stopped) => stopped,
        Err(error) => {
            return Err(match stop_result {
                Err(stop_error) => stop_rejected(core, stop_error, core.outcome_uncertain()),
                Ok(()) => StartupOwnerFailure::WaitingDependency(format!(
                    "could not confirm the Local Core stop: {error:#}"
                )),
            });
        }
    };
    let stop_confirmed = stopped.run_type == current.run_type
        && stopped.state_changed_at > 0
        && matches!(&stopped.state, CoreState::Stopped(_));
    if stop_confirmed {
        if let Err(error) = stop_result
            && !startup_stop_error_is_idempotent(&error)
        {
            return Err(stop_rejected(core, error, core.outcome_uncertain()));
        }
    } else {
        if let Err(error) = stop_result {
            return Err(stop_rejected(core, error, core.outcome_uncertain()));
        }
        return Err(StartupOwnerFailure::WaitingDependency(format!(
            "the Local Core did not publish a confirmed stopped state: {stopped:?}"
        )));
    }
    confirm_local_stopped_baseline(core, &stopped)
        .await
        .map_err(StartupOwnerFailure::WaitingDependency)
}

pub async fn confirm_local_stopped_baseline(
    core: &(impl CoreLifecyclePort + ?Sized),
    stopped: &CoreStatusSnapshot,
) -> Result<(), String> {
    if stopped.state_changed_at == 0 {
        return Ok(());
    }
    let baseline = core
        .observe_runtime_baseline()
        .await
        .map(|baseline| {
            matches!(
                baseline,
                super::super::reconciler::reestablish::KnownRuntimeState::Stopped
            )
        })
        .map_err(|error| {
            format!("the Local Core stop lacks authoritative state evidence: {error:#}")
        })?;
    if !baseline {
        return Err(format!(
            "the Local Core runtime is not proven stopped after retirement: {baseline:?}"
        ));
    }
    let confirmed = core
        .status()
        .await
        .map_err(|error| format!("could not recheck the stopped Local Core: {error:#}"))?;
    if !same_core_status(stopped, &confirmed) || !matches!(&confirmed.state, CoreState::Stopped(_))
    {
        return Err("the Local Core state changed after its stop was confirmed".into());
    }
    Ok(())
}

pub async fn recover_service_error_stop<F: std::future::Future<Output = (ServiceEvidence, u64)>>(
    core: &(impl CoreLifecyclePort + ?Sized),
    observe_service: impl Fn() -> F,
    service: ServiceEvidence,
    previous_generation: u64,
) -> Result<(ServiceEvidence, u64), StartupOwnerFailure> {
    if !matches!(
        &service,
        ServiceEvidence::CoreStoppedWithReason {
            runtime_owned: true,
            ..
        }
    ) {
        return Ok((service, previous_generation));
    }

    core.recover_service_quarantine()
        .await
        .map_err(|error| stop_rejected(core, error, core.outcome_uncertain()))?;

    let (service, fresh_generation) = observe_service().await;
    if fresh_generation != previous_generation || !service.proves_runtime_owner() {
        return Err(StartupOwnerFailure::RecoveryRequired(
                "the Service runtime identity changed during quarantine recovery; startup will not hand off another host".into(),
            ));
    }
    let service = match service {
        ServiceEvidence::CoreStoppedWithReason {
            ready,
            phase,
            runtime_owned: true,
            ..
        } => {
            // Recover returns only after the manager has cleared its
            // quarantined epochs; some harmless stop reasons remain in
            // the status snapshot when there was no quarantine.
            ServiceEvidence::CoreStopped {
                ready,
                phase,
                runtime_owned: true,
            }
        }
        service => service,
    };
    Ok((service, fresh_generation))
}

pub async fn retire_service_startup_owner<
    F: std::future::Future<Output = (ServiceEvidence, u64)>,
>(
    core: &(impl CoreLifecyclePort + ?Sized),
    observe_service: impl Fn() -> F,
    service: &ServiceEvidence,
    require_ready_stopped: bool,
) -> Result<ServiceEvidence, StartupOwnerFailure> {
    let expected_owner = service.expected_owner().map_err(|reason| {
        StartupOwnerFailure::RecoveryRequired(format!(
            "the Service Core owner cannot be fenced for retirement: {reason}"
        ))
    })?;
    let stop_result = core.retire_service_core(expected_owner).await;
    if core.outcome_uncertain() {
        return Err(StartupOwnerFailure::RecoveryRequired(
            "the Service Core stop outcome is uncertain".into(),
        ));
    }
    let (after_stop, _) = observe_service().await;
    let confirmed = if require_ready_stopped {
        matches!(
            &after_stop,
            ServiceEvidence::CoreStopped {
                ready: true,
                runtime_owned: true,
                ..
            }
        )
    } else {
        after_stop.proves_no_owned_core()
    };
    if confirmed {
        if let Err(error) = stop_result
            && !startup_stop_error_is_idempotent(&error)
        {
            return Err(stop_rejected(core, error, core.outcome_uncertain()));
        }
        return Ok(after_stop);
    }
    if matches!(&after_stop, ServiceEvidence::Unreadable { .. }) {
        if let Err(error) = stop_result {
            return Err(stop_rejected(core, error, core.outcome_uncertain()));
        }
        return Err(StartupOwnerFailure::WaitingDependency(format!(
            "the Service Core stop could not be confirmed by a fresh probe: {after_stop:?}"
        )));
    }
    if let Err(error) = stop_result {
        return Err(stop_rejected(core, error, core.outcome_uncertain()));
    }
    Err(StartupOwnerFailure::WaitingDependency(format!(
        "the Service Core stop was accepted but a fresh probe did not confirm release: {after_stop:?}"
    )))
}

/// Execute S3 Stop against the same selected instance and committed target.
/// The caller retains the round and selection owner; no independent actor.
pub async fn stop_planned_unverified_core<F, S>(
    core: &(impl CoreLifecyclePort + ?Sized),
    target_identity: impl Fn() -> F,
    observe_service: impl Fn() -> S,
    invalidate_selection: impl FnOnce(),
    target: &str,
    observed: &CoreStatusSnapshot,
    accepted: Option<&std::sync::Arc<crate::client::runtime::RuntimeApplyReceipt>>,
    startup_service: Option<(&ServiceEvidence, u64)>,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<String>>,
    S: std::future::Future<Output = (ServiceEvidence, u64)>,
{
    use crate::client::reconciler::attempt::OwnershipTransitionPending;
    use crate::client::reconciler::{
        attempt::CommittedTargetAdvanced, decision::confirmed_planned_stop,
    };

    // The old source may have advanced while S2/S3 sampled its owner.
    // Do not stop a real Core for a candidate no longer committed.
    if target_identity().await? != target {
        return Err(CommittedTargetAdvanced.into());
    }
    let before = core.status().await?;
    if !same_core_status(observed, &before) || !matches!(before.state, CoreState::Running) {
        return Err(OwnershipTransitionPending::waiting(
            "the selected Core changed before its S3 stop could begin",
        )
        .into());
    }
    let current = core.active_runtime_receipt().await.map_err(|_| {
        OwnershipTransitionPending::waiting(
            "the accepted Core receipt became unreadable before S3 Stop",
        )
    })?;
    if accepted.is_some_and(|receipt| {
        receipt.host != crate::client::reconciler::proof::host_for(before.run_type)
    }) || !crate::client::reconciler::decision::same_active_receipt(accepted, current.as_ref())
    {
        return Err(OwnershipTransitionPending::waiting(
            "the accepted Core revision changed before the S3 stop",
        )
        .into());
    }
    if core.outcome_uncertain() {
        return Err(OwnershipTransitionPending::waiting(
            "the Core outcome became uncertain before S3 Stop",
        )
        .into());
    }
    let stop = if let Some((expected, generation)) = startup_service {
        let (fresh, fresh_generation) = observe_service().await;
        if &fresh != expected
            || fresh_generation != generation
            || !same_core_status(&before, &core.status().await?)
            || target_identity().await? != target
            || core.outcome_uncertain()
        {
            return Err(OwnershipTransitionPending::waiting(
                "the Service owner changed before its conditional S3 retirement",
            )
            .into());
        }
        let final_receipt = core.active_runtime_receipt().await.map_err(|_| {
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
        retire_service_startup_owner(core, &observe_service, &fresh, true)
            .await
            .map(|_| ())
            .map_err(|failure| {
                // S3 failure must not become an S4 Service refusal and authorize fallback.
                anyhow::Error::new(OwnershipTransitionPending::waiting(format!(
                    "Service S3 retirement did not settle: {:?}",
                    failure
                )))
            })
    } else {
        core.stop().await
    };
    if core.outcome_uncertain() {
        anyhow::bail!("S3 Core stop outcome is uncertain; do not apply a second runtime");
    }
    let stopped = core.status().await?;
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
    let confirmed = core.status().await?;
    if !same_core_status(&stopped, &confirmed) {
        return Err(OwnershipTransitionPending::waiting(
            "the selected Core changed while confirming its S3 stop",
        )
        .into());
    }
    invalidate_selection();
    // The stop is now proven, but the previously captured source target
    // could have been superseded while the external Core action ran.
    // In that case only a latest-source round may perform the next Apply.
    if target_identity().await? != target {
        return Err(CommittedTargetAdvanced.into());
    }
    Ok(())
}

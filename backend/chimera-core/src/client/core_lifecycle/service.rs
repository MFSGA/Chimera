//! Shared lower Service endpoint recovery, following the ref retry budget.
use super::{ports::CoreLifecyclePort, workflow::ServiceRecovery};
pub async fn settle_service_recovery_action(core: &(impl CoreLifecyclePort + ?Sized)) -> bool {
    match core.settle_service_recovery_action().await {
        Some(Ok(())) | None => true,
        Some(Err(reason)) => {
            tracing::debug!(%reason, "Service endpoint recovery deferred until the previous Core action settles");
            false
        }
    }
}

pub async fn report_service_endpoint_down(
    core: &(impl CoreLifecyclePort + ?Sized),
    recovery: &parking_lot::Mutex<ServiceRecovery>,
    shutdown: &tokio_util::sync::CancellationToken,
) -> anyhow::Result<bool> {
    let due = recovery
        .lock()
        .due(core.service_endpoint_degraded(), shutdown.is_cancelled());
    if !due || !settle_service_recovery_action(core).await {
        return Ok(false);
    }
    let endpoint_degraded = core.service_endpoint_degraded();
    {
        let mut recovery = recovery.lock();
        if !recovery.begin(endpoint_degraded, shutdown.is_cancelled()) {
            return Ok(false);
        }
    }

    let result = core
        .recover_service_endpoint(shutdown)
        .await
        .unwrap_or_else(|| {
            Err(chimera_core_manager::CoreError::new(
                chimera_core_manager::CoreErrorKind::BackendUnavailable,
                "the shared Core router does not support Service endpoint recovery",
                true,
            ))
        });
    let failed = result.is_err();
    let terminal_error = result.as_ref().err().is_some_and(|error| !error.retryable);
    let endpoint_degraded = core.service_endpoint_degraded();
    recovery
        .lock()
        .finish_attempt(failed, terminal_error, endpoint_degraded);
    if let Err(error) = result {
        tracing::warn!(%error, "Service endpoint recovery attempt failed");
        return Err(error.into());
    }
    Ok(true)
}

use super::{executor::LifecycleHost, ports::ServiceTransitionLease, status::RunType};
use crate::client::reconciler::plan::Trigger;
use chimera_ipc::api::status::CoreState;

pub async fn uninstall_service(
    host: &(impl LifecycleHost + ?Sized),
    transition: &mut dyn ServiceTransitionLease,
) -> anyhow::Result<()> {
    host.service_recovery().lock().suppress();
    transition.uninstall_daemon().await?;
    transition.confirm_stopped().await?;
    if !host.application_config().enable_service_mode {
        return Ok(());
    }

    let before = host.core().status().await?;
    if !matches!(before.state, CoreState::Running) || before.run_type == RunType::Service {
        host.reconcile_with_trigger(Trigger::ServiceOperation)
            .await?;
    }

    let after = host.core().status().await?;
    anyhow::ensure!(
        matches!(after.state, CoreState::Running) && after.run_type != RunType::Service,
        "core did not recover to the local host after Service uninstall"
    );
    Ok(())
}

pub async fn start_service(
    host: &(impl LifecycleHost + ?Sized),
    transition: &mut dyn ServiceTransitionLease,
    restart: bool,
    ready_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    if restart {
        transition.restart_daemon().await?;
    } else {
        transition.start_daemon().await?;
    }
    host.service_recovery().lock().rearm();
    if !host.application_config().enable_service_mode {
        return Ok(());
    }

    transition.confirm_ready(ready_timeout).await?;
    let before = host.core().status().await?;
    if !matches!(before.state, CoreState::Running) || before.run_type != RunType::Service {
        host.reconcile_with_trigger(Trigger::ServiceOperation)
            .await?;
    }

    transition
        .confirm_ready(std::time::Duration::from_secs(5))
        .await?;
    let after = host.core().status().await?;
    anyhow::ensure!(
        matches!(after.state, CoreState::Running) && after.run_type == RunType::Service,
        "core did not reach the Chimera Service host"
    );
    Ok(())
}

pub async fn stop_service(
    host: &(impl LifecycleHost + ?Sized),
    transition: &mut dyn ServiceTransitionLease,
) -> anyhow::Result<()> {
    host.service_recovery().lock().suppress();
    transition.stop_daemon().await?;
    transition.confirm_stopped().await?;
    if !host.application_config().enable_service_mode {
        return Ok(());
    }

    let before = host.core().status().await?;
    if !matches!(before.state, CoreState::Running) || before.run_type == RunType::Service {
        host.reconcile_with_trigger(Trigger::ServiceOperation)
            .await?;
    }

    let after = host.core().status().await?;
    anyhow::ensure!(
        matches!(after.state, CoreState::Running) && after.run_type != RunType::Service,
        "core did not recover to the local host after Service stop"
    );
    Ok(())
}

use super::ports::ServiceLifecyclePort;
use crate::{
    actor_v2::{endpoint::ExecutionHost, service_status::ServiceHostStatus},
    client::reconciler::service_evidence::ServiceEvidence,
};

pub async fn service_evidence(
    service: &dyn ServiceLifecyclePort,
    legacy_status: Option<&tokio::sync::watch::Receiver<ServiceHostStatus>>,
) -> (ServiceEvidence, u64) {
    let probe = service.probe().await;
    if let Some(status) = service.actor_status() {
        // Production uses ref ServiceActor for both the ownership evidence
        // and the readiness generation. Do NOT derive another generation
        // from CoreLifecycle's independent probe cache; it can disagree
        // across a daemon handoff or failed startup attempt.
        let generation = status.ready_generation;
        let evidence = if probe.is_ok() {
            ServiceEvidence::from_status(&status)
        } else {
            // An unanswered ref Actor probe is not a stop or Ready proof,
            // even if a previous watch snapshot was healthy.
            ServiceEvidence::Unreadable {
                phase: crate::actor_v2::service_policy::ServicePhase::Unknown,
            }
        };
        return (evidence, generation);
    }
    let Ok(info) = probe else {
        let generation = legacy_status
            .map(|status| status.borrow().ready_generation)
            .unwrap_or_default();
        return (
            ServiceEvidence::Unreadable {
                phase: crate::actor_v2::service_policy::ServicePhase::Unknown,
            },
            generation,
        );
    };
    // Legacy-only tests keep their injected probe/watch and restart-policy
    // semantics. No second read is performed against a production host.
    let runtime_owned = service.runtime_owned(&info);
    let previous = legacy_status
        .map(|status| status.borrow().clone())
        .unwrap_or_else(ServiceHostStatus::probing);
    let status = previous.observe_probe(info, runtime_owned, Some(service.restart_policy()));
    let generation = status.ready_generation;
    (ServiceEvidence::from_status(&status), generation)
}
pub async fn refresh_service_recovery_host(
    core: &(impl CoreLifecyclePort + ?Sized),
    recovery: &parking_lot::Mutex<ServiceRecovery>,
) {
    let Ok(status) = core.status().await else {
        return;
    };
    recovery
        .lock()
        .follow_host(if status.run_type == RunType::Service {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        });
}

pub fn service_recovery_due_now(
    core: &(impl CoreLifecyclePort + ?Sized),
    recovery: &parking_lot::Mutex<ServiceRecovery>,
    shutdown: &tokio_util::sync::CancellationToken,
) -> bool {
    core.service_endpoint_recovery_supported()
        && recovery
            .lock()
            .due(core.service_endpoint_degraded(), shutdown.is_cancelled())
}

pub async fn service_endpoint_recovery_due(
    core: &(impl CoreLifecyclePort + ?Sized),
    recovery: &parking_lot::Mutex<ServiceRecovery>,
    shutdown: &tokio_util::sync::CancellationToken,
) -> bool {
    if !service_recovery_due_now(core, recovery, shutdown)
        || !settle_service_recovery_action(core).await
    {
        return false;
    }
    service_recovery_due_now(core, recovery, shutdown)
}

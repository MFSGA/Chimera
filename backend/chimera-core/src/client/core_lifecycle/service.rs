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

//! Shared lower Service endpoint recovery, following the ref retry budget.
use super::{ports::RuntimeOwnershipPort, workflow::ServiceRecovery};
pub async fn settle_service_recovery_action(core: &(impl RuntimeOwnershipPort + ?Sized)) -> bool {
    match core.settle_service_recovery_action().await {
        Some(Ok(())) | None => true,
        Some(Err(reason)) => {
            tracing::debug!(%reason, "Service endpoint recovery deferred until the previous Core action settles");
            false
        }
    }
}

pub async fn report_service_endpoint_down(
    core: &(impl RuntimeOwnershipPort + ?Sized),
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

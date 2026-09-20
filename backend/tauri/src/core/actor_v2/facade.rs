//! Control helpers owned by the lower core-host boundary.
//!
//! This is the staged Chimera counterpart of ref `core/actor_v2/facade.rs`.
//! It centralizes ownership of the legacy `CoreManager` and its lifecycle
//! lock without pretending that Chimera already has ref's submit/wait protocol.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use chimera_config::clash::config::ClashConfig;
use futures::FutureExt;

use super::endpoint::{
    ControlEndpoint, CoreCommand, CoreStatusSnapshot, LocalEndpoint, OperationPhase,
};
use crate::{
    client::runtime::{RuntimeSnapshot, RuntimeTransformFailure},
    config::{chimera::ClashCore, clash::ClashInfo},
    core::{
        clash::{api::ApiClient, core::RunType},
        connection_interruption::ConnectionInterruptionService,
    },
    enhance::PostProcessingOutput,
};

const SERVICE_RESTART_BUDGET: u8 = 3;
const LOCAL_OPERATION_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceRestartDecision {
    Ignore,
    Restart { next_attempt: u8 },
    Exhaust,
}

fn service_restart_decision(
    status: chimera_ipc::types::ServiceStatus,
    attempts: u8,
    exhausted: bool,
    budget: u8,
) -> ServiceRestartDecision {
    if exhausted || status != chimera_ipc::types::ServiceStatus::Stopped {
        return ServiceRestartDecision::Ignore;
    }
    if attempts >= budget {
        ServiceRestartDecision::Exhaust
    } else {
        ServiceRestartDecision::Restart {
            next_attempt: attempts.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ServiceRestartPolicySnapshot {
    pub(crate) attempts: u8,
    pub(crate) exhausted: bool,
}

#[derive(Debug)]
pub(crate) struct CoreFacade {
    endpoint: Arc<LocalEndpoint>,
    outcome_uncertain: Arc<AtomicBool>,
    service_restart_attempts: Arc<AtomicU8>,
    service_restart_exhausted: Arc<AtomicBool>,
}

pub(crate) struct ServiceTransition {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    outcome_uncertain: Arc<AtomicBool>,
    restart_attempts: Arc<AtomicU8>,
    restart_exhausted: Arc<AtomicBool>,
}

struct MutationReplyGuard {
    outcome_uncertain: Arc<AtomicBool>,
    armed: bool,
}

impl MutationReplyGuard {
    fn new(outcome_uncertain: Arc<AtomicBool>) -> Self {
        Self {
            outcome_uncertain,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for MutationReplyGuard {
    fn drop(&mut self) {
        if self.armed {
            self.outcome_uncertain.store(true, Ordering::Release);
        }
    }
}

async fn run_in_place_mutation<F>(
    outcome_uncertain: Arc<AtomicBool>,
    operation: &'static str,
    future: F,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    if outcome_uncertain.load(Ordering::Acquire) {
        anyhow::bail!(
            "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
        );
    }

    let mut reply_guard = MutationReplyGuard::new(outcome_uncertain.clone());
    let result = AssertUnwindSafe(future).catch_unwind().await;
    reply_guard.disarm();
    match result {
        Ok(result) => result,
        Err(_) => {
            outcome_uncertain.store(true, Ordering::Release);
            Err(anyhow::anyhow!(
                "{operation} panicked; outcome is uncertain"
            ))
        }
    }
}

impl CoreFacade {
    pub(crate) fn new_local() -> Self {
        Self {
            endpoint: Arc::new(LocalEndpoint::new()),
            outcome_uncertain: Arc::new(AtomicBool::new(false)),
            service_restart_attempts: Arc::new(AtomicU8::new(0)),
            service_restart_exhausted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    async fn run_local_mutation(&self, command: CoreCommand) -> anyhow::Result<()> {
        if self.outcome_uncertain() {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }
        let admitted = self.endpoint.submit(command).await?;
        let id = admitted.id;
        match self.endpoint.wait_operation(id, LOCAL_OPERATION_WAIT).await {
            Some(info) if info.phase == OperationPhase::Succeeded => Ok(()),
            Some(info) if info.phase == OperationPhase::Failed => {
                Err(anyhow::anyhow!(info.error.unwrap_or_else(|| {
                    "lower core operation failed without an error".to_string()
                })))
            }
            Some(info) if info.phase == OperationPhase::Uncertain => {
                self.outcome_uncertain.store(true, Ordering::Release);
                Err(anyhow::anyhow!(info.error.unwrap_or_else(|| {
                    "lower core operation reached an uncertain terminal state".to_string()
                })))
            }
            Some(_) | None => {
                self.outcome_uncertain.store(true, Ordering::Release);
                anyhow::bail!(
                    "lower core operation {} did not reach a terminal state within the wait budget",
                    id.get()
                )
            }
        }
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        let expected_applied = self
            .endpoint
            .status()
            .await?
            .applied
            .map(|identity| identity.revision);
        self.run_local_mutation(CoreCommand::Reconcile {
            clash,
            target_core,
            run_type,
            expected_applied,
        })
        .await
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        self.run_local_mutation(CoreCommand::Stop).await
    }

    pub(crate) async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        self.run_local_mutation(CoreCommand::ChangeCore(clash_core))
            .await
    }

    pub(crate) async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.endpoint.status().await
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.endpoint.recovery_notify()
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        self.endpoint.runtime_transform_output()
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.endpoint.promoted_runtime_snapshot()
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.endpoint.runtime_transform_failure()
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        self.endpoint.effective_clash_info()
    }

    pub(crate) async fn probe_service(
        &self,
    ) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        crate::core::service::control::status().await
    }

    pub(crate) fn service_restart_policy(&self) -> ServiceRestartPolicySnapshot {
        ServiceRestartPolicySnapshot {
            attempts: self.service_restart_attempts.load(Ordering::Acquire),
            exhausted: self.service_restart_exhausted.load(Ordering::Acquire),
        }
    }

    pub(crate) async fn report_service_endpoint_down(&self) -> anyhow::Result<()> {
        if self.service_restart_exhausted.load(Ordering::Acquire) {
            return Ok(());
        }

        let _guard = crate::core::service::HOST_TRANSITION_LOCK.lock().await;
        let info = match tokio::time::timeout(
            Duration::from_secs(5),
            crate::core::service::control::status(),
        )
        .await
        {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => {
                tracing::warn!(%error, "service endpoint-down probe failed; not restarting blindly");
                return Ok(());
            }
            Err(_) => {
                tracing::warn!("service endpoint-down probe timed out; not restarting blindly");
                return Ok(());
            }
        };
        if info.status != chimera_ipc::types::ServiceStatus::Stopped {
            return Ok(());
        }

        let attempts = self.service_restart_attempts.load(Ordering::Acquire);
        match service_restart_decision(
            info.status,
            attempts,
            self.service_restart_exhausted.load(Ordering::Acquire),
            SERVICE_RESTART_BUDGET,
        ) {
            ServiceRestartDecision::Ignore => return Ok(()),
            ServiceRestartDecision::Exhaust => {
                self.service_restart_exhausted
                    .store(true, Ordering::Release);
                return Ok(());
            }
            ServiceRestartDecision::Restart { next_attempt } => {
                self.service_restart_attempts
                    .store(next_attempt, Ordering::Release);
            }
        }
        if let Err(error) = run_in_place_mutation(
            self.outcome_uncertain.clone(),
            "service auto-restart",
            async { crate::core::service::control::start_service_daemon().await },
        )
        .await
        {
            tracing::warn!(%error, "service auto-restart failed");
        }
        Ok(())
    }

    pub(crate) async fn begin_service_transition(&self) -> anyhow::Result<ServiceTransition> {
        if self.outcome_uncertain() {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }
        Ok(ServiceTransition {
            _guard: crate::core::service::HOST_TRANSITION_LOCK.lock().await,
            outcome_uncertain: self.outcome_uncertain.clone(),
            restart_attempts: self.service_restart_attempts.clone(),
            restart_exhausted: self.service_restart_exhausted.clone(),
        })
    }

    pub(crate) async fn on_profile_change(&self, break_when: bool) {
        let result = match ApiClient::new(self.effective_clash_info()) {
            Ok(api) => ConnectionInterruptionService::on_profile_change(&api, break_when).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            tracing::warn!(%error, "failed to interrupt connections after profile change");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_down_restart_policy_only_restarts_explicitly_stopped_daemons() {
        use chimera_ipc::types::ServiceStatus;

        assert_eq!(
            service_restart_decision(ServiceStatus::Running, 0, false, 3),
            ServiceRestartDecision::Ignore
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::NotInstalled, 0, false, 3),
            ServiceRestartDecision::Ignore
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 0, false, 3),
            ServiceRestartDecision::Restart { next_attempt: 1 }
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 2, false, 3),
            ServiceRestartDecision::Restart { next_attempt: 3 }
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 3, false, 3),
            ServiceRestartDecision::Exhaust
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 1, true, 3),
            ServiceRestartDecision::Ignore
        );
    }

    #[tokio::test]
    async fn in_place_mutation_cancellation_latches_uncertain() {
        let uncertain = Arc::new(AtomicBool::new(false));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let waiter = {
            let uncertain = uncertain.clone();
            let started = started.clone();
            let release = release.clone();
            tokio::spawn(async move {
                run_in_place_mutation(uncertain, "test service mutation", async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(())
                })
                .await
            })
        };

        started.notified().await;
        waiter.abort();
        let _ = waiter.await;
        assert!(uncertain.load(Ordering::Acquire));
    }
}

impl ServiceTransition {
    fn rearm_restart_policy(&self) {
        self.restart_attempts.store(0, Ordering::Release);
        self.restart_exhausted.store(false, Ordering::Release);
    }

    pub(crate) async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.rearm_restart_policy();
        run_in_place_mutation(self.outcome_uncertain.clone(), "service install", async {
            crate::core::service::control::install_service_daemon().await
        })
        .await
    }

    pub(crate) async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        run_in_place_mutation(self.outcome_uncertain.clone(), "service uninstall", async {
            crate::core::service::control::uninstall_service().await
        })
        .await
    }

    pub(crate) async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.rearm_restart_policy();
        run_in_place_mutation(self.outcome_uncertain.clone(), "service update", async {
            crate::core::service::control::update_service().await
        })
        .await
    }

    pub(crate) async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.rearm_restart_policy();
        run_in_place_mutation(self.outcome_uncertain.clone(), "service start", async {
            crate::core::service::control::start_service_daemon().await
        })
        .await
    }

    pub(crate) async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.rearm_restart_policy();
        run_in_place_mutation(self.outcome_uncertain.clone(), "service restart", async {
            crate::core::service::control::restart_service_daemon().await
        })
        .await
    }

    pub(crate) async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        run_in_place_mutation(self.outcome_uncertain.clone(), "service stop", async {
            crate::core::service::control::stop_service().await
        })
        .await
    }

    pub(crate) async fn confirm_ready(&mut self, timeout: Duration) -> anyhow::Result<()> {
        crate::core::service::ipc::wait_until_ready(timeout).await?;
        Ok(())
    }

    pub(crate) async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        let observation = crate::core::service::ipc::refresh_state_now()
            .await
            .context("failed to verify Chimera Service after stop")?;
        if observation.status == chimera_ipc::types::ServiceStatus::Running {
            anyhow::bail!("Chimera Service still reports running after stop");
        }
        crate::core::service::ipc::mark_disconnected_now();
        Ok(())
    }
}

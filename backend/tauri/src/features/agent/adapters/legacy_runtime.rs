use std::{future::Future, sync::Arc, time::Duration};

use super::super::{
    diagnostics::host_scope,
    model::{
        AgentActionRequest, AgentAppliedState, AgentCommandError, AgentCoreState, AgentHostScope,
        AgentNetworkSnapshot, AgentResult, AgentRoutingMode,
    },
    ports::{
        AgentConfigurationPort, AgentMutationPort, AgentRuntimePort, AgentTelemetryPort,
        CoreLifecyclePort, CoreRoutingProbePort, HostConnectivityPort, PlatformReadinessPort,
        ServiceControlPort, SystemProxyConfiguration, SystemProxyPort,
    },
};
use super::legacy_snapshot::{LegacySnapshotPorts, collect_network_snapshot};

const CORE_ACTION_TIMEOUT: Duration = Duration::from_secs(3);
const CORE_RESTART_TIMEOUT: Duration = Duration::from_secs(30);
const TUN_ACTION_TIMEOUT: Duration = Duration::from_secs(60);
const SERVICE_MODE_ACTION_TIMEOUT: Duration = Duration::from_secs(60);
const TELEMETRY_ACTION_TIMEOUT: Duration = Duration::from_secs(15);
const SERVICE_ACTION_TIMEOUT: Duration = Duration::from_secs(60);
const STATE_STABILIZE_TIMEOUT: Duration = Duration::from_secs(5);
const STATE_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) struct LegacyAgentRuntime {
    configuration: Arc<dyn AgentConfigurationPort>,
    core: Arc<dyn CoreLifecyclePort>,
    mutation: Arc<dyn AgentMutationPort>,
    routing_probe: Arc<dyn CoreRoutingProbePort>,
    host_connectivity: Arc<dyn HostConnectivityPort>,
    platform_readiness: Arc<dyn PlatformReadinessPort>,
    service: Arc<dyn ServiceControlPort>,
    system_proxy: Arc<dyn SystemProxyPort>,
    telemetry: Arc<dyn AgentTelemetryPort>,
}

impl LegacyAgentRuntime {
    pub(crate) fn new(
        configuration: Arc<dyn AgentConfigurationPort>,
        core: Arc<dyn CoreLifecyclePort>,
        mutation: Arc<dyn AgentMutationPort>,
        routing_probe: Arc<dyn CoreRoutingProbePort>,
        host_connectivity: Arc<dyn HostConnectivityPort>,
        platform_readiness: Arc<dyn PlatformReadinessPort>,
        service: Arc<dyn ServiceControlPort>,
        system_proxy: Arc<dyn SystemProxyPort>,
        telemetry: Arc<dyn AgentTelemetryPort>,
    ) -> Self {
        Self {
            configuration,
            core,
            mutation,
            routing_probe,
            host_connectivity,
            platform_readiness,
            service,
            system_proxy,
            telemetry,
        }
    }
}

#[async_trait::async_trait]
impl AgentRuntimePort for LegacyAgentRuntime {
    async fn snapshot(&self) -> AgentNetworkSnapshot {
        collect_network_snapshot(LegacySnapshotPorts {
            configuration: self.configuration.as_ref(),
            core: self.core.as_ref(),
            routing: self.routing_probe.as_ref(),
            connectivity: self.host_connectivity.as_ref(),
            readiness: self.platform_readiness.as_ref(),
            service: self.service.as_ref(),
            system_proxy: self.system_proxy.as_ref(),
            telemetry: self.telemetry.as_ref(),
        })
        .await
    }

    async fn set_tun_enabled(&self, before: bool, target: bool) -> AgentResult<()> {
        let applied =
            tokio::time::timeout(TUN_ACTION_TIMEOUT, self.mutation.set_tun_enabled(target)).await;
        if !matches!(applied, Ok(Ok(()))) {
            return if self.rollback_tun(before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }

        if wait_for_observation(
            STATE_STABILIZE_TIMEOUT,
            STATE_POLL_INTERVAL,
            || self.snapshot(),
            |snapshot| tun_target_is_applied(snapshot, target),
        )
        .await
        {
            return Ok(());
        }
        if self.rollback_tun(before).await {
            Err(AgentCommandError::VerificationFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        }
    }

    async fn set_system_proxy_enabled(&self, before: bool, target: bool) -> AgentResult<()> {
        let applied = self.system_proxy.apply_desired(target).await;
        if applied.is_err() {
            return if self.rollback_system_proxy_enabled(before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
        if wait_for_observation(
            STATE_STABILIZE_TIMEOUT,
            STATE_POLL_INTERVAL,
            || self.snapshot(),
            |snapshot| system_proxy_target_is_applied(snapshot, target),
        )
        .await
        {
            return Ok(());
        }
        if self.rollback_system_proxy_enabled(before).await {
            Err(AgentCommandError::VerificationFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        }
    }

    async fn set_service_mode(&self, before: bool, target: bool) -> AgentResult<()> {
        let applied = tokio::time::timeout(
            SERVICE_MODE_ACTION_TIMEOUT,
            self.mutation.set_service_mode(target),
        )
        .await;
        if !matches!(applied, Ok(Ok(()))) {
            return if self.rollback_service_mode(before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
        if service_mode_target_is_applied(&self.snapshot().await, target) {
            return Ok(());
        }
        if self.rollback_service_mode(before).await {
            Err(AgentCommandError::VerificationFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        }
    }

    async fn ensure_core_running(&self) -> AgentResult<()> {
        execute_uncertain_action(CORE_RESTART_TIMEOUT, self.core.ensure_running()).await
    }

    async fn restart_core(&self) -> AgentResult<()> {
        execute_uncertain_action(CORE_RESTART_TIMEOUT, self.core.restart()).await
    }

    async fn reconnect_telemetry(&self) -> AgentResult<()> {
        execute_uncertain_action(TELEMETRY_ACTION_TIMEOUT, self.telemetry.reconnect()).await
    }

    async fn control_service(&self, action: &AgentActionRequest) -> AgentResult<()> {
        execute_uncertain_action(SERVICE_ACTION_TIMEOUT, async {
            match action {
                AgentActionRequest::StartService => self.service.start().await,
                AgentActionRequest::StopService => self.service.stop().await,
                AgentActionRequest::RestartService => self.service.restart().await,
                _ => Err(anyhow::anyhow!("unsupported service action")),
            }
        })
        .await
    }

    async fn set_routing_mode(
        &self,
        before: AgentRoutingMode,
        target: AgentRoutingMode,
    ) -> AgentResult<()> {
        let applied =
            tokio::time::timeout(CORE_ACTION_TIMEOUT, self.mutation.set_routing_mode(target)).await;
        if !matches!(applied, Ok(Ok(()))) {
            return if self.rollback_routing_mode(before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
        if routing_mode_target_is_applied(&self.snapshot().await, target) {
            return Ok(());
        }
        if self.rollback_routing_mode(before).await {
            Err(AgentCommandError::VerificationFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        }
    }

    async fn repair_system_proxy_endpoint(
        &self,
        snapshot: &AgentNetworkSnapshot,
        expected_port: u16,
        desired_before: bool,
    ) -> AgentResult<()> {
        if snapshot.core.state != AgentCoreState::Running || !desired_before {
            return Err(AgentCommandError::NetworkStateChanged);
        }
        let original = self.system_proxy.read().await?;
        if !original.enabled || is_expected_enabled_proxy(&original, expected_port) {
            return Err(AgentCommandError::NetworkStateChanged);
        }
        let mut repaired = original.clone();
        repaired.enabled = true;
        repaired.host = "127.0.0.1".into();
        repaired.port = expected_port;
        if self.system_proxy.write(repaired).await.is_err() {
            return if self.rollback_system_proxy(original, desired_before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
        match self.system_proxy.read().await {
            Ok(observed) if is_expected_enabled_proxy(&observed, expected_port) => Ok(()),
            _ if self.rollback_system_proxy(original, desired_before).await => {
                Err(AgentCommandError::VerificationFailed)
            }
            _ => Err(AgentCommandError::PartialApply),
        }
    }

    async fn disable_stale_system_proxy(
        &self,
        snapshot: &AgentNetworkSnapshot,
        expected_port: u16,
        desired_before: bool,
    ) -> AgentResult<()> {
        if snapshot.core.state != AgentCoreState::Stopped {
            return Err(AgentCommandError::NetworkStateChanged);
        }
        let original = self.system_proxy.read().await?;
        if !is_expected_enabled_proxy(&original, expected_port) {
            return Err(AgentCommandError::NetworkStateChanged);
        }
        if self
            .mutation
            .persist_system_proxy_desired(false)
            .await
            .is_err()
        {
            return if self.rollback_system_proxy(original, desired_before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
        let mut disabled = original.clone();
        disabled.enabled = false;
        if self.system_proxy.write(disabled).await.is_err() {
            return if self.rollback_system_proxy(original, desired_before).await {
                Err(AgentCommandError::ActionFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
        let observed = match self.system_proxy.read().await {
            Ok(observed) => observed,
            Err(_) => {
                return if self.rollback_system_proxy(original, desired_before).await {
                    Err(AgentCommandError::VerificationFailed)
                } else {
                    Err(AgentCommandError::PartialApply)
                };
            }
        };
        if !observed.enabled {
            return Ok(());
        }
        if self.rollback_system_proxy(original, desired_before).await {
            Err(AgentCommandError::VerificationFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        }
    }
}

impl LegacyAgentRuntime {
    async fn rollback_tun(&self, target: bool) -> bool {
        let restored =
            tokio::time::timeout(TUN_ACTION_TIMEOUT, self.mutation.set_tun_enabled(target)).await;
        if !matches!(restored, Ok(Ok(()))) {
            return false;
        }
        wait_for_observation(
            STATE_STABILIZE_TIMEOUT,
            STATE_POLL_INTERVAL,
            || self.snapshot(),
            |snapshot| tun_target_is_applied(snapshot, target),
        )
        .await
    }

    async fn rollback_system_proxy_enabled(&self, target: bool) -> bool {
        self.system_proxy.apply_desired(target).await.is_ok()
            && wait_for_observation(
                STATE_STABILIZE_TIMEOUT,
                STATE_POLL_INTERVAL,
                || self.snapshot(),
                |snapshot| system_proxy_target_is_applied(snapshot, target),
            )
            .await
    }

    async fn rollback_service_mode(&self, target: bool) -> bool {
        let restored = tokio::time::timeout(
            SERVICE_MODE_ACTION_TIMEOUT,
            self.mutation.restore_service_mode(target),
        )
        .await;
        matches!(restored, Ok(Ok(())))
            && service_mode_target_is_applied(&self.snapshot().await, target)
    }

    async fn rollback_routing_mode(&self, target: AgentRoutingMode) -> bool {
        let restored =
            tokio::time::timeout(CORE_ACTION_TIMEOUT, self.mutation.set_routing_mode(target)).await;
        matches!(restored, Ok(Ok(())))
            && routing_mode_target_is_applied(&self.snapshot().await, target)
    }

    async fn rollback_system_proxy(
        &self,
        original: SystemProxyConfiguration,
        desired_before: bool,
    ) -> bool {
        let persisted = self
            .mutation
            .persist_system_proxy_desired(desired_before)
            .await
            .is_ok();
        let restored = self.system_proxy.write(original).await.is_ok();
        persisted && restored
    }
}

async fn wait_for_observation<T, F, Fut, P>(
    timeout: Duration,
    poll_interval: Duration,
    mut observe: F,
    predicate: P,
) -> bool
where
    F: FnMut() -> Fut,
    Fut: Future<Output = T>,
    P: Fn(&T) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let observation = match tokio::time::timeout(remaining, observe()).await {
            Ok(observation) => observation,
            Err(_) => return false,
        };
        if predicate(&observation) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(poll_interval.min(remaining)).await;
    }
}

async fn execute_uncertain_action<F>(action_timeout: Duration, action: F) -> AgentResult<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    tokio::time::timeout(action_timeout, action)
        .await
        .map_err(|_| AgentCommandError::PartialApply)?
        .map_err(|_| AgentCommandError::PartialApply)
}

fn tun_target_is_applied(snapshot: &AgentNetworkSnapshot, target: bool) -> bool {
    snapshot.tun.desired_enabled == target
        && snapshot.tun.generated_runtime_enabled == Some(target)
        && snapshot.tun.observed_enabled == Some(target)
        && snapshot.tun.applied_consistency == AgentAppliedState::Consistent
        && snapshot.core.state == AgentCoreState::Running
}

fn system_proxy_target_is_applied(snapshot: &AgentNetworkSnapshot, target: bool) -> bool {
    snapshot.system_proxy.desired_enabled == target
        && snapshot.system_proxy.observed_enabled == Some(target)
        && (!target || snapshot.system_proxy.matches_expected_endpoint == Some(true))
}

fn service_mode_target_is_applied(snapshot: &AgentNetworkSnapshot, target: bool) -> bool {
    snapshot.service.desired_enabled == target
        && snapshot.service.state == super::super::model::AgentServiceState::Running
        && snapshot.service.ipc_connected
        && snapshot.service.runtime_compatible == Some(true)
        && snapshot.core.state == AgentCoreState::Running
        && if target {
            snapshot.core.run_type == super::super::model::AgentRunType::Service
        } else {
            snapshot.core.run_type != super::super::model::AgentRunType::Service
        }
}

fn routing_mode_target_is_applied(
    snapshot: &AgentNetworkSnapshot,
    target: AgentRoutingMode,
) -> bool {
    snapshot.core.routing_mode == Some(target)
        && snapshot.core.observed_routing_mode == Some(target)
        && snapshot.core.applied_consistency == AgentAppliedState::Consistent
}

fn is_expected_enabled_proxy(proxy: &SystemProxyConfiguration, expected_port: u16) -> bool {
    proxy.enabled
        && proxy.port == expected_port
        && host_scope(&proxy.host) == AgentHostScope::Loopback
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::{execute_uncertain_action, wait_for_observation};
    use crate::features::agent::AgentCommandError;

    #[tokio::test]
    async fn observation_polling_waits_for_a_delayed_verified_state() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();

        assert!(
            wait_for_observation(
                Duration::from_secs(1),
                Duration::from_millis(1),
                move || {
                    let attempt = observed.fetch_add(1, Ordering::SeqCst);
                    async move { attempt >= 2 }
                },
                |enabled| *enabled,
            )
            .await
        );
        assert!(calls.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn observation_polling_fails_closed_after_its_deadline() {
        assert!(
            !wait_for_observation(
                Duration::from_millis(5),
                Duration::from_millis(1),
                || async { false },
                |enabled| *enabled,
            )
            .await
        );
    }

    #[tokio::test]
    async fn observation_polling_bounds_a_stalled_observation() {
        assert!(
            !tokio::time::timeout(
                Duration::from_secs(1),
                wait_for_observation(
                    Duration::from_millis(5),
                    Duration::from_millis(1),
                    std::future::pending::<bool>,
                    |enabled| *enabled,
                ),
            )
            .await
            .expect("stalled observation must respect the inner deadline")
        );
    }

    #[tokio::test]
    async fn uncertain_actions_map_failure_and_timeout_to_partial_apply() {
        assert!(
            execute_uncertain_action(Duration::from_secs(1), async { Ok(()) })
                .await
                .is_ok()
        );
        assert!(matches!(
            execute_uncertain_action(Duration::from_secs(1), async {
                Err(anyhow::anyhow!("action failed after dispatch"))
            })
            .await,
            Err(AgentCommandError::PartialApply)
        ));
        assert!(matches!(
            execute_uncertain_action(
                Duration::from_millis(1),
                std::future::pending::<anyhow::Result<()>>(),
            )
            .await,
            Err(AgentCommandError::PartialApply)
        ));
    }
}

use std::time::Duration;

use super::super::{
    diagnostics::{
        derive_findings, derive_health, snapshot_revision, summarize_service,
        summarize_system_proxy, tun_applied_consistency,
    },
    model::{
        AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState,
        AgentNetworkSnapshot, AgentOsFamily, AgentPrivacyBoundary, AgentProbeCode,
        AgentProbeFailure, AgentTelemetrySnapshot, AgentTunSnapshot,
        NETWORK_SNAPSHOT_SCHEMA_VERSION,
    },
    planning::recommendations,
    ports::{
        AgentConfigurationPort, AgentTelemetryPort, CoreLifecyclePort, CoreRoutingProbePort,
        ServiceControlPort, SystemProxyPort,
    },
};

const SNAPSHOT_INFRASTRUCTURE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) async fn collect_network_snapshot(
    configuration_port: &dyn AgentConfigurationPort,
    core_lifecycle: &dyn CoreLifecyclePort,
    routing_probe: &dyn CoreRoutingProbePort,
    service_control: &dyn ServiceControlPort,
    system_proxy_port: &dyn SystemProxyPort,
    telemetry_port: &dyn AgentTelemetryPort,
) -> AgentNetworkSnapshot {
    let configuration = configuration_port.snapshot();

    let core_status =
        tokio::time::timeout(SNAPSHOT_INFRASTRUCTURE_TIMEOUT, core_lifecycle.status());
    let service_ipc_connected = service_control.ipc_connected();
    let service_status =
        tokio::time::timeout(SNAPSHOT_INFRASTRUCTURE_TIMEOUT, service_control.status());
    let system_proxy = system_proxy_port.probe();
    let (core_status, service_status, system_proxy) =
        tokio::join!(core_status, service_status, system_proxy);

    let mut failures = Vec::new();
    let (core_state, core_run_type, core_state_changed_at) = match core_status {
        Ok(status) => (status.state, status.run_type, status.state_changed_at),
        Err(_) => {
            failures.push(AgentProbeFailure {
                code: AgentProbeCode::CoreStatusTimeout,
            });
            (
                AgentCoreState::Unknown,
                super::super::model::AgentRunType::Unknown,
                0,
            )
        }
    };
    let mut core = AgentCoreSnapshot {
        state: core_state,
        run_type: core_run_type,
        selected_core: configuration.selected_core,
        state_changed_at: core_state_changed_at,
        runtime_config_present: configuration.runtime_config_present,
        routing_mode: configuration.routing_mode,
        observed_routing_mode: None,
        applied_consistency: AgentAppliedState::Unknown,
    };

    if core.state == AgentCoreState::Running {
        match routing_probe.observed_mode().await {
            Ok(mode) => {
                core.observed_routing_mode = Some(mode);
                core.applied_consistency = if core.routing_mode == Some(mode) {
                    AgentAppliedState::Consistent
                } else {
                    AgentAppliedState::Stale
                };
            }
            Err(()) => failures.push(AgentProbeFailure {
                code: AgentProbeCode::CoreConfigUnavailable,
            }),
        }
    }
    let service = summarize_service(
        configuration.desired_service_mode,
        service_ipc_connected,
        service_status,
        &mut failures,
    );
    let observed_system_proxy = system_proxy;
    let system_proxy = summarize_system_proxy(
        configuration.desired_system_proxy,
        configuration.expected_mixed_port,
        observed_system_proxy,
        &mut failures,
    );
    let tun = AgentTunSnapshot {
        desired_enabled: configuration.desired_tun,
        generated_runtime_enabled: configuration.generated_tun_enabled,
        observed_active: AgentAppliedState::Unknown,
        applied_consistency: tun_applied_consistency(
            configuration.desired_tun,
            configuration.generated_tun_enabled,
        ),
    };
    let profiles = configuration.profiles;
    let telemetry = telemetry_port.snapshot().unwrap_or_else(|| {
        failures.push(AgentProbeFailure {
            code: AgentProbeCode::TelemetryUnavailable,
        });
        AgentTelemetrySnapshot {
            state: AgentConnectorState::Unknown,
            active_connection_count: None,
            upload_speed: None,
            download_speed: None,
            upload_total: None,
            download_total: None,
            recent_error_count: 0,
        }
    });
    let findings = derive_findings(
        &core,
        &service,
        &system_proxy,
        &tun,
        &profiles,
        &telemetry,
        configuration.secret_is_weak,
    );
    let health = derive_health(&findings, &failures);
    let revision = snapshot_revision(&core, &service, &system_proxy, &tun);

    let mut snapshot = AgentNetworkSnapshot {
        schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        revision,
        captured_at: chrono::Utc::now().timestamp_millis(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        os_family: AgentOsFamily::current(),
        health,
        core,
        service,
        system_proxy,
        tun,
        profiles,
        telemetry,
        findings,
        probe_failures: failures,
        recommendations: Vec::new(),
        privacy: AgentPrivacyBoundary::privacy_safe(),
    };
    snapshot.recommendations = recommendations(&snapshot);
    snapshot
}

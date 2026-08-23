use std::time::Duration;

use super::super::{
    diagnostics::{
        append_connectivity_finding, append_platform_readiness_findings, derive_findings,
        derive_health, snapshot_revision, summarize_service, summarize_system_proxy,
        tun_applied_consistency,
    },
    host_connectivity::unavailable_host_connectivity,
    model::{
        AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState,
        AgentNetworkSnapshot, AgentOsFamily, AgentPrivacyBoundary, AgentProbeCode,
        AgentProbeFailure, AgentProcessPrivilegeStatus, AgentTelemetrySnapshot,
        AgentTunPermissionReadiness, AgentTunSnapshot, NETWORK_SNAPSHOT_SCHEMA_VERSION,
    },
    planning::recommendations,
    platform_readiness::classify_platform_readiness,
    ports::{
        AgentConfigurationPort, AgentTelemetryPort, CoreLifecyclePort, CoreRoutingProbePort,
        HostConnectivityPort, PlatformReadinessPort, ServiceControlPort, SystemProxyPort,
    },
};

const SNAPSHOT_INFRASTRUCTURE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct LegacySnapshotPorts<'a> {
    pub(crate) configuration: &'a dyn AgentConfigurationPort,
    pub(crate) core: &'a dyn CoreLifecyclePort,
    pub(crate) routing: &'a dyn CoreRoutingProbePort,
    pub(crate) connectivity: &'a dyn HostConnectivityPort,
    pub(crate) readiness: &'a dyn PlatformReadinessPort,
    pub(crate) service: &'a dyn ServiceControlPort,
    pub(crate) system_proxy: &'a dyn SystemProxyPort,
    pub(crate) telemetry: &'a dyn AgentTelemetryPort,
}

pub(crate) async fn collect_network_snapshot(
    ports: LegacySnapshotPorts<'_>,
) -> AgentNetworkSnapshot {
    let LegacySnapshotPorts {
        configuration: configuration_port,
        core: core_lifecycle,
        routing: routing_probe,
        connectivity: host_connectivity_port,
        readiness: platform_readiness_port,
        service: service_control,
        system_proxy: system_proxy_port,
        telemetry: telemetry_port,
    } = ports;
    let configuration = configuration_port.snapshot();

    let core_status =
        tokio::time::timeout(SNAPSHOT_INFRASTRUCTURE_TIMEOUT, core_lifecycle.status());
    let service_ipc_connected = service_control.ipc_connected();
    let service_status =
        tokio::time::timeout(SNAPSHOT_INFRASTRUCTURE_TIMEOUT, service_control.status());
    let host_connectivity = tokio::time::timeout(
        SNAPSHOT_INFRASTRUCTURE_TIMEOUT,
        host_connectivity_port.snapshot(),
    );
    let process_privilege = tokio::time::timeout(
        SNAPSHOT_INFRASTRUCTURE_TIMEOUT,
        platform_readiness_port.process_privilege(),
    );
    let system_proxy = system_proxy_port.probe();
    let (core_status, service_status, host_connectivity, process_privilege, system_proxy) = tokio::join!(
        core_status,
        service_status,
        host_connectivity,
        process_privilege,
        system_proxy
    );

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

    let mut observed_tun_enabled = None;
    if core.state == AgentCoreState::Running {
        match routing_probe.observed_configuration().await {
            Ok(observed) => {
                core.observed_routing_mode = Some(observed.routing_mode);
                core.applied_consistency = if core.routing_mode == Some(observed.routing_mode) {
                    AgentAppliedState::Consistent
                } else {
                    AgentAppliedState::Stale
                };
                observed_tun_enabled = observed.tun_enabled;
                if observed_tun_enabled.is_none() {
                    failures.push(AgentProbeFailure {
                        code: AgentProbeCode::TunStatusUnavailable,
                    });
                }
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
        observed_enabled: observed_tun_enabled,
        applied_consistency: tun_applied_consistency(
            configuration.desired_tun,
            configuration.generated_tun_enabled,
            observed_tun_enabled,
        ),
    };
    let profiles = configuration.profiles;
    let connectivity = host_connectivity.unwrap_or_else(|_| unavailable_host_connectivity());
    if connectivity.status == super::super::model::AgentHostConnectivityStatus::Indeterminate {
        failures.push(AgentProbeFailure {
            code: AgentProbeCode::HostConnectivityUnavailable,
        });
    }
    let process_privilege = process_privilege.unwrap_or(AgentProcessPrivilegeStatus::Unknown);
    let platform_readiness =
        classify_platform_readiness(process_privilege, &core, &service, &tun, &connectivity);
    if tun.desired_enabled
        && platform_readiness.tun_permission == AgentTunPermissionReadiness::Indeterminate
    {
        failures.push(AgentProbeFailure {
            code: AgentProbeCode::PlatformReadinessUnavailable,
        });
    }
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
    let mut findings = derive_findings(
        &core,
        &service,
        &system_proxy,
        &tun,
        &profiles,
        &telemetry,
        configuration.secret_is_weak,
    );
    append_connectivity_finding(&mut findings, connectivity.status);
    append_platform_readiness_findings(&mut findings, &platform_readiness);
    let health = derive_health(&findings, &failures);
    let revision = snapshot_revision(
        &core,
        &service,
        &system_proxy,
        &tun,
        &connectivity,
        &platform_readiness,
    );

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
        connectivity,
        platform_readiness,
        findings,
        probe_failures: failures,
        recommendations: Vec::new(),
        privacy: AgentPrivacyBoundary::privacy_safe(),
    };
    snapshot.recommendations = recommendations(&snapshot);
    snapshot
}

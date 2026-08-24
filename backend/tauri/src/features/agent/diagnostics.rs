use std::net::IpAddr;

use sha2::{Digest, Sha256};

use super::model::{
    AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState, AgentFinding,
    AgentFindingCode, AgentFindingSeverity, AgentHealth, AgentHostConnectivitySnapshot,
    AgentHostConnectivityStatus, AgentHostScope, AgentPlatformReadinessSnapshot, AgentProbeCode,
    AgentProbeFailure, AgentProfileSnapshot, AgentServiceSnapshot, AgentServiceState,
    AgentSystemDnsVerificationStatus, AgentSystemProxySnapshot, AgentTelemetrySnapshot,
    AgentTunPermissionReadiness, AgentTunSnapshot, NETWORK_SNAPSHOT_SCHEMA_VERSION,
};
use super::ports::{ServiceLifecycleStatus, SystemProxyConfiguration};

pub(super) fn tun_applied_consistency(
    desired: bool,
    generated: Option<bool>,
    observed: Option<bool>,
) -> AgentAppliedState {
    if generated.is_some_and(|enabled| enabled != desired)
        || observed.is_some_and(|enabled| enabled != desired)
    {
        AgentAppliedState::Stale
    } else if generated == Some(desired) && observed == Some(desired) {
        AgentAppliedState::Consistent
    } else {
        AgentAppliedState::Unknown
    }
}

pub(super) fn summarize_service(
    desired_enabled: bool,
    ipc_connected: bool,
    result: Result<anyhow::Result<ServiceLifecycleStatus>, tokio::time::error::Elapsed>,
    failures: &mut Vec<AgentProbeFailure>,
) -> AgentServiceSnapshot {
    match result {
        Ok(Ok(status)) => AgentServiceSnapshot {
            desired_enabled,
            state: status.state,
            ipc_connected,
            runtime_compatible: status.runtime_compatible,
        },
        Ok(Err(_)) => {
            failures.push(AgentProbeFailure {
                code: AgentProbeCode::ServiceStatusUnavailable,
            });
            AgentServiceSnapshot {
                desired_enabled,
                state: AgentServiceState::Unknown,
                ipc_connected,
                runtime_compatible: None,
            }
        }
        Err(_) => {
            failures.push(AgentProbeFailure {
                code: AgentProbeCode::ServiceStatusTimeout,
            });
            AgentServiceSnapshot {
                desired_enabled,
                state: AgentServiceState::Unknown,
                ipc_connected,
                runtime_compatible: None,
            }
        }
    }
}

pub(super) fn summarize_system_proxy(
    desired_enabled: bool,
    expected_mixed_port: u16,
    observed: Option<SystemProxyConfiguration>,
    failures: &mut Vec<AgentProbeFailure>,
) -> AgentSystemProxySnapshot {
    match observed {
        Some(proxy) => {
            let scope = host_scope(&proxy.host);
            AgentSystemProxySnapshot {
                desired_enabled,
                observed_enabled: Some(proxy.enabled),
                observed_host_scope: scope,
                observed_port: Some(proxy.port),
                expected_mixed_port,
                matches_expected_endpoint: Some(
                    scope == AgentHostScope::Loopback && proxy.port == expected_mixed_port,
                ),
            }
        }
        _ => {
            failures.push(AgentProbeFailure {
                code: AgentProbeCode::SystemProxyUnavailable,
            });
            AgentSystemProxySnapshot {
                desired_enabled,
                observed_enabled: None,
                observed_host_scope: AgentHostScope::Unknown,
                observed_port: None,
                expected_mixed_port,
                matches_expected_endpoint: None,
            }
        }
    }
}

pub(super) fn host_scope(host: &str) -> AgentHostScope {
    if host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
    {
        AgentHostScope::Loopback
    } else if host.trim().is_empty() {
        AgentHostScope::Unknown
    } else {
        AgentHostScope::NonLoopback
    }
}

pub(super) fn derive_findings(
    core: &AgentCoreSnapshot,
    service: &AgentServiceSnapshot,
    proxy: &AgentSystemProxySnapshot,
    tun: &AgentTunSnapshot,
    profiles: &AgentProfileSnapshot,
    telemetry: &AgentTelemetrySnapshot,
    secret_is_weak: bool,
) -> Vec<AgentFinding> {
    let mut findings = Vec::new();
    push_finding(
        &mut findings,
        secret_is_weak,
        AgentFindingCode::WeakControllerSecret,
        AgentFindingSeverity::Warning,
    );
    push_finding(
        &mut findings,
        proxy.observed_enabled == Some(true) && core.state == AgentCoreState::Stopped,
        AgentFindingCode::SystemProxyWithoutRunningCore,
        AgentFindingSeverity::Critical,
    );
    push_finding(
        &mut findings,
        proxy.observed_enabled == Some(true) && proxy.matches_expected_endpoint == Some(false),
        AgentFindingCode::SystemProxyEndpointMismatch,
        AgentFindingSeverity::Warning,
    );
    push_finding(
        &mut findings,
        !core.runtime_config_present,
        AgentFindingCode::RuntimeConfigMissing,
        AgentFindingSeverity::Critical,
    );
    push_finding(
        &mut findings,
        profiles.active_count == 0 || !profiles.active_references_valid,
        AgentFindingCode::ActiveProfileMissing,
        AgentFindingSeverity::Warning,
    );
    push_finding(
        &mut findings,
        service.desired_enabled
            && (!service.ipc_connected || service.runtime_compatible == Some(false)),
        AgentFindingCode::ServiceModeInconsistent,
        AgentFindingSeverity::Warning,
    );
    push_finding(
        &mut findings,
        core.state == AgentCoreState::Running
            && telemetry.state == AgentConnectorState::Disconnected,
        AgentFindingCode::ClashConnectorDisconnected,
        AgentFindingSeverity::Warning,
    );
    push_finding(
        &mut findings,
        tun.generated_runtime_enabled
            .is_some_and(|generated| generated != tun.desired_enabled),
        AgentFindingCode::TunRuntimeMismatch,
        AgentFindingSeverity::Critical,
    );
    push_finding(
        &mut findings,
        telemetry.recent_error_count > 0,
        AgentFindingCode::RecentCoreErrors,
        AgentFindingSeverity::Info,
    );
    findings
}

pub(super) fn append_connectivity_finding(
    findings: &mut Vec<AgentFinding>,
    status: AgentHostConnectivityStatus,
) {
    let finding = match status {
        AgentHostConnectivityStatus::LinkDisconnected => Some((
            AgentFindingCode::HostLinkDisconnected,
            AgentFindingSeverity::Critical,
        )),
        AgentHostConnectivityStatus::AddressUnavailable => Some((
            AgentFindingCode::HostAddressUnavailable,
            AgentFindingSeverity::Critical,
        )),
        AgentHostConnectivityStatus::DefaultRouteUnavailable => Some((
            AgentFindingCode::HostDefaultRouteUnavailable,
            AgentFindingSeverity::Critical,
        )),
        AgentHostConnectivityStatus::DnsUnavailable => Some((
            AgentFindingCode::HostDnsUnavailable,
            AgentFindingSeverity::Critical,
        )),
        AgentHostConnectivityStatus::CaptivePortalSuspected => Some((
            AgentFindingCode::HostCaptivePortalSuspected,
            AgentFindingSeverity::Warning,
        )),
        AgentHostConnectivityStatus::InternetUnreachable => Some((
            AgentFindingCode::HostInternetUnreachable,
            AgentFindingSeverity::Critical,
        )),
        AgentHostConnectivityStatus::OnlineIpv4Only => {
            Some((AgentFindingCode::HostIpv4Only, AgentFindingSeverity::Info))
        }
        AgentHostConnectivityStatus::OnlineIpv6Only => {
            Some((AgentFindingCode::HostIpv6Only, AgentFindingSeverity::Info))
        }
        AgentHostConnectivityStatus::OnlineDualStack
        | AgentHostConnectivityStatus::Indeterminate => None,
    };
    if let Some((code, severity)) = finding {
        findings.push(AgentFinding { code, severity });
    }
}

pub(super) fn append_platform_readiness_findings(
    findings: &mut Vec<AgentFinding>,
    readiness: &AgentPlatformReadinessSnapshot,
) {
    push_finding(
        findings,
        readiness.tun_permission == AgentTunPermissionReadiness::Required,
        AgentFindingCode::TunPermissionRequired,
        AgentFindingSeverity::Warning,
    );
    push_finding(
        findings,
        matches!(
            readiness.system_dns_verification,
            AgentSystemDnsVerificationStatus::NotConfigured
                | AgentSystemDnsVerificationStatus::ResolutionFailed
        ),
        AgentFindingCode::TunSystemDnsUnverified,
        AgentFindingSeverity::Warning,
    );
}

fn push_finding(
    findings: &mut Vec<AgentFinding>,
    condition: bool,
    code: AgentFindingCode,
    severity: AgentFindingSeverity,
) {
    if condition {
        findings.push(AgentFinding { code, severity });
    }
}

pub(super) fn derive_health(
    findings: &[AgentFinding],
    failures: &[AgentProbeFailure],
) -> AgentHealth {
    if findings
        .iter()
        .any(|finding| finding.severity == AgentFindingSeverity::Critical)
    {
        AgentHealth::Critical
    } else if !failures.is_empty() {
        AgentHealth::Degraded
    } else if findings
        .iter()
        .any(|finding| finding.severity == AgentFindingSeverity::Warning)
    {
        AgentHealth::Warning
    } else {
        AgentHealth::Healthy
    }
}

pub(super) fn snapshot_revision(
    core: &AgentCoreSnapshot,
    service: &AgentServiceSnapshot,
    proxy: &AgentSystemProxySnapshot,
    tun: &AgentTunSnapshot,
    connectivity: &AgentHostConnectivitySnapshot,
    readiness: &AgentPlatformReadinessSnapshot,
) -> String {
    let material = format!(
        "{}:{:?}:{:?}:{}:{:?}:{:?}:{}:{:?}:{:?}:{:?}:{}:{:?}:{:?}:{:?}:{:?}:{}:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
        NETWORK_SNAPSHOT_SCHEMA_VERSION,
        core.state,
        core.run_type,
        core.state_changed_at,
        core.routing_mode,
        core.observed_routing_mode,
        service.desired_enabled,
        service.state,
        proxy.observed_enabled,
        proxy.observed_port,
        tun.desired_enabled,
        tun.generated_runtime_enabled,
        connectivity.status,
        connectivity.active_interface_kind,
        connectivity.link_up,
        connectivity.ipv4.usable_ip,
        connectivity.ipv6.usable_ip,
        connectivity.dns_resolves,
        connectivity.captive_portal_suspected,
        readiness.process_privilege,
        readiness.tun_permission,
        readiness.tun_verification,
        readiness.system_dns_verification,
    );
    hex::encode(Sha256::digest(material.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        append_connectivity_finding, append_platform_readiness_findings, derive_findings,
        derive_health, host_scope, snapshot_revision, summarize_system_proxy,
        tun_applied_consistency,
    };
    use crate::features::agent::model::{
        AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState,
        AgentFindingCode, AgentFindingSeverity, AgentHealth, AgentHostConnectivitySnapshot,
        AgentHostConnectivityStatus, AgentHostScope, AgentIpFamilyConnectivity,
        AgentNetworkInterfaceKind, AgentPlatformReadinessSnapshot, AgentProbeCode,
        AgentProbeFailure, AgentProcessPrivilegeStatus, AgentProfileSnapshot, AgentRoutingMode,
        AgentRunType, AgentSelectedCore, AgentServiceSnapshot, AgentServiceState,
        AgentSystemDnsVerificationStatus, AgentSystemProxySnapshot, AgentTelemetrySnapshot,
        AgentTunPermissionReadiness, AgentTunSnapshot, AgentTunVerificationStatus,
    };
    use crate::features::agent::ports::SystemProxyConfiguration;

    #[test]
    fn classifies_only_loopback_hosts_as_loopback() {
        assert_eq!(host_scope("127.0.0.1"), AgentHostScope::Loopback);
        assert_eq!(host_scope("::1"), AgentHostScope::Loopback);
        assert_eq!(host_scope("localhost"), AgentHostScope::Loopback);
        assert_eq!(host_scope("192.168.1.1"), AgentHostScope::NonLoopback);
    }

    #[test]
    fn tun_consistency_requires_desired_generated_and_observed_state() {
        assert_eq!(
            tun_applied_consistency(true, Some(true), Some(true)),
            AgentAppliedState::Consistent
        );
        assert_eq!(
            tun_applied_consistency(false, Some(false), Some(false)),
            AgentAppliedState::Consistent
        );
        assert_eq!(
            tun_applied_consistency(true, Some(false), Some(true)),
            AgentAppliedState::Stale
        );
        assert_eq!(
            tun_applied_consistency(false, Some(false), Some(true)),
            AgentAppliedState::Stale
        );
        assert_eq!(
            tun_applied_consistency(true, Some(true), None),
            AgentAppliedState::Unknown
        );
    }

    #[test]
    fn unknown_core_state_does_not_claim_the_core_is_stopped() {
        let core = AgentCoreSnapshot {
            state: AgentCoreState::Unknown,
            run_type: AgentRunType::Unknown,
            selected_core: AgentSelectedCore::Mihomo,
            state_changed_at: 0,
            runtime_config_present: true,
            routing_mode: Some(AgentRoutingMode::Rule),
            observed_routing_mode: None,
            applied_consistency: AgentAppliedState::Unknown,
        };
        let service = AgentServiceSnapshot {
            desired_enabled: false,
            state: AgentServiceState::Unknown,
            ipc_connected: false,
            runtime_compatible: None,
        };
        let proxy = AgentSystemProxySnapshot {
            desired_enabled: true,
            observed_enabled: Some(true),
            observed_host_scope: AgentHostScope::Loopback,
            observed_port: Some(7890),
            expected_mixed_port: 7890,
            matches_expected_endpoint: Some(true),
        };
        let tun = AgentTunSnapshot {
            desired_enabled: false,
            generated_runtime_enabled: Some(false),
            observed_enabled: Some(false),
            applied_consistency: AgentAppliedState::Consistent,
        };
        let profiles = AgentProfileSnapshot {
            total_count: 1,
            active_count: 1,
            remote_count: 0,
            local_count: 1,
            active_references_valid: true,
        };
        let telemetry = AgentTelemetrySnapshot {
            state: AgentConnectorState::Unknown,
            active_connection_count: None,
            upload_speed: None,
            download_speed: None,
            upload_total: None,
            download_total: None,
            recent_error_count: 0,
        };

        let findings = derive_findings(&core, &service, &proxy, &tun, &profiles, &telemetry, false);
        assert!(
            !findings
                .iter()
                .any(|finding| { finding.code == AgentFindingCode::SystemProxyWithoutRunningCore })
        );
    }

    #[test]
    fn connectivity_statuses_map_to_stable_findings_and_health() {
        let cases = [
            (
                AgentHostConnectivityStatus::LinkDisconnected,
                Some((
                    AgentFindingCode::HostLinkDisconnected,
                    AgentFindingSeverity::Critical,
                )),
                AgentHealth::Critical,
            ),
            (
                AgentHostConnectivityStatus::AddressUnavailable,
                Some((
                    AgentFindingCode::HostAddressUnavailable,
                    AgentFindingSeverity::Critical,
                )),
                AgentHealth::Critical,
            ),
            (
                AgentHostConnectivityStatus::DefaultRouteUnavailable,
                Some((
                    AgentFindingCode::HostDefaultRouteUnavailable,
                    AgentFindingSeverity::Critical,
                )),
                AgentHealth::Critical,
            ),
            (
                AgentHostConnectivityStatus::DnsUnavailable,
                Some((
                    AgentFindingCode::HostDnsUnavailable,
                    AgentFindingSeverity::Critical,
                )),
                AgentHealth::Critical,
            ),
            (
                AgentHostConnectivityStatus::CaptivePortalSuspected,
                Some((
                    AgentFindingCode::HostCaptivePortalSuspected,
                    AgentFindingSeverity::Warning,
                )),
                AgentHealth::Warning,
            ),
            (
                AgentHostConnectivityStatus::InternetUnreachable,
                Some((
                    AgentFindingCode::HostInternetUnreachable,
                    AgentFindingSeverity::Critical,
                )),
                AgentHealth::Critical,
            ),
            (
                AgentHostConnectivityStatus::OnlineIpv4Only,
                Some((AgentFindingCode::HostIpv4Only, AgentFindingSeverity::Info)),
                AgentHealth::Healthy,
            ),
            (
                AgentHostConnectivityStatus::OnlineIpv6Only,
                Some((AgentFindingCode::HostIpv6Only, AgentFindingSeverity::Info)),
                AgentHealth::Healthy,
            ),
            (
                AgentHostConnectivityStatus::OnlineDualStack,
                None,
                AgentHealth::Healthy,
            ),
            (
                AgentHostConnectivityStatus::Indeterminate,
                None,
                AgentHealth::Degraded,
            ),
        ];

        for (status, expected, expected_health) in cases {
            let mut findings = Vec::new();
            append_connectivity_finding(&mut findings, status);
            match expected {
                Some((code, severity)) => {
                    assert_eq!(findings.len(), 1, "status {status:?}");
                    assert_eq!(findings[0].code, code, "status {status:?}");
                    assert_eq!(findings[0].severity, severity, "status {status:?}");
                }
                None => assert!(findings.is_empty(), "status {status:?}"),
            }
            let failures = if status == AgentHostConnectivityStatus::Indeterminate {
                vec![AgentProbeFailure {
                    code: AgentProbeCode::HostConnectivityUnavailable,
                }]
            } else {
                Vec::new()
            };
            assert_eq!(derive_health(&findings, &failures), expected_health);
        }
    }

    #[test]
    fn connectivity_changes_are_bound_into_snapshot_revision() {
        let core = AgentCoreSnapshot {
            state: AgentCoreState::Running,
            run_type: AgentRunType::Normal,
            selected_core: AgentSelectedCore::Mihomo,
            state_changed_at: 1,
            runtime_config_present: true,
            routing_mode: Some(AgentRoutingMode::Rule),
            observed_routing_mode: Some(AgentRoutingMode::Rule),
            applied_consistency: AgentAppliedState::Consistent,
        };
        let service = AgentServiceSnapshot {
            desired_enabled: false,
            state: AgentServiceState::Stopped,
            ipc_connected: false,
            runtime_compatible: None,
        };
        let proxy = AgentSystemProxySnapshot {
            desired_enabled: false,
            observed_enabled: Some(false),
            observed_host_scope: AgentHostScope::Loopback,
            observed_port: Some(7890),
            expected_mixed_port: 7890,
            matches_expected_endpoint: Some(true),
        };
        let tun = AgentTunSnapshot {
            desired_enabled: false,
            generated_runtime_enabled: Some(false),
            observed_enabled: Some(false),
            applied_consistency: AgentAppliedState::Consistent,
        };
        let online = connectivity(AgentHostConnectivityStatus::OnlineDualStack);
        let offline = connectivity(AgentHostConnectivityStatus::InternetUnreachable);
        let readiness = readiness(
            AgentTunPermissionReadiness::NotRequired,
            AgentSystemDnsVerificationStatus::NotRequired,
        );

        assert_ne!(
            snapshot_revision(&core, &service, &proxy, &tun, &online, &readiness),
            snapshot_revision(&core, &service, &proxy, &tun, &offline, &readiness)
        );
    }

    #[test]
    fn readiness_findings_and_health_are_relevant_only_to_requested_tun() {
        let cases = [
            (
                readiness(
                    AgentTunPermissionReadiness::NotRequired,
                    AgentSystemDnsVerificationStatus::NotRequired,
                ),
                None,
                AgentHealth::Healthy,
            ),
            (
                readiness(
                    AgentTunPermissionReadiness::Required,
                    AgentSystemDnsVerificationStatus::Verified,
                ),
                Some(AgentFindingCode::TunPermissionRequired),
                AgentHealth::Warning,
            ),
            (
                readiness(
                    AgentTunPermissionReadiness::Satisfied,
                    AgentSystemDnsVerificationStatus::ResolutionFailed,
                ),
                Some(AgentFindingCode::TunSystemDnsUnverified),
                AgentHealth::Warning,
            ),
        ];

        for (readiness, expected_code, expected_health) in cases {
            let mut findings = Vec::new();
            append_platform_readiness_findings(&mut findings, &readiness);
            assert_eq!(findings.first().map(|finding| finding.code), expected_code);
            assert_eq!(derive_health(&findings, &[]), expected_health);
        }

        let degraded = readiness(
            AgentTunPermissionReadiness::Indeterminate,
            AgentSystemDnsVerificationStatus::Unavailable,
        );
        let mut findings = Vec::new();
        append_platform_readiness_findings(&mut findings, &degraded);
        assert_eq!(
            derive_health(
                &findings,
                &[AgentProbeFailure {
                    code: AgentProbeCode::PlatformReadinessUnavailable,
                }],
            ),
            AgentHealth::Degraded
        );
    }

    #[test]
    fn readiness_changes_are_bound_into_snapshot_revision() {
        let core = AgentCoreSnapshot {
            state: AgentCoreState::Running,
            run_type: AgentRunType::Normal,
            selected_core: AgentSelectedCore::Mihomo,
            state_changed_at: 1,
            runtime_config_present: true,
            routing_mode: Some(AgentRoutingMode::Rule),
            observed_routing_mode: Some(AgentRoutingMode::Rule),
            applied_consistency: AgentAppliedState::Consistent,
        };
        let service = AgentServiceSnapshot {
            desired_enabled: false,
            state: AgentServiceState::NotInstalled,
            ipc_connected: false,
            runtime_compatible: None,
        };
        let proxy = AgentSystemProxySnapshot {
            desired_enabled: false,
            observed_enabled: Some(false),
            observed_host_scope: AgentHostScope::Loopback,
            observed_port: Some(7890),
            expected_mixed_port: 7890,
            matches_expected_endpoint: Some(true),
        };
        let tun = AgentTunSnapshot {
            desired_enabled: true,
            generated_runtime_enabled: Some(true),
            observed_enabled: Some(true),
            applied_consistency: AgentAppliedState::Consistent,
        };
        let connectivity = connectivity(AgentHostConnectivityStatus::OnlineDualStack);
        let ready = readiness(
            AgentTunPermissionReadiness::Satisfied,
            AgentSystemDnsVerificationStatus::Verified,
        );
        let required = readiness(
            AgentTunPermissionReadiness::Required,
            AgentSystemDnsVerificationStatus::Verified,
        );

        assert_ne!(
            snapshot_revision(&core, &service, &proxy, &tun, &connectivity, &ready),
            snapshot_revision(&core, &service, &proxy, &tun, &connectivity, &required,)
        );
    }

    #[test]
    fn connectivity_codes_serialize_to_stable_privacy_safe_values() {
        let finding = AgentFindingCode::HostCaptivePortalSuspected;
        let readiness_finding = AgentFindingCode::TunPermissionRequired;
        let probe = AgentProbeCode::HostConnectivityUnavailable;
        let readiness_probe = AgentProbeCode::PlatformReadinessUnavailable;
        assert_eq!(
            serde_json::to_string(&finding).unwrap(),
            "\"host_captive_portal_suspected\""
        );
        assert_eq!(
            serde_json::to_string(&readiness_finding).unwrap(),
            "\"tun_permission_required\""
        );
        assert_eq!(
            serde_json::to_string(&probe).unwrap(),
            "\"host_connectivity_unavailable\""
        );
        assert_eq!(
            serde_json::to_string(&readiness_probe).unwrap(),
            "\"platform_readiness_unavailable\""
        );
    }

    fn readiness(
        tun_permission: AgentTunPermissionReadiness,
        system_dns_verification: AgentSystemDnsVerificationStatus,
    ) -> AgentPlatformReadinessSnapshot {
        AgentPlatformReadinessSnapshot {
            process_privilege: AgentProcessPrivilegeStatus::Standard,
            service_mode_available: Some(false),
            tun_permission,
            tun_verification: if tun_permission == AgentTunPermissionReadiness::NotRequired {
                AgentTunVerificationStatus::NotRequested
            } else {
                AgentTunVerificationStatus::Unavailable
            },
            system_dns_verification,
            reasons: Vec::new(),
        }
    }

    fn connectivity(status: AgentHostConnectivityStatus) -> AgentHostConnectivitySnapshot {
        AgentHostConnectivitySnapshot {
            status,
            active_interface_kind: AgentNetworkInterfaceKind::Unknown,
            link_up: None,
            ipv4: AgentIpFamilyConnectivity {
                usable_ip: false,
                default_route: false,
                internet_reachable: None,
            },
            ipv6: AgentIpFamilyConnectivity {
                usable_ip: false,
                default_route: false,
                internet_reachable: None,
            },
            dns_configured: None,
            dns_resolves: None,
            captive_portal_suspected: None,
            reasons: Vec::new(),
        }
    }

    #[test]
    fn unavailable_system_proxy_probe_fails_closed() {
        let mut failures = Vec::new();
        let summary = summarize_system_proxy(true, 7890, None, &mut failures);

        assert_eq!(summary.observed_enabled, None);
        assert_eq!(summary.observed_host_scope, AgentHostScope::Unknown);
        assert_eq!(summary.observed_port, None);
        assert_eq!(summary.matches_expected_endpoint, None);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].code, AgentProbeCode::SystemProxyUnavailable);
    }

    #[test]
    fn system_proxy_summary_does_not_serialize_raw_host_or_bypass() {
        let raw = SystemProxyConfiguration {
            enabled: true,
            host: "controller-secret.canary.example".into(),
            port: 7890,
            bypass: "subscription-token.canary".into(),
        };
        let mut failures = Vec::new();
        let summary = summarize_system_proxy(true, 7890, Some(raw), &mut failures);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("controller-secret.canary.example"));
        assert!(!serialized.contains("subscription-token.canary"));
        assert_eq!(summary.observed_host_scope, AgentHostScope::NonLoopback);
        assert!(failures.is_empty());
    }
}

use std::net::IpAddr;

use sha2::{Digest, Sha256};

use super::model::{
    AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState, AgentFinding,
    AgentFindingCode, AgentFindingSeverity, AgentHealth, AgentHostScope, AgentProbeCode,
    AgentProbeFailure, AgentProfileSnapshot, AgentServiceSnapshot, AgentServiceState,
    AgentSystemProxySnapshot, AgentTelemetrySnapshot, AgentTunSnapshot,
    NETWORK_SNAPSHOT_SCHEMA_VERSION,
};
use super::ports::{ServiceLifecycleStatus, SystemProxyConfiguration};

pub(super) fn tun_applied_consistency(desired: bool, generated: Option<bool>) -> AgentAppliedState {
    match generated {
        Some(enabled) if enabled == desired => AgentAppliedState::Consistent,
        Some(_) => AgentAppliedState::Stale,
        None => AgentAppliedState::Unknown,
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
) -> String {
    let material = format!(
        "{}:{:?}:{:?}:{}:{:?}:{:?}:{}:{:?}:{:?}:{:?}:{}:{:?}",
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
    );
    hex::encode(Sha256::digest(material.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{derive_findings, host_scope, summarize_system_proxy, tun_applied_consistency};
    use crate::features::agent::model::{
        AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState,
        AgentFindingCode, AgentHostScope, AgentProbeCode, AgentProfileSnapshot, AgentRoutingMode,
        AgentRunType, AgentSelectedCore, AgentServiceSnapshot, AgentServiceState,
        AgentSystemProxySnapshot, AgentTelemetrySnapshot, AgentTunSnapshot,
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
    fn tun_consistency_is_derived_from_desired_and_generated_state() {
        assert_eq!(
            tun_applied_consistency(true, Some(true)),
            AgentAppliedState::Consistent
        );
        assert_eq!(
            tun_applied_consistency(false, Some(false)),
            AgentAppliedState::Consistent
        );
        assert_eq!(
            tun_applied_consistency(true, Some(false)),
            AgentAppliedState::Stale
        );
        assert_eq!(
            tun_applied_consistency(false, Some(true)),
            AgentAppliedState::Stale
        );
        assert_eq!(
            tun_applied_consistency(true, None),
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
            observed_active: AgentAppliedState::Unknown,
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

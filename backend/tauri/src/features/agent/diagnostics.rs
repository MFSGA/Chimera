use std::{net::IpAddr, time::Duration};

use chimera_ipc::{api::status::CoreState, types::ServiceStatus};
use sha2::{Digest, Sha256};
use sysproxy::Sysproxy;
use tauri::{AppHandle, Manager};

use crate::{
    config::{core::Config, profile::item::Profile},
    core::{
        clash::{
            core::{CoreManager, RunType},
            ws::{ClashConnectionsConnector, ClashConnectionsConnectorState},
        },
        service,
    },
};

use super::{
    core_probe,
    model::{
        AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState, AgentFinding,
        AgentFindingCode, AgentFindingSeverity, AgentHealth, AgentHostScope, AgentNetworkSnapshot,
        AgentPrivacyBoundary, AgentProbeCode, AgentProbeFailure, AgentProfileSnapshot,
        AgentRoutingMode, AgentRunType, AgentServiceSnapshot, AgentServiceState,
        AgentSystemProxySnapshot, AgentTelemetrySnapshot, AgentTunSnapshot,
        NETWORK_SNAPSHOT_SCHEMA_VERSION,
    },
};

pub(crate) async fn collect_network_snapshot(app: &AppHandle) -> AgentNetworkSnapshot {
    let verge = Config::verge().latest().clone();
    let clash = Config::clash().latest().clone();
    let runtime = Config::runtime().latest().clone();
    let profiles = Config::profiles().data().clone();
    let expected_mixed_port = verge
        .verge_mixed_port
        .unwrap_or_else(|| clash.get_mixed_port());
    let selected_core = verge.clash_core.unwrap_or_default().to_string();
    let runtime_config_present = runtime.config.is_some();
    let routing_mode = runtime
        .config
        .as_ref()
        .and_then(|config| config.get("mode"))
        .and_then(serde_yaml::Value::as_str)
        .and_then(AgentRoutingMode::parse);
    let generated_runtime_enabled = generated_tun_enabled(runtime.config.as_ref());
    let secret_is_weak = clash
        .get_client_info()
        .secret
        .as_deref()
        .map(|secret| secret.trim().is_empty() || secret == "chimera")
        .unwrap_or(true);

    let core_status = CoreManager::global().status();
    let service_status = tokio::time::timeout(Duration::from_secs(2), service::control::status());
    let system_proxy = tokio::task::spawn_blocking(Sysproxy::get_system_proxy);
    let (core_status, service_status, system_proxy) =
        tokio::join!(core_status, service_status, system_proxy);

    let mut core = AgentCoreSnapshot {
        state: map_core_state(core_status.0.as_ref()),
        run_type: map_run_type(core_status.2),
        selected_core,
        state_changed_at: core_status.1,
        runtime_config_present,
        routing_mode,
        observed_routing_mode: None,
        applied_consistency: AgentAppliedState::Unknown,
    };

    let mut failures = Vec::new();
    if core.state == AgentCoreState::Running {
        match core_probe::observed_routing_mode().await {
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
    let service = summarize_service(&verge, service_status, &mut failures);
    let system_proxy = summarize_system_proxy(
        verge.enable_system_proxy.unwrap_or(false),
        expected_mixed_port,
        system_proxy,
        &mut failures,
    );
    let tun = AgentTunSnapshot {
        desired_enabled: verge.enable_tun_mode.unwrap_or(false),
        generated_runtime_enabled,
        observed_active: AgentAppliedState::Unknown,
        applied_consistency: AgentAppliedState::Unknown,
    };
    let profiles = summarize_profiles(&profiles);
    let telemetry = summarize_telemetry(app, &mut failures);
    let findings = derive_findings(
        &core,
        &service,
        &system_proxy,
        &tun,
        &profiles,
        &telemetry,
        secret_is_weak,
    );
    let health = derive_health(&findings, &failures);
    let revision = snapshot_revision(&core, &service, &system_proxy, &tun);

    AgentNetworkSnapshot {
        schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        revision,
        captured_at: chrono::Utc::now().timestamp_millis(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        os_family: std::env::consts::OS.to_owned(),
        health,
        core,
        service,
        system_proxy,
        tun,
        profiles,
        telemetry,
        findings,
        probe_failures: failures,
        privacy: AgentPrivacyBoundary {
            contains_raw_logs: false,
            contains_profile_names: false,
            contains_profile_urls: false,
            contains_connection_targets: false,
            contains_controller_secret: false,
        },
    }
}

fn generated_tun_enabled(config: Option<&serde_yaml::Mapping>) -> Option<bool> {
    config
        .and_then(|config| config.get("tun"))
        .and_then(serde_yaml::Value::as_mapping)
        .and_then(|tun| tun.get("enable"))
        .and_then(serde_yaml::Value::as_bool)
}

fn map_core_state(state: &CoreState) -> AgentCoreState {
    match state {
        CoreState::Running => AgentCoreState::Running,
        CoreState::Stopped(_) => AgentCoreState::Stopped,
    }
}

fn map_run_type(run_type: RunType) -> AgentRunType {
    match run_type {
        RunType::Normal => AgentRunType::Normal,
        RunType::Service => AgentRunType::Service,
        RunType::Elevated => AgentRunType::Elevated,
    }
}

fn summarize_service(
    verge: &crate::config::chimera::IVerge,
    result: Result<anyhow::Result<chimera_ipc::types::StatusInfo<'_>>, tokio::time::error::Elapsed>,
    failures: &mut Vec<AgentProbeFailure>,
) -> AgentServiceSnapshot {
    let desired_enabled = verge.enable_service_mode.unwrap_or(false);
    let ipc_connected = service::ipc::get_ipc_state().is_connected();
    match result {
        Ok(Ok(status)) => AgentServiceSnapshot {
            desired_enabled,
            state: match status.status {
                ServiceStatus::NotInstalled => AgentServiceState::NotInstalled,
                ServiceStatus::Stopped => AgentServiceState::Stopped,
                ServiceStatus::Running => AgentServiceState::Running,
            },
            ipc_connected,
            runtime_compatible: matches!(status.status, ServiceStatus::Running)
                .then(|| service::is_service_runtime_compatible(&status)),
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

fn summarize_system_proxy(
    desired_enabled: bool,
    expected_mixed_port: u16,
    result: Result<Result<sysproxy::Sysproxy, sysproxy::Error>, tokio::task::JoinError>,
    failures: &mut Vec<AgentProbeFailure>,
) -> AgentSystemProxySnapshot {
    match result {
        Ok(Ok(proxy)) => {
            let scope = host_scope(&proxy.host);
            AgentSystemProxySnapshot {
                desired_enabled,
                observed_enabled: Some(proxy.enable),
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

fn summarize_profiles(
    profiles: &crate::config::profile::profiles::Profiles,
) -> AgentProfileSnapshot {
    let remote_count = profiles
        .items
        .iter()
        .filter(|profile| matches!(profile, Profile::Remote(_)))
        .count() as u32;
    let active_references_valid = profiles.current.iter().all(|uid| {
        profiles
            .items
            .iter()
            .any(|profile| crate::config::profile::item::ProfileMetaGetter::uid(profile) == uid)
    });
    AgentProfileSnapshot {
        total_count: profiles.items.len() as u32,
        active_count: profiles.current.len() as u32,
        remote_count,
        local_count: profiles.items.len() as u32 - remote_count,
        active_references_valid,
    }
}

fn summarize_telemetry(
    app: &AppHandle,
    failures: &mut Vec<AgentProbeFailure>,
) -> AgentTelemetrySnapshot {
    let Some(connector) = app.try_state::<ClashConnectionsConnector>() else {
        failures.push(AgentProbeFailure {
            code: AgentProbeCode::TelemetryUnavailable,
        });
        return AgentTelemetrySnapshot {
            state: AgentConnectorState::Unknown,
            active_connection_count: None,
            upload_speed: None,
            download_speed: None,
            upload_total: None,
            download_total: None,
            recent_error_count: 0,
        };
    };
    let snapshot = connector.snapshot();
    let latest = snapshot.connections.last();
    AgentTelemetrySnapshot {
        state: match snapshot.state {
            ClashConnectionsConnectorState::Disconnected => AgentConnectorState::Disconnected,
            ClashConnectionsConnectorState::Connecting => AgentConnectorState::Connecting,
            ClashConnectionsConnectorState::Connected => AgentConnectorState::Connected,
        },
        active_connection_count: latest
            .and_then(|sample| sample.connections.as_ref())
            .map(|connections| connections.len() as u32),
        upload_speed: latest.map(|sample| sample.upload_speed),
        download_speed: latest.map(|sample| sample.download_speed),
        upload_total: latest.map(|sample| sample.upload_total.to_string()),
        download_total: latest.map(|sample| sample.download_total.to_string()),
        recent_error_count: snapshot
            .logs
            .iter()
            .filter(|entry| {
                matches!(
                    entry.log_type.to_ascii_lowercase().as_str(),
                    "error" | "fatal"
                )
            })
            .count() as u32,
    }
}

fn derive_findings(
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
        proxy.observed_enabled == Some(true) && core.state != AgentCoreState::Running,
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
        tun.desired_enabled && tun.generated_runtime_enabled == Some(false),
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

fn derive_health(findings: &[AgentFinding], failures: &[AgentProbeFailure]) -> AgentHealth {
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

fn snapshot_revision(
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
    use super::{host_scope, summarize_system_proxy};
    use crate::features::agent::model::AgentHostScope;
    use sysproxy::Sysproxy;

    #[test]
    fn classifies_only_loopback_hosts_as_loopback() {
        assert_eq!(host_scope("127.0.0.1"), AgentHostScope::Loopback);
        assert_eq!(host_scope("::1"), AgentHostScope::Loopback);
        assert_eq!(host_scope("localhost"), AgentHostScope::Loopback);
        assert_eq!(host_scope("192.168.1.1"), AgentHostScope::NonLoopback);
    }

    #[test]
    fn system_proxy_summary_does_not_serialize_raw_host_or_bypass() {
        let raw = Sysproxy {
            enable: true,
            host: "controller-secret.canary.example".into(),
            port: 7890,
            bypass: "subscription-token.canary".into(),
        };
        let mut failures = Vec::new();
        let summary = summarize_system_proxy(true, 7890, Ok(Ok(raw)), &mut failures);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("controller-secret.canary.example"));
        assert!(!serialized.contains("subscription-token.canary"));
        assert_eq!(summary.observed_host_scope, AgentHostScope::NonLoopback);
        assert!(failures.is_empty());
    }
}

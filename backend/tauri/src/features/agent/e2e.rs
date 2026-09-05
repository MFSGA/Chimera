use std::sync::atomic::{AtomicBool, Ordering};

use super::model::{
    AgentActionRequest, AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState,
    AgentFinding, AgentFindingCode, AgentFindingSeverity, AgentHealth, AgentHostScope,
    AgentNetworkSnapshot, AgentPrivacyBoundary, AgentProfileSnapshot, AgentRoutingMode,
    AgentRunType, AgentServiceSnapshot, AgentServiceState, AgentSystemProxySnapshot,
    AgentTelemetrySnapshot, AgentTunSnapshot, NETWORK_SNAPSHOT_SCHEMA_VERSION,
};

const FIXTURE_ENV: &str = "CHIMERA_E2E_AGENT_FIXTURE";
const STALE_PROXY_FIXTURE: &str = "stale-proxy";
const FIXTURE_STATE_CHANGED_AT: i64 = 1_788_623_990_000;
const FIXTURE_PORT: u16 = 7890;

static STALE_PROXY_REPAIRED: AtomicBool = AtomicBool::new(false);

pub(super) fn fixture_enabled() -> bool {
    std::env::var(FIXTURE_ENV).as_deref() == Ok(STALE_PROXY_FIXTURE)
}

pub(super) fn collect_network_snapshot() -> Option<AgentNetworkSnapshot> {
    fixture_enabled().then(snapshot)
}

pub(super) fn mark_stale_proxy_repaired() {
    if fixture_enabled() {
        STALE_PROXY_REPAIRED.store(true, Ordering::SeqCst);
    }
}

fn snapshot() -> AgentNetworkSnapshot {
    let repaired = STALE_PROXY_REPAIRED.load(Ordering::SeqCst);
    AgentNetworkSnapshot {
        schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        revision: if repaired {
            "agent-e2e-healthy"
        } else {
            "agent-e2e-stale-proxy"
        }
        .into(),
        captured_at: chrono::Utc::now().timestamp_millis(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        os_family: std::env::consts::OS.into(),
        health: if repaired {
            AgentHealth::Healthy
        } else {
            AgentHealth::Critical
        },
        core: core_snapshot(),
        service: service_snapshot(),
        system_proxy: proxy_snapshot(repaired),
        tun: tun_snapshot(),
        profiles: profile_snapshot(),
        telemetry: telemetry_snapshot(),
        findings: findings(repaired),
        probe_failures: Vec::new(),
        privacy: privacy_boundary(),
    }
}

fn core_snapshot() -> AgentCoreSnapshot {
    AgentCoreSnapshot {
        state: AgentCoreState::Stopped,
        run_type: AgentRunType::Normal,
        selected_core: "mihomo".into(),
        state_changed_at: FIXTURE_STATE_CHANGED_AT,
        runtime_config_present: true,
        routing_mode: Some(AgentRoutingMode::Rule),
        observed_routing_mode: None,
        applied_consistency: AgentAppliedState::Unknown,
    }
}

fn service_snapshot() -> AgentServiceSnapshot {
    AgentServiceSnapshot {
        desired_enabled: false,
        state: AgentServiceState::NotInstalled,
        ipc_connected: false,
        runtime_compatible: None,
    }
}

fn proxy_snapshot(repaired: bool) -> AgentSystemProxySnapshot {
    AgentSystemProxySnapshot {
        desired_enabled: !repaired,
        observed_enabled: Some(!repaired),
        observed_host_scope: AgentHostScope::Loopback,
        observed_port: Some(FIXTURE_PORT),
        expected_mixed_port: FIXTURE_PORT,
        matches_expected_endpoint: Some(!repaired),
    }
}

fn tun_snapshot() -> AgentTunSnapshot {
    AgentTunSnapshot {
        desired_enabled: false,
        generated_runtime_enabled: Some(false),
        observed_active: AgentAppliedState::Consistent,
        applied_consistency: AgentAppliedState::Consistent,
    }
}

fn profile_snapshot() -> AgentProfileSnapshot {
    AgentProfileSnapshot {
        total_count: 3,
        active_count: 1,
        remote_count: 2,
        local_count: 1,
        active_references_valid: true,
    }
}

fn telemetry_snapshot() -> AgentTelemetrySnapshot {
    AgentTelemetrySnapshot {
        state: AgentConnectorState::Disconnected,
        active_connection_count: Some(0),
        upload_speed: Some(0),
        download_speed: Some(0),
        upload_total: Some("0".into()),
        download_total: Some("0".into()),
        recent_error_count: 0,
    }
}

fn findings(repaired: bool) -> Vec<AgentFinding> {
    if repaired {
        return Vec::new();
    }
    vec![AgentFinding {
        code: AgentFindingCode::SystemProxyWithoutRunningCore,
        severity: AgentFindingSeverity::Critical,
        recommended_action: Some(AgentActionRequest::DisableStaleSystemProxy),
    }]
}

fn privacy_boundary() -> AgentPrivacyBoundary {
    AgentPrivacyBoundary {
        contains_raw_logs: false,
        contains_profile_names: false,
        contains_profile_urls: false,
        contains_connection_targets: false,
        contains_controller_secret: false,
    }
}

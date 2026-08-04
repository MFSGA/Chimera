use super::{
    AgentActionRequest, AgentConnectorState, AgentCoreState, AgentNetworkProbeRequest,
    AgentOsFamily, AgentPrivacyBoundary, AgentProbeCode, AgentRunType, AgentSelectedCore,
    AgentTelemetrySnapshot,
};
use crate::features::agent::AgentCommandError;

#[test]
fn os_family_projects_to_a_closed_public_enum() {
    let cases = [
        ("windows", AgentOsFamily::Windows, "windows"),
        ("macos", AgentOsFamily::Macos, "macos"),
        ("ios", AgentOsFamily::Ios, "ios"),
        ("linux", AgentOsFamily::Linux, "linux"),
        ("android", AgentOsFamily::Android, "android"),
        ("freebsd", AgentOsFamily::Freebsd, "freebsd"),
        ("dragonfly", AgentOsFamily::Dragonfly, "dragonfly"),
        ("openbsd", AgentOsFamily::Openbsd, "openbsd"),
        ("netbsd", AgentOsFamily::Netbsd, "netbsd"),
        ("future-os", AgentOsFamily::Unknown, "unknown"),
    ];

    for (source, expected, serialized) in cases {
        let projected = AgentOsFamily::from_name(source);
        assert_eq!(projected, expected);
        assert_eq!(serde_json::to_value(projected).unwrap(), serialized);
    }
}

#[test]
fn selected_core_serializes_to_a_closed_public_enum() {
    let cases = [
        (AgentSelectedCore::Clash, "clash"),
        (AgentSelectedCore::ClashRs, "clash-rs"),
        (AgentSelectedCore::Mihomo, "mihomo"),
        (AgentSelectedCore::ChimeraClient, "chimera-client"),
        (AgentSelectedCore::MihomoAlpha, "mihomo-alpha"),
        (AgentSelectedCore::ClashRsAlpha, "clash-rs-alpha"),
    ];

    for (core, serialized) in cases {
        assert_eq!(serde_json::to_value(core).unwrap(), serialized);
    }
}

#[test]
fn unknown_runtime_state_serializes_to_stable_closed_codes() {
    assert_eq!(
        serde_json::to_value(AgentCoreState::Unknown).unwrap(),
        "unknown"
    );
    assert_eq!(
        serde_json::to_value(AgentRunType::Unknown).unwrap(),
        "unknown"
    );
    assert_eq!(
        serde_json::to_value(AgentProbeCode::CoreStatusTimeout).unwrap(),
        "core_status_timeout"
    );
}

#[test]
fn privacy_boundary_serializes_only_negative_assertions() {
    let serialized = serde_json::to_value(AgentPrivacyBoundary::privacy_safe()).unwrap();

    assert_eq!(
        serialized,
        serde_json::json!({
            "contains_raw_logs": false,
            "contains_profile_names": false,
            "contains_profile_urls": false,
            "contains_connection_targets": false,
            "contains_controller_secret": false,
        })
    );
}

#[test]
fn telemetry_totals_serialize_as_bounded_numbers() {
    let telemetry = AgentTelemetrySnapshot {
        state: AgentConnectorState::Connected,
        active_connection_count: Some(1),
        upload_speed: Some(2),
        download_speed: Some(3),
        upload_total: Some(4),
        download_total: Some(5),
        recent_error_count: 0,
    };
    let serialized = serde_json::to_value(telemetry).unwrap();

    assert_eq!(serialized["upload_total"], 4);
    assert_eq!(serialized["download_total"], 5);
    assert!(serialized["upload_total"].is_number());
    assert!(serialized["download_total"].is_number());
}

#[test]
fn public_requests_reject_unknown_fields() {
    let probe = serde_json::from_value::<AgentNetworkProbeRequest>(serde_json::json!({
        "url": "https://example.com",
        "expected_status": 204,
        "timeout_ms": 1_000,
        "controller_secret": "sensitive-canary"
    }));
    assert!(probe.is_err(), "probe request must reject unknown fields");

    let action = serde_json::from_value::<AgentActionRequest>(serde_json::json!({
        "action": "start_core",
        "raw_logs": "sensitive-canary"
    }));
    assert!(action.is_err(), "action request must reject unknown fields");

    let valid_probe = serde_json::from_value::<AgentNetworkProbeRequest>(serde_json::json!({
        "url": "https://example.com",
        "expected_status": 204,
        "timeout_ms": 1_000
    }));
    assert!(valid_probe.is_ok());

    let valid_action = serde_json::from_value::<AgentActionRequest>(serde_json::json!({
        "action": "start_core"
    }));
    assert_eq!(valid_action.unwrap(), AgentActionRequest::StartCore);

    let valid_tun_action = serde_json::from_value::<AgentActionRequest>(serde_json::json!({
        "action": "set_tun_enabled",
        "enabled": true
    }));
    assert_eq!(
        valid_tun_action.unwrap(),
        AgentActionRequest::SetTunEnabled { enabled: true }
    );
    assert!(
        serde_json::from_value::<AgentActionRequest>(serde_json::json!({
            "action": "set_tun_enabled"
        }))
        .is_err(),
        "TUN action must require an explicit target"
    );
    assert!(
        serde_json::from_value::<AgentActionRequest>(serde_json::json!({
            "action": "set_tun_enabled",
            "enabled": false,
            "patch": { "secret": "sensitive-canary" }
        }))
        .is_err(),
        "TUN action must reject arbitrary patch fields"
    );
}

#[test]
fn command_errors_serialize_to_their_stable_public_codes() {
    let cases = [
        (
            AgentCommandError::ActionNotAvailable,
            "agent_action_not_available",
        ),
        (
            AgentCommandError::ProposalNotFound,
            "agent_proposal_not_found",
        ),
        (AgentCommandError::ProposalExpired, "agent_proposal_expired"),
        (
            AgentCommandError::ProposalDigestMismatch,
            "agent_proposal_digest_mismatch",
        ),
        (
            AgentCommandError::NetworkStateChanged,
            "agent_network_state_changed",
        ),
        (
            AgentCommandError::ProposalRateLimited,
            "agent_proposal_rate_limited",
        ),
        (
            AgentCommandError::ProposalLimitReached,
            "agent_proposal_limit_reached",
        ),
        (
            AgentCommandError::ConfirmationDeclined,
            "agent_confirmation_declined",
        ),
        (AgentCommandError::ActionFailed, "agent_action_failed"),
        (
            AgentCommandError::PartialApply,
            "agent_action_partially_applied",
        ),
        (
            AgentCommandError::VerificationFailed,
            "agent_action_verification_failed",
        ),
        (
            AgentCommandError::BridgeStartFailed,
            "agent_bridge_start_failed",
        ),
        (
            AgentCommandError::HistoryClearFailed,
            "agent_history_clear_failed",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert_eq!(serde_json::to_value(error).unwrap(), expected);
    }
}

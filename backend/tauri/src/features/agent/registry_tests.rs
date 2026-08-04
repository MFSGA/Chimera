use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use serde_json::json;

use super::{
    AGENT_TOOL_INPUT_SCHEMA_VERSION, AGENT_TOOL_VERSION, AGENT_TOOLS, AgentToolErrorCode,
    AgentToolInput, MAX_NETWORK_PROBE_URL_BYTES, MAX_RESOLVED_ADDRESSES,
    NETWORK_PROBE_REQUEST_TIMEOUT_MS, agent_manifest, collect_safe_addresses, is_blocked_hostname,
    is_blocked_ip, parse_empty_request, serialize_tool_result, tool_definition, tool_timeout,
    validate_probe_request, validate_tool_request,
};
use crate::features::agent::model::{AGENT_MANIFEST_SCHEMA_VERSION, AgentNetworkProbeRequest};

fn request(url: &str) -> AgentNetworkProbeRequest {
    AgentNetworkProbeRequest {
        url: url.into(),
        expected_status: Some(200),
        timeout_ms: Some(1_000),
    }
}

#[test]
fn probe_rejects_non_http_credentials_and_local_names() {
    assert!(validate_probe_request(request("ftp://example.com")).is_err());
    assert!(validate_probe_request(request("https://user@example.com")).is_err());
    assert!(validate_probe_request(request("http://localhost")).is_err());
    assert!(validate_probe_request(request("http://service.internal")).is_err());
}

#[test]
fn probe_rejects_private_and_special_ip_literals() {
    for url in [
        "http://127.0.0.1",
        "http://10.0.0.1",
        "http://169.254.169.254",
        "http://192.168.1.1",
        "http://[::1]",
        "http://[fc00::1]",
    ] {
        assert!(validate_probe_request(request(url)).is_err(), "{url}");
    }
    assert!(validate_probe_request(request("https://8.8.8.8")).is_ok());
}

#[test]
fn probe_validates_status_timeout_and_url_ranges() {
    let mut invalid_status = request("https://8.8.8.8");
    invalid_status.expected_status = Some(99);
    assert!(validate_probe_request(invalid_status).is_err());

    let mut invalid_timeout = request("https://8.8.8.8");
    invalid_timeout.timeout_ms = Some(999);
    assert!(validate_probe_request(invalid_timeout).is_err());

    let mut maximum_timeout = request("https://8.8.8.8");
    maximum_timeout.timeout_ms = Some(NETWORK_PROBE_REQUEST_TIMEOUT_MS);
    assert!(validate_probe_request(maximum_timeout).is_ok());

    let mut excessive_timeout = request("https://8.8.8.8");
    excessive_timeout.timeout_ms = Some(NETWORK_PROBE_REQUEST_TIMEOUT_MS + 1);
    assert!(validate_probe_request(excessive_timeout).is_err());

    let excessive_url = format!(
        "https://8.8.8.8/{}",
        "a".repeat(MAX_NETWORK_PROBE_URL_BYTES)
    );
    let error = match validate_probe_request(request(&excessive_url)) {
        Ok(_) => panic!("oversized URL must be rejected before parsing or resolution"),
        Err(error) => error,
    };
    assert_eq!(error.code, AgentToolErrorCode::InvalidRequest);
    assert!(
        tool_timeout("network.probe").expect("registered network probe timeout")
            > Duration::from_millis(u64::from(NETWORK_PROBE_REQUEST_TIMEOUT_MS))
    );
}

#[test]
fn probe_client_disables_redirects_proxies_and_dns_rebinding() {
    let source = include_str!("adapters/http_network_probe.rs");
    assert!(source.contains(".no_proxy()"));
    assert!(source.contains(".redirect(Policy::none())"));
    assert!(source.contains("client.resolve_to_addrs(domain, &addresses)"));
    assert!(source.contains("collect_safe_addresses(resolved)?"));
}

#[test]
fn address_filter_blocks_non_public_ranges() {
    assert!(is_blocked_hostname("printer.local."));
    assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
    assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1))));
    assert!(is_blocked_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    for address in [
        "::192.168.1.1",
        "::ffff:192.168.1.1",
        "100::1",
        "2001:2::1",
        "3fff::1",
        "5f00::1",
    ] {
        assert!(is_blocked_ip(address.parse().unwrap()), "{address}");
    }
    assert!(!is_blocked_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    assert!(!is_blocked_ip("2606:4700:4700::1111".parse().unwrap()));
}

#[test]
fn resolved_probe_addresses_are_deduplicated_bounded_and_public() {
    let public = (1..=MAX_RESOLVED_ADDRESSES + 4)
        .map(|last| SocketAddr::from(([1, 1, 1, last as u8], 443)))
        .chain([SocketAddr::from(([1, 1, 1, 1], 443))]);
    let addresses = collect_safe_addresses(public).expect("public addresses");
    assert_eq!(addresses.len(), MAX_RESOLVED_ADDRESSES);
    assert_eq!(addresses[0], SocketAddr::from(([1, 1, 1, 1], 443)));

    let blocked = collect_safe_addresses([
        SocketAddr::from(([1, 1, 1, 1], 443)),
        SocketAddr::from(([127, 0, 0, 1], 443)),
    ])
    .expect_err("mixed private resolution must fail closed");
    assert_eq!(blocked.code, AgentToolErrorCode::TargetBlocked);

    let empty = collect_safe_addresses([]).expect_err("empty resolution must fail");
    assert_eq!(empty.code, AgentToolErrorCode::ResolutionFailed);
}

#[test]
fn registry_definition_drives_manifest_timeout_validation_and_lookup() {
    let manifest = agent_manifest();
    assert_eq!(manifest.schema_version, AGENT_MANIFEST_SCHEMA_VERSION);
    assert_ne!(manifest.schema_version, 0);
    assert_eq!(manifest.tools.len(), AGENT_TOOLS.len());

    for (definition, tool) in AGENT_TOOLS.iter().zip(&manifest.tools) {
        assert_eq!(tool.name, definition.name);
        assert_eq!(tool.version, AGENT_TOOL_VERSION);
        assert_eq!(tool.description, definition.description);
        assert_eq!(tool.input_schema_version, AGENT_TOOL_INPUT_SCHEMA_VERSION);
        assert_eq!(tool.output_schema_version, definition.output_schema_version);
        assert_ne!(tool.output_schema_version, 0);
        assert_eq!(tool.timeout_ms, definition.timeout_ms);
        assert_eq!(
            tool_timeout(definition.name.as_str()),
            Some(std::time::Duration::from_millis(u64::from(
                definition.timeout_ms
            )))
        );
        assert_eq!(
            tool_definition(definition.name.as_str()).unwrap().kind,
            definition.kind
        );

        let body: &[u8] = match definition.input {
            AgentToolInput::Empty => b"{}",
            AgentToolInput::NetworkProbe => br#"{"arguments":{"url":"https://8.8.8.8"}}"#,
        };
        assert!(validate_tool_request(definition.name.as_str(), body).is_ok());
    }

    assert!(tool_timeout("not.registered").is_none());
    assert_eq!(
        tool_definition("not.registered").unwrap_err().code,
        AgentToolErrorCode::UnknownTool
    );
    assert_eq!(
        validate_tool_request("not.registered", b"{}")
            .unwrap_err()
            .code,
        AgentToolErrorCode::UnknownTool
    );
}

#[test]
fn manifest_registers_the_complete_read_only_tool_set() {
    let manifest = agent_manifest();
    let names = manifest
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "system.snapshot",
            "network.diagnose",
            "network.probe",
            "core.status",
            "proxy.status",
            "tun.status",
            "profile.summary",
            "service.status",
        ]
    );
    assert!(manifest.tools.iter().all(|tool| {
        tool.read_only
            && tool.requires_authentication
            && tool.risk == crate::features::agent::model::AgentToolRisk::ReadOnly
    }));
}

#[test]
fn public_output_rejects_nested_sensitive_keys() {
    for key in [
        "token",
        "bearerToken",
        "controller_secret",
        "subscription_url",
        "profileUrls",
        "host",
        "target_address",
        "connectionTarget",
        "logs",
        "raw_log",
        "proxy_bypass",
    ] {
        let result = serialize_tool_result(
            json!({ "safe": [{ key: "sensitive-canary" }] }),
            "serialization failed",
            &["safe"],
        );
        let error = result.expect_err("sensitive-shaped output key must be rejected");
        assert_eq!(error.code, AgentToolErrorCode::ExecutionFailed);
        assert_eq!(
            error.message,
            "agent tool output violates the privacy contract"
        );
    }
}

#[test]
fn public_output_rejects_unregistered_top_level_fields() {
    let result = serialize_tool_result(
        json!({ "status": 204, "diagnostic_note": "unexpected" }),
        "serialization failed",
        &["status"],
    );
    let error = result.expect_err("unregistered output fields must be rejected");
    assert_eq!(error.code, AgentToolErrorCode::ExecutionFailed);
    assert_eq!(
        error.message,
        "agent tool output violates the privacy contract"
    );
}

#[test]
fn public_output_allows_only_negative_privacy_assertions() {
    let value = serialize_tool_result(
        json!({
            "privacy": {
                "contains_raw_logs": false,
                "contains_profile_names": false,
                "contains_profile_urls": false,
                "contains_connection_targets": false,
                "contains_controller_secret": false
            },
            "probe_failure_codes": ["telemetry_unavailable"]
        }),
        "serialization failed",
        &["privacy", "probe_failure_codes"],
    )
    .expect("negative privacy assertions are safe");

    assert_eq!(value["privacy"]["contains_raw_logs"], false);

    for unsafe_value in [json!(true), json!(null), json!("false"), json!(0)] {
        let result = serialize_tool_result(
            json!({ "privacy": { "contains_raw_logs": unsafe_value } }),
            "serialization failed",
            &["privacy"],
        );
        let error = result.expect_err("privacy assertions must be boolean false");
        assert_eq!(error.code, AgentToolErrorCode::ExecutionFailed);
        assert_eq!(
            error.message,
            "agent tool output violates the privacy contract"
        );
    }
}

#[test]
fn empty_tools_accept_only_an_empty_envelope() {
    assert!(parse_empty_request(b"").is_ok());
    assert!(parse_empty_request(br#"{}"#).is_ok());
    assert!(parse_empty_request(br#"{"arguments":{}}"#).is_ok());
    assert!(parse_empty_request(br#"{"unexpected":true}"#).is_err());
    assert!(parse_empty_request(br#"{"arguments":{"unexpected":true}}"#).is_err());
}

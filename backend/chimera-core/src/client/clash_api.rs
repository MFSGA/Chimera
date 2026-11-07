//! Pure Clash protocol-to-application DTO projections. No Tauri access.
//! Keep the instance-bound application client in the app crate until its
//! controller ownership is extracted into the independent core.

use crate::clash::api::{ClashApiConfig, RuleProviderItem};
use anyhow::Result;

/// Convert the native protocol DTO at the facade boundary rather than inside
/// the instance-bound API capability. Check every port narrowing explicitly.
pub fn config_item(config: clash_api::RuntimeConfig) -> Result<ClashApiConfig> {
    Ok(ClashApiConfig {
        port: config.port.map(u16::try_from).transpose()?,
        socket_port: config.socket_port.map(u16::try_from).transpose()?,
        socks_port: config.socks_port.map(u16::try_from).transpose()?,
        mixed_port: config.mixed_port.map(u16::try_from).transpose()?,
        redir_port: config.redir_port.map(u16::try_from).transpose()?,
        tproxy_port: config.tproxy_port.map(u16::try_from).transpose()?,
        mode: config.mode.map(|value| value.as_str().to_owned()),
        log_level: config.log_level.map(|value| value.as_str().to_owned()),
        allow_lan: config.allow_lan,
        ipv6: config.ipv6,
        external_controller: config.external_controller,
        secret: config.secret,
    })
}

pub fn rule_provider_item(provider: clash_api::RuleProvider) -> Result<RuleProviderItem> {
    Ok(RuleProviderItem {
        name: provider.name.as_str().to_owned(),
        behavior: provider.behavior.map(|value| value.as_str().to_owned()),
        format: provider.format.map(|value| value.as_str().to_owned()),
        rule_count: provider.rule_count.map(u32::try_from).transpose()?,
        r#type: provider
            .provider_type
            .map(|value| value.as_str().to_owned()),
        vehicle_type: provider.vehicle_type.map(|value| value.as_str().to_owned()),
        updated_at: provider.updated_at.map(|value| value.to_rfc3339()),
    })
}

#[cfg(test)]
mod tests {
    use super::config_item;

    #[test]
    fn config_projection_matches_ref_optional_fields_and_unknown_modes() {
        let native = serde_json::from_value(serde_json::json!({
            "mode": "future-mode",
            "log-level": "trace",
            "mixed-port": 0,
            "port": 65535,
            "socket-port": 100,
            "ipv6": false,
            "external-controller": "127.0.0.1:9090",
            "secret": "fixture-secret"
        }))
        .unwrap();
        let config = config_item(native).unwrap();
        assert_eq!(config.mode.as_deref(), Some("future-mode"));
        assert_eq!(config.log_level.as_deref(), Some("trace"));
        assert_eq!(config.mixed_port, Some(0));
        assert_eq!(config.port, Some(65535));
        assert_eq!(config.socket_port, Some(100));
        assert_eq!(config.ipv6, Some(false));
        assert_eq!(config.secret.as_deref(), Some("fixture-secret"));
        assert_eq!(config.socks_port, None);
        assert_eq!(config.allow_lan, None);
        let encoded = serde_json::to_value(config).unwrap();
        assert_eq!(encoded["mixed-port"], 0);
        assert_eq!(encoded["log-level"], "trace");
        assert!(encoded.get("geodata-mode").is_none());
        assert!(encoded.get("tun").is_none());
    }

    #[test]
    fn config_projection_rejects_out_of_range_ports_in_native_protocol_values() {
        for field in [
            "port",
            "socket-port",
            "socks-port",
            "mixed-port",
            "redir-port",
            "tproxy-port",
        ] {
            for port in [-1, 65536] {
                let mut value = serde_json::json!({});
                value[field] = port.into();
                let native = serde_json::from_value(value).unwrap();
                assert!(config_item(native).is_err(), "{field}={port} must fail");
            }
        }
    }
}

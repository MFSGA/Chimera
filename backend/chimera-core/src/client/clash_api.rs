//! Running-core Clash operations and protocol-to-application DTO projections.

use super::ChimeraClient;
use crate::{
    actor_v2::api::ApiClient,
    clash::{
        api::{
            ClashApiConfig, ClashRule, ClashVersion, ProvidersRulesRes, RuleProviderItem, RulesRes,
        },
        proxies::{Proxies, ProxyProviderSummary},
    },
};
use anyhow::Result;
use clash_api::{DelayQuery, ExpectedStatus};
use std::time::Duration;

/// Used only if neither the caller nor the app supplies a test URL.
pub const FALLBACK_LATENCY_TEST_URL: &str = "http://www.gstatic.com/generate_204";

/// Build one typed test query for direct, group and provider delay endpoints.
///
/// A caller-specified nonempty URL wins over the app default. The test timeout
/// is always supplied explicitly by the caller; an invalid expected-status
/// expression fails before any request is issued.
pub fn delay_query(
    url: Option<&str>,
    expected: Option<&str>,
    app_default_url: &str,
    timeout: Duration,
) -> Result<DelayQuery> {
    let url = url
        .filter(|value| !value.is_empty())
        .or_else(|| (!app_default_url.is_empty()).then_some(app_default_url))
        .unwrap_or(FALLBACK_LATENCY_TEST_URL);
    let query = DelayQuery::new(url.parse()?, timeout)?;
    Ok(match expected.filter(|value| !value.is_empty()) {
        Some(value) => query.with_expected(ExpectedStatus::new(value)?),
        None => query,
    })
}

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

/// Why a Clash read or probe has no result, in a redacted response.
///
/// The messages are fixed and carry no source: the core's response, the request it failed on and
/// the private URLs behind a probe stay out of the error, and the raw diagnostics stay on the
/// protected full queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClashReadError {
    #[error("the Clash core settings are unavailable")]
    SettingsUnavailable,
    #[error("the proxies are unavailable")]
    ProxiesUnavailable,
    #[error("the proxy group does not exist")]
    UnknownGroup,
    #[error("the delay test could not be completed")]
    DelayTestFailed,
    #[error("the Clash core version is unavailable")]
    VersionUnavailable,
    #[error("the rules are unavailable")]
    RulesUnavailable,
    #[error("the rule providers are unavailable")]
    RuleProvidersUnavailable,
    #[error("the proxy providers are unavailable")]
    ProxyProvidersUnavailable,
    #[error("the Clash streams are unavailable")]
    StreamsUnavailable,
}

impl ChimeraClient {
    pub async fn clash_api_client(&self) -> Result<ApiClient> {
        if let Some(facade) = self.ref_facade() {
            let client = facade.api_client_if_running().await?.ok_or_else(|| {
                anyhow::anyhow!("the ref Core Facade confirmed the core is stopped")
            })?;
            return Ok(client);
        }
        if let Some(core) = self.core_api() {
            // Test fixtures can inject CoreActor without a ServiceActor;
            // production always constructs the shared ref Facade above.
            return Ok(core.api_client().await?);
        }
        // Only test fixtures constructed from injected legacy ports take this
        // fallback. The production Client always receives ref CoreClient.
        self.core().clash_api_client().await
    }

    pub async fn clash_configs(&self) -> Result<ClashApiConfig> {
        config_item(self.clash_api_client().await?.configs().await?)
    }

    pub async fn clash_version(&self) -> Result<ClashVersion> {
        let version = self.clash_api_client().await?.version().await?;
        Ok(ClashVersion {
            version: version.version,
            premium: version.premium,
            meta: Some(version.meta),
        })
    }

    pub async fn clash_api_query_dns(
        &self,
        name: String,
        record_type: String,
    ) -> Result<clash_api::DnsResponse> {
        let record_type = clash_api::DnsRecordType::new(record_type)?;
        let query = clash_api::DnsQuery::new(name, record_type)?;
        Ok(self.clash_api_client().await?.dns_query(&query).await?)
    }

    pub async fn clash_api_flush_dns_cache(&self) -> Result<()> {
        self.clash_api_client().await?.flush_dns_cache().await?;
        Ok(())
    }

    pub async fn clash_api_flush_fake_ip_cache(&self) -> Result<()> {
        self.clash_api_client().await?.flush_fake_ip_cache().await?;
        Ok(())
    }

    pub async fn clash_rules(&self) -> Result<RulesRes> {
        let rules = self.clash_api_client().await?.rules().await?;
        Ok(RulesRes {
            rules: rules
                .into_iter()
                .map(|rule| ClashRule {
                    r#type: rule.rule_type,
                    payload: rule.payload,
                    proxy: rule.proxy,
                })
                .collect(),
        })
    }

    pub async fn clash_rule_providers(&self) -> Result<ProvidersRulesRes> {
        let providers = self.clash_api_client().await?.rule_providers().await?;
        Ok(ProvidersRulesRes {
            providers: providers
                .into_iter()
                .map(|(name, provider)| {
                    Ok((name.as_str().to_owned(), rule_provider_item(provider)?))
                })
                .collect::<Result<_>>()?,
        })
    }

    pub async fn update_clash_rule_provider(&self, name: String) -> Result<()> {
        self.clash_api_client()
            .await?
            .update_rule_provider(&clash_api::RuleProviderName::new(name))
            .await
            .map_err(Into::into)
    }

    pub async fn clash_native_proxies(&self) -> Result<crate::clash::proxies::Proxies> {
        let default_url = self.get_app_config()?.default_latency_test;
        Ok(self
            .proxies()
            .get(false)
            .await?
            .trim_for_frontend(&default_url))
    }

    /// Separate icon URLs from ordinary native proxy graph reads. This app's
    /// existing desktop IPC currently has no ref-style per-scope permissions;
    /// adding an icon command does not itself establish an authorization gate.
    pub async fn clash_proxy_group_icons(
        &self,
    ) -> Result<indexmap::IndexMap<clash_api::ProxyName, String>> {
        Ok(self.proxies().get(false).await?.group_icons())
    }

    pub async fn proxy_providers(
        &self,
    ) -> Result<indexmap::IndexMap<clash_api::ProviderName, ProxyProviderSummary>> {
        self.proxies().providers().await
    }

    pub async fn update_proxy_provider(&self, name: String) -> Result<()> {
        self.proxies().update_provider(name).await
    }

    pub async fn healthcheck_proxy_provider(&self, name: String) -> Result<()> {
        self.proxies().healthcheck_provider(name).await
    }

    pub async fn select_proxy(
        &self,
        group: String,
        name: String,
        strategy: chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode,
    ) -> Result<crate::proxies::ProxyMutationOutcome> {
        self.proxies().select(group, name, strategy).await
    }

    pub async fn clear_proxy_fixed(
        &self,
        group: String,
        strategy: chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode,
    ) -> Result<crate::proxies::ProxyMutationOutcome> {
        self.proxies().clear_fixed(group, strategy).await
    }

    pub fn request_proxy_refresh(&self) {
        self.proxies().request_refresh();
    }

    pub fn subscribe_proxy_changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.proxies().subscribe()
    }

    pub fn notify_proxy_rearranged(&self) {
        self.proxies().notify_rearranged();
    }

    pub async fn proxy_delay(
        &self,
        name: String,
        provider: Option<String>,
        url: Option<String>,
        expected: Option<String>,
    ) -> Result<clash_api::Delay> {
        let query = delay_query(
            url.as_deref(),
            expected.as_deref(),
            &self.get_app_config()?.default_latency_test,
            Duration::from_secs(10),
        )?;
        let provider = provider
            .filter(|value| !value.is_empty())
            .map(clash_api::ProviderName::new);
        Ok(self
            .clash_api_client()
            .await?
            .proxy_delay(&clash_api::ProxyName::new(name), provider.as_ref(), &query)
            .await?)
    }

    pub async fn group_delay(
        &self,
        group: String,
        url: Option<String>,
        expected: Option<String>,
    ) -> Result<indexmap::IndexMap<clash_api::ProxyName, u16>> {
        let query = delay_query(
            url.as_deref(),
            expected.as_deref(),
            &self.get_app_config()?.default_latency_test,
            Duration::from_secs(10),
        )?;
        Ok(self
            .clash_api_client()
            .await?
            .group_delay(&clash_api::ProxyName::new(group), &query)
            .await?)
    }

    /// Native UI names a member, not its potentially private test URL.
    pub async fn test_native_proxy_node_delay(&self, group: String, name: String) -> Result<u16> {
        let default_url = self.get_app_config()?.default_latency_test;
        let api = self.clash_api_client().await?;
        let result = crate::proxies::test_group_node_delay(
            &api,
            &group,
            &name,
            &default_url,
            Duration::from_secs(10),
        )
        .await;
        self.request_proxy_refresh();
        result
    }

    pub async fn test_native_proxy_group_delay(
        &self,
        group: String,
    ) -> Result<indexmap::IndexMap<clash_api::ProxyName, u16>> {
        let default_url = self.get_app_config()?.default_latency_test;
        let api = self.clash_api_client().await?;
        crate::proxies::test_group_delay(
            &api,
            &clash_api::ProxyName::new(group),
            &default_url,
            Duration::from_secs(10),
        )
        .await
    }

    pub async fn close_clash_connections(&self, id: Option<String>) -> Result<()> {
        Ok(self
            .clash_api_client()
            .await?
            .delete_connections(id.as_deref())
            .await?)
    }
}

impl ChimeraClient {
    /// Redacted settings and fixed errors, suitable for future admitted read callers.
    pub async fn clash_api_settings(
        &self,
    ) -> Result<crate::clash::api::ClashApiSettings, ClashReadError> {
        public_settings(self.clash_configs().await)
    }

    pub async fn get_proxies(&self) -> Result<Proxies, ClashReadError> {
        self.read_proxies(false).await
    }

    pub async fn refresh_proxies(&self) -> Result<Proxies, ClashReadError> {
        self.read_proxies(true).await
    }

    async fn read_proxies(&self, force: bool) -> Result<Proxies, ClashReadError> {
        let proxies = self
            .proxies()
            .get(force)
            .await
            .map_err(|_| ClashReadError::ProxiesUnavailable)?;
        let url = self
            .get_app_config()
            .map_err(|_| ClashReadError::ProxiesUnavailable)?
            .default_latency_test;
        let default_url = if url.is_empty() {
            FALLBACK_LATENCY_TEST_URL
        } else {
            &url
        };
        Ok(proxies.trim_for_frontend(default_url))
    }
}

fn public_settings(
    configs: Result<ClashApiConfig>,
) -> Result<crate::clash::api::ClashApiSettings, ClashReadError> {
    configs
        .map(Into::into)
        .map_err(|_| ClashReadError::SettingsUnavailable)
}

#[cfg(test)]
mod tests {
    use super::{ClashReadError, config_item, rule_provider_item};

    #[test]
    fn a_failed_live_settings_read_carries_neither_the_source_nor_its_context() {
        let failure = anyhow::anyhow!("GET https://user:error-sentinel@127.0.0.1:9090/configs")
            .context("body: {\"secret\":\"error-sentinel\"}");

        let error = super::public_settings(Err(failure)).unwrap_err();

        assert_eq!(error, ClashReadError::SettingsUnavailable);
        for rendered in [error.to_string(), format!("{error:?}")] {
            assert!(!rendered.contains("sentinel"), "{rendered}");
        }
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn delay_query_prefers_caller_then_app_default_then_fallback() {
        use std::time::Duration;
        let timeout = Duration::from_millis(4200);
        let direct = super::delay_query(
            Some("https://example.test/check"),
            None,
            "https://app.example.test/generate_204",
            timeout,
        )
        .unwrap();
        assert_eq!(direct.url.as_str(), "https://example.test/check");
        assert_eq!(direct.timeout, timeout);
        assert!(direct.expected.is_none());

        let app_default = super::delay_query(
            Some(""),
            None,
            "https://app.example.test/generate_204",
            timeout,
        )
        .unwrap();
        assert_eq!(
            app_default.url.as_str(),
            "https://app.example.test/generate_204"
        );

        let fallback = super::delay_query(None, None, "", timeout).unwrap();
        assert_eq!(fallback.url.as_str(), super::FALLBACK_LATENCY_TEST_URL);
    }

    #[test]
    fn delay_query_validates_expected_status_and_timeout_before_transport() {
        use std::time::Duration;
        let query = super::delay_query(
            None,
            Some("200/204/401-429"),
            "",
            Duration::from_millis(3000),
        )
        .unwrap();
        assert_eq!(query.expected.unwrap().as_str(), "200/204/401-429");

        assert!(
            super::delay_query(None, Some(""), "", Duration::from_secs(3))
                .unwrap()
                .expected
                .is_none()
        );
        assert!(
            super::delay_query(None, Some("not a status"), "", Duration::from_secs(3)).is_err()
        );
        assert!(super::delay_query(None, None, "", Duration::ZERO).is_err());
        assert!(super::delay_query(None, None, "", Duration::from_secs(10_000_000)).is_err());
    }

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
    #[test]
    fn provider_dto_preserves_unknown_values_and_absence() {
        let provider = serde_json::from_value(serde_json::json!({
            "name":"provider", "behavior":"future", "format":"format-v2",
            "type":"remote-v2", "vehicleType":"transport-v2",
            "updatedAt":"2026-09-07T10:00:00+08:00"
        }))
        .unwrap();
        let item = serde_json::to_value(rule_provider_item(provider).unwrap()).unwrap();
        assert_eq!(
            item,
            serde_json::json!({
                "name":"provider", "behavior":"future", "format":"format-v2",
                "type":"remote-v2", "vehicleType":"transport-v2", "ruleCount":null,
                "updatedAt":"2026-09-07T10:00:00+08:00"
            })
        );
    }

    #[test]
    fn provider_dto_rejects_counts_outside_the_ui_contract() {
        for count in [-1, i64::from(u32::MAX) + 1] {
            let provider =
                serde_json::from_value(serde_json::json!({"name":"p","ruleCount":count})).unwrap();
            assert!(rule_provider_item(provider).is_err());
        }
    }
    #[test]
    fn settings_projection_omits_secret_and_rejects_non_socket_controller() {
        let config = super::ClashApiConfig {
            secret: Some("secret-sentinel".into()),
            external_controller: Some("https://user:private-sentinel@example.test".into()),
            ..Default::default()
        };
        let settings = super::public_settings(Ok(config)).unwrap();
        let json = serde_json::to_value(settings).unwrap();
        assert!(json.get("secret").is_none());
        assert!(json["external-controller"].is_null());
        assert!(!json.to_string().contains("sentinel"));
    }
}

//! Running-core Clash API capability owned by ChimeraClient.
//!
//! Production Clash API reads acquire the ref CoreActor's instance-bound
//! capability directly. Compatibility tests may still inject a legacy port.

#[cfg(test)]
use std::sync::Arc;

use anyhow::Result;

use super::ChimeraClient;
#[cfg(test)]
use super::core_lifecycle::CoreLifecyclePort;
use crate::client::runtime::{Degradation, MutationOutcome};
use crate::core::{
    actor_v2::api::ApiClient,
    clash::{
        api::{ClashApiConfig, ClashRule, ClashVersion, ProvidersRulesRes, RulesRes},
        proxies::ProxyProviderSummary,
    },
};
#[cfg(test)]
use chimera_core::clash::ws::ClashApiClientSource;
use chimera_core::client::clash_api::{config_item, rule_provider_item};
use chimera_core::proxies_actor::ProxyMutationOutcome;
#[cfg(test)]
use chimera_core::proxies_actor::{CoreClient as ProxyCoreClient, ProxyApiClientSource};

/// Only injected legacy test clients need a facade-based API source.
#[cfg(test)]
struct LifecycleProxyApiClientSource(Arc<dyn CoreLifecyclePort>);

#[cfg(test)]
#[async_trait::async_trait]
impl ProxyApiClientSource for LifecycleProxyApiClientSource {
    async fn api_client(&self) -> Result<chimera_core::actor_v2::api::ApiClient> {
        Ok(self.0.clash_api_client().await?.core_client())
    }
}

#[cfg(test)]
pub(crate) fn proxy_core_client(core: Arc<dyn CoreLifecyclePort>) -> ProxyCoreClient {
    ProxyCoreClient::new(Arc::new(LifecycleProxyApiClientSource(core)))
}

fn into_mutation_outcome(outcome: ProxyMutationOutcome) -> MutationOutcome<()> {
    // The shared core now returns ref's typed Committed/CommittedDegraded
    // outcome. Only the existing Tauri legacy wire still needs a flat code.
    let (_, typed) = outcome.into_parts();
    let degradations = typed.into_iter().map(Degradation::from_shared).collect();
    MutationOutcome::from_parts((), degradations)
}

#[cfg(test)]
struct LifecycleStreamsApiClientSource(Arc<dyn CoreLifecyclePort>);

#[cfg(test)]
#[async_trait::async_trait]
impl ClashApiClientSource for LifecycleStreamsApiClientSource {
    async fn api_client(&self) -> Result<chimera_core::actor_v2::api::ApiClient> {
        Ok(self.0.clash_api_client().await?.core_client())
    }
}

#[cfg(test)]
pub(crate) fn streams_api_client_source(
    core: Arc<dyn CoreLifecyclePort>,
) -> Arc<dyn ClashApiClientSource> {
    Arc::new(LifecycleStreamsApiClientSource(core))
}

impl ChimeraClient {
    /// A fresh ref CoreActor projection for diagnostics that must not trust
    /// the legacy Facade's controller cache. The caller is responsible for
    /// checking host/generation before and after using a leased API client.
    pub(crate) async fn refreshed_routed_core_status(
        &self,
    ) -> Result<chimera_core::actor_v2::router::CoreStatusProjection> {
        if let Some(facade) = &self.inner.ref_facade {
            return Ok(facade.refresh_status().await?);
        }
        let core = self
            .inner
            .core_api
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the ref Core Actor is unavailable"))?;
        Ok(core.refresh_status().await?)
    }

    pub(crate) async fn clash_api_client(&self) -> Result<ApiClient> {
        if let Some(facade) = &self.inner.ref_facade {
            let client = facade.api_client_if_running().await?.ok_or_else(|| {
                anyhow::anyhow!("the ref Core Facade confirmed the core is stopped")
            })?;
            return Ok(ApiClient::from_core_client(client));
        }
        if let Some(core) = &self.inner.core_api {
            // Test fixtures can inject CoreActor without a ServiceActor;
            // production always constructs the shared ref Facade above.
            return Ok(ApiClient::from_core_client(core.api_client().await?));
        }
        // Only test fixtures constructed from injected legacy ports take this
        // fallback. The production Client always receives ref CoreClient.
        self.inner.core.clash_api_client().await
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

    /// Read the single actor-owned proxy snapshot and project it to the
    /// existing Chimera legacy UI DTO at the IPC boundary.
    pub async fn clash_proxies(&self) -> Result<crate::core::clash::proxies::Proxies> {
        let native = self.inner.proxies.get(false).await?;
        Ok(crate::core::clash::proxies::Proxies::from_native(native))
    }

    pub async fn clash_native_proxies(
        &self,
    ) -> Result<chimera_core::clash::proxies::NativeProxies> {
        let default_url = self.get_app_config()?.default_latency_test;
        Ok(self
            .inner
            .proxies
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
        Ok(self.inner.proxies.get(false).await?.group_icons())
    }

    pub async fn proxy_providers(
        &self,
    ) -> Result<indexmap::IndexMap<clash_api::ProviderName, ProxyProviderSummary>> {
        self.inner.proxies.providers().await
    }

    pub async fn update_proxy_provider(&self, name: String) -> Result<()> {
        self.inner.proxies.update_provider(name).await
    }

    pub async fn healthcheck_proxy_provider(&self, name: String) -> Result<()> {
        self.inner.proxies.healthcheck_provider(name).await
    }

    pub async fn select_proxy(
        &self,
        group: String,
        name: String,
        strategy: chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode,
    ) -> Result<crate::client::runtime::MutationOutcome<()>> {
        Ok(into_mutation_outcome(
            self.inner.proxies.select(group, name, strategy).await?,
        ))
    }

    pub async fn clear_proxy_fixed(
        &self,
        group: String,
        strategy: chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode,
    ) -> Result<crate::client::runtime::MutationOutcome<()>> {
        Ok(into_mutation_outcome(
            self.inner.proxies.clear_fixed(group, strategy).await?,
        ))
    }

    pub(crate) fn request_proxy_refresh(&self) {
        self.inner.proxies.request_refresh();
    }

    pub(crate) fn subscribe_proxy_changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.inner.proxies.subscribe()
    }

    pub(crate) fn notify_proxy_rearranged(&self) {
        self.inner.proxies.notify_rearranged();
    }
}

#[cfg(test)]
mod shared_proxy_outcome_tests {
    use super::*;
    use chimera_contracts::runtime::{
        Degradation as SharedDegradation, DegradationPhase as SharedPhase,
        DegradationReason as SharedReason, InterruptFailure,
    };

    #[test]
    fn shared_typed_proxy_reason_keeps_the_existing_desktop_wire() {
        let typed = ProxyMutationOutcome::from_parts(
            (),
            vec![SharedDegradation {
                phase: SharedPhase::SystemEffect,
                reason: SharedReason::ProxyInterruptionFailed {
                    cause: InterruptFailure::Protocol,
                },
                message: "delete rejected".into(),
                retryable: false,
            }],
        );
        let projected = into_mutation_outcome(typed);
        let json = serde_json::to_value(&projected).unwrap();
        assert_eq!(json["status"], "committed_degraded");
        assert_eq!(json["degradations"][0]["code"], "proxy_interruption_failed");
        assert_eq!(json["degradations"][0]["phase"], "system_effect");
        assert_eq!(json["degradations"][0]["retryable"], false);
        assert!(json["degradations"][0].get("reason").is_none());
    }
}

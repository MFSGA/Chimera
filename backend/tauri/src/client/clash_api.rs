//! Running-core Clash API capability owned by ChimeraClient.
//!
//! This follows the reference client ownership direction and reads the active
//! runtime through the shared CoreFacade host boundary.

use std::sync::Arc;

use anyhow::Result;

use super::ChimeraClient;
use crate::client::core_lifecycle::CoreLifecyclePort;
use crate::core::{
    actor_v2::api::ApiClient,
    clash::{
        api::{ClashApiConfig, ClashRule, ClashVersion, ProvidersRulesRes, RulesRes},
        proxies::ProxyProviderSummary,
    },
};
use chimera_core::clash::ws::ClashApiClientSource;
use chimera_core::client::clash_api::{config_item, rule_provider_item};

struct LifecycleStreamsApiClientSource(Arc<dyn CoreLifecyclePort>);

#[async_trait::async_trait]
impl ClashApiClientSource for LifecycleStreamsApiClientSource {
    async fn api_client(&self) -> Result<chimera_core::actor_v2::api::ApiClient> {
        Ok(self.0.clash_api_client().await?.core_client())
    }
}

pub(crate) fn streams_api_client_source(
    core: Arc<dyn CoreLifecyclePort>,
) -> Arc<dyn ClashApiClientSource> {
    Arc::new(LifecycleStreamsApiClientSource(core))
}

impl ChimeraClient {
    pub(crate) async fn active_core_binding(
        &self,
    ) -> Result<crate::client::core_lifecycle::ports::ActiveCoreBinding> {
        self.inner.core.active_core_binding().await
    }

    pub(crate) async fn active_clash_info(&self) -> Result<crate::config::clash::ClashInfo> {
        self.inner.core.active_clash_info().await
    }

    pub(crate) async fn clash_api_client(&self) -> Result<ApiClient> {
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

    pub async fn proxy_providers(
        &self,
    ) -> Result<indexmap::IndexMap<clash_api::ProviderName, ProxyProviderSummary>> {
        let providers = self.clash_api_client().await?.proxy_providers().await?;
        Ok(providers
            .into_iter()
            .map(|(name, provider)| (name, provider.into()))
            .collect())
    }

    pub async fn update_proxy_provider(&self, name: String) -> Result<()> {
        self.clash_api_client()
            .await?
            .update_proxy_provider(&clash_api::ProviderName::new(name))
            .await
            .map_err(Into::into)
    }
}

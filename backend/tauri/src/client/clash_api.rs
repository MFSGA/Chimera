//! Compatibility projections at the desktop IPC boundary. All native
//! controller operations are implemented by the shared application facade.
use super::ChimeraClient;
#[cfg(test)]
use super::core_lifecycle::CoreLifecyclePort;
use crate::client::runtime::MutationOutcome;
use anyhow::Result;
#[cfg(test)]
use chimera_core::proxies::ProxyApiClientSource;
use chimera_core::proxies::ProxyMutationOutcome;
#[cfg(test)]
use std::sync::Arc;

/// Only injected legacy test clients need a facade-based API source.
#[cfg(test)]
struct LifecycleProxyApiClientSource(Arc<dyn CoreLifecyclePort>);

#[cfg(test)]
#[async_trait::async_trait]
impl ProxyApiClientSource for LifecycleProxyApiClientSource {
    async fn api_client(&self) -> Result<chimera_core::actor_v2::api::ApiClient> {
        self.0.clash_api_client().await
    }
}

#[cfg(test)]
pub(crate) fn proxy_core_client(core: Arc<dyn CoreLifecyclePort>) -> Arc<dyn ProxyApiClientSource> {
    Arc::new(LifecycleProxyApiClientSource(core))
}

fn into_mutation_outcome(outcome: ProxyMutationOutcome) -> MutationOutcome<()> {
    outcome.into()
}

impl ChimeraClient {
    pub(crate) async fn clash_api_client(&self) -> Result<crate::core::actor_v2::api::ApiClient> {
        Ok(crate::core::actor_v2::api::ApiClient::from_core_client(
            self.inner.shared.clash_api_client().await?,
        ))
    }

    /// Read the single actor-owned proxy snapshot and project it to the
    /// existing Chimera legacy UI DTO at the IPC boundary.
    pub async fn clash_proxies(&self) -> Result<crate::core::clash::proxies::LegacyProxies> {
        let native = self.inner.shared.proxies().get(false).await?;
        Ok(crate::core::clash::proxies::LegacyProxies::from_native(
            native,
        ))
    }

    pub async fn select_proxy(
        &self,
        group: String,
        name: String,
        strategy: chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode,
    ) -> Result<MutationOutcome<()>> {
        Ok(into_mutation_outcome(
            self.inner
                .shared
                .select_proxy(group, name, strategy)
                .await?,
        ))
    }

    pub async fn clear_proxy_fixed(
        &self,
        group: String,
        strategy: chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode,
    ) -> Result<MutationOutcome<()>> {
        Ok(into_mutation_outcome(
            self.inner.shared.clear_proxy_fixed(group, strategy).await?,
        ))
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
        let typed = ProxyMutationOutcome::CommittedDegraded {
            value: (),
            commits: vec![chimera_contracts::runtime::CommitReceipt {
                operation_id: Some("proxy-operation".into()),
                domain: "clash".into(),
                source_version: 7,
                runtime: chimera_contracts::runtime::RuntimeCommitStatus::Applied,
            }],
            notifications_pending: false,
            degradations: vec![SharedDegradation {
                phase: SharedPhase::SystemEffect,
                reason: SharedReason::ProxyInterruptionFailed {
                    cause: InterruptFailure::Protocol,
                },
                message: "delete rejected".into(),
                retryable: false,
            }],
        };
        let projected = into_mutation_outcome(typed);
        let json = serde_json::to_value(&projected).unwrap();
        assert_eq!(json["status"], "committed_degraded");
        assert_eq!(json["commits"][0]["operation_id"], "proxy-operation");
        assert_eq!(json["commits"][0]["source_version"], 7);
        assert_eq!(json["notifications_pending"], false);
        assert_eq!(json["degradations"][0]["code"], "proxy_interruption_failed");
        assert_eq!(json["degradations"][0]["phase"], "system_effect");
        assert_eq!(json["degradations"][0]["retryable"], false);
        assert!(json["degradations"][0].get("reason").is_none());
    }
}

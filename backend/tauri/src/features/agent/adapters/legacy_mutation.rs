use crate::{config::chimera::IVerge, feat};

use super::super::{model::AgentRoutingMode, ports::AgentMutationPort};

// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: runtime configuration mutations are still exposed through feat module functions.
// Remove when: ConfigClient is injected through NyanpasuClient.
pub(crate) struct LegacyAgentMutation;

impl LegacyAgentMutation {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl AgentMutationPort for LegacyAgentMutation {
    async fn set_tun_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        feat::set_tun_enabled(enabled).await
    }

    async fn set_system_proxy_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        feat::set_system_proxy_enabled(enabled).await
    }

    async fn persist_system_proxy_desired(&self, enabled: bool) -> anyhow::Result<()> {
        feat::patch_verge(IVerge {
            enable_system_proxy: Some(enabled),
            ..Default::default()
        })
        .await
    }

    async fn set_service_mode(&self, enabled: bool) -> anyhow::Result<()> {
        feat::set_service_mode(enabled).await
    }

    async fn restore_service_mode(&self, enabled: bool) -> anyhow::Result<()> {
        feat::restore_service_mode(enabled).await
    }

    async fn set_routing_mode(&self, mode: AgentRoutingMode) -> anyhow::Result<()> {
        feat::set_routing_mode(to_runtime_routing_mode(mode)).await
    }
}

fn to_runtime_routing_mode(mode: AgentRoutingMode) -> feat::RoutingMode {
    match mode {
        AgentRoutingMode::Rule => feat::RoutingMode::Rule,
        AgentRoutingMode::Global => feat::RoutingMode::Global,
        AgentRoutingMode::Direct => feat::RoutingMode::Direct,
    }
}

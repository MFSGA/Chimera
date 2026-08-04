use crate::config::{chimera::ClashCore, core::Config};

use super::super::{
    model::{AgentProfileSnapshot, AgentRoutingMode, AgentSelectedCore},
    ports::{AgentConfigurationPort, AgentConfigurationSnapshot},
};

// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: generated configuration and profile state are still exposed through Config globals.
// Remove when: ConfigClient is injected through NyanpasuClient.
pub(crate) struct LegacyAgentConfiguration;

impl LegacyAgentConfiguration {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl AgentConfigurationPort for LegacyAgentConfiguration {
    fn snapshot(&self) -> AgentConfigurationSnapshot {
        let verge = Config::verge().latest().clone();
        let clash = Config::clash().latest().clone();
        let runtime = Config::runtime().latest().clone();
        let profiles = Config::profiles().data().clone();
        let expected_mixed_port = verge
            .verge_mixed_port
            .unwrap_or_else(|| clash.get_mixed_port());
        let routing_mode = runtime
            .config
            .as_ref()
            .and_then(|config| config.get("mode"))
            .and_then(serde_yaml::Value::as_str)
            .and_then(AgentRoutingMode::parse);
        let secret_is_weak = clash
            .get_client_info()
            .secret
            .as_deref()
            .map(|secret| secret.trim().is_empty() || secret == "chimera")
            .unwrap_or(true);

        AgentConfigurationSnapshot {
            expected_mixed_port,
            selected_core: map_selected_core(verge.clash_core.unwrap_or_default()),
            runtime_config_present: runtime.config.is_some(),
            routing_mode,
            generated_tun_enabled: generated_tun_enabled(runtime.config.as_ref()),
            secret_is_weak,
            desired_service_mode: verge.enable_service_mode.unwrap_or(false),
            desired_system_proxy: verge.enable_system_proxy.unwrap_or(false),
            desired_tun: verge.enable_tun_mode.unwrap_or(false),
            profiles: summarize_profiles(&profiles),
        }
    }
}

fn map_selected_core(core: ClashCore) -> AgentSelectedCore {
    match core {
        ClashCore::ClashPremium => AgentSelectedCore::Clash,
        ClashCore::ClashRs => AgentSelectedCore::ClashRs,
        ClashCore::Mihomo => AgentSelectedCore::Mihomo,
        ClashCore::ChimeraClient => AgentSelectedCore::ChimeraClient,
        ClashCore::MihomoAlpha => AgentSelectedCore::MihomoAlpha,
        ClashCore::ClashRsAlpha => AgentSelectedCore::ClashRsAlpha,
    }
}

fn generated_tun_enabled(config: Option<&serde_yaml::Mapping>) -> Option<bool> {
    config
        .and_then(|config| config.get("tun"))
        .and_then(serde_yaml::Value::as_mapping)
        .and_then(|tun| tun.get("enable"))
        .and_then(serde_yaml::Value::as_bool)
}

fn summarize_profiles(
    profiles: &crate::config::profile::profiles::Profiles,
) -> AgentProfileSnapshot {
    let remote_count = profiles
        .items
        .iter()
        .filter(|profile| matches!(profile, crate::config::profile::item::Profile::Remote(_)))
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

#[cfg(test)]
mod tests {
    use super::{AgentSelectedCore, ClashCore, map_selected_core};

    #[test]
    fn selected_core_mapping_is_closed_at_the_configuration_boundary() {
        let cases = [
            (ClashCore::ClashPremium, AgentSelectedCore::Clash),
            (ClashCore::ClashRs, AgentSelectedCore::ClashRs),
            (ClashCore::Mihomo, AgentSelectedCore::Mihomo),
            (ClashCore::ChimeraClient, AgentSelectedCore::ChimeraClient),
            (ClashCore::MihomoAlpha, AgentSelectedCore::MihomoAlpha),
            (ClashCore::ClashRsAlpha, AgentSelectedCore::ClashRsAlpha),
        ];

        for (source, expected) in cases {
            assert_eq!(map_selected_core(source), expected);
        }
    }
}

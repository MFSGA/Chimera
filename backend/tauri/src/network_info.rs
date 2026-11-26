//! Desktop entry points for shared direct-egress discovery.
use crate::client::ChimeraClient;
use chimera_config::application::{
    IpLookupProviderInfo, StunServer, StunServerCapabilities, validate_public_ip_probe_sources,
};
use chimera_contracts::net::DirectEgressProbe as _;
#[cfg(test)]
use chimera_contracts::net::EgressSource;
pub use chimera_contracts::net::{DirectEgress, StunServerTest};
use chimera_net::direct_egress::NetworkDirectEgressProbe;

pub fn ip_lookup_providers() -> Vec<IpLookupProviderInfo> {
    chimera_rpc::commands::system::network::get_ip_lookup_providers()
}

pub async fn probe_direct_egress(client: &ChimeraClient) -> anyhow::Result<DirectEgress> {
    client.probe_direct_egress().await
}

/// Explicit desktop user action: test all transports without saving results;
/// never allow an arbitrary STUN target to resolve to a LAN/private IP.
pub async fn test_stun_server(host: String, port: u16) -> anyhow::Result<StunServerTest> {
    let server = StunServer {
        host: host.trim().to_owned(),
        port,
        enabled: true,
        builtin: false,
        capabilities: StunServerCapabilities::default(),
    };
    validate_public_ip_probe_sources(&[server.clone()], &[]).map_err(anyhow::Error::msg)?;
    let test = NetworkDirectEgressProbe::default()
        .test_stun_server(&server.host, server.port)
        .await;
    Ok(test)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::application::IpLookupProvider;
    #[test]
    fn lookup_source_preserves_which_consented_service_answered() {
        assert_eq!(
            EgressSource::Lookup {
                provider: IpLookupProvider::Ipify
            },
            EgressSource::Lookup {
                provider: IpLookupProvider::Ipify
            },
        );
    }
    #[test]
    fn providers_have_both_family_endpoints() {
        for provider in ip_lookup_providers() {
            assert!(provider.ipv4_url.starts_with("https://"));
            assert!(provider.ipv6_url.starts_with("https://"));
        }
    }
}

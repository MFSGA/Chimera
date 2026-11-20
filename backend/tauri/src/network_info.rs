//! Ref-aligned direct-egress probe using Chimera's typed committed settings.
//! STUN and consented HTTP echo bypass configured proxies; capture modes block
//! requests because they could misidentify a proxy exit as local egress.
use crate::client::ChimeraClient;
use chimera_config::{
    application::{
        ChimeraAppConfig, IpLookupProvider, IpLookupProviderInfo, StunServer,
        StunServerCapabilities, validate_public_ip_probe_sources,
    },
    clash::config::{ClashConfig, TransparentProxyMode},
};
use chimera_net::direct_egress::{
    DirectEgressProbe as _, EgressAnswer, EgressSource as NetEgressSource,
    NetworkDirectEgressProbe, ProbePermission, ProbeSources, StunAttempt as NetStunAttempt,
    StunServerTest as NetStunServerTest, StunTransport as NetStunTransport,
};
use serde::Serialize;
use specta::Type;
use std::{net::IpAddr, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectEgress {
    Disabled,
    TunEnabled,
    TransparentProxyEnabled,
    Probed {
        ipv4: Option<EgressAddress>,
        ipv6: Option<EgressAddress>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EgressAddress {
    pub address: String,
    pub country: Option<String>,
    pub source: EgressSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EgressSource {
    Stun {
        host: String,
        port: u16,
        transport: StunTransport,
    },
    Lookup {
        provider: IpLookupProvider,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StunTransport {
    Udp,
    Tcp,
}

impl From<NetEgressSource> for EgressSource {
    fn from(source: NetEgressSource) -> Self {
        match source {
            NetEgressSource::Stun {
                host,
                port,
                transport,
            } => Self::Stun {
                host,
                port,
                transport: match transport {
                    NetStunTransport::Udp => StunTransport::Udp,
                    NetStunTransport::Tcp => StunTransport::Tcp,
                },
            },
            NetEgressSource::Lookup { provider } => Self::Lookup { provider },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StunAttempt {
    Answered { address: String, latency_ms: u64 },
    NoAddress,
    NoAnswer,
}
impl From<NetStunAttempt> for StunAttempt {
    fn from(attempt: NetStunAttempt) -> Self {
        match attempt {
            NetStunAttempt::Answered {
                address,
                latency_ms,
            } => Self::Answered {
                address: address.to_string(),
                latency_ms,
            },
            NetStunAttempt::NoAddress => Self::NoAddress,
            NetStunAttempt::NoAnswer => Self::NoAnswer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct StunServerTest {
    pub udp_ipv4: StunAttempt,
    pub udp_ipv6: StunAttempt,
    pub tcp_ipv4: StunAttempt,
    pub tcp_ipv6: StunAttempt,
    pub capabilities: StunServerCapabilities,
}
impl From<NetStunServerTest> for StunServerTest {
    fn from(test: NetStunServerTest) -> Self {
        Self {
            udp_ipv4: test.udp_ipv4.into(),
            udp_ipv6: test.udp_ipv6.into(),
            tcp_ipv4: test.tcp_ipv4.into(),
            tcp_ipv6: test.tcp_ipv6.into(),
            capabilities: test.capabilities,
        }
    }
}

pub fn ip_lookup_providers() -> Vec<IpLookupProviderInfo> {
    IpLookupProvider::ALL.into_iter().map(Into::into).collect()
}

fn blocked(app: &ChimeraAppConfig, clash: &ClashConfig) -> Option<DirectEgress> {
    if !app.enable_local_ip_probe {
        Some(DirectEgress::Disabled)
    } else if clash.enable_tun_mode {
        Some(DirectEgress::TunEnabled)
    } else if clash.transparent_proxy.local
        && clash.transparent_proxy.mode != TransparentProxyMode::Disabled
    {
        Some(DirectEgress::TransparentProxyEnabled)
    } else {
        None
    }
}

fn current_gate(client: &ChimeraClient) -> anyhow::Result<Option<DirectEgress>> {
    Ok(blocked(
        &client.get_app_config()?,
        &client.get_clash_config()?,
    ))
}

struct LiveProbePermission(ChimeraClient);
impl ProbePermission for LiveProbePermission {
    fn allows_stun(&self, server: &StunServer) -> bool {
        current_gate(&self.0).ok() == Some(None)
            && self
                .0
                .get_app_config()
                .is_ok_and(|app| app.stun_servers.contains(server) && server.enabled)
    }
    fn allows_lookup(&self, provider: IpLookupProvider) -> bool {
        current_gate(&self.0).ok() == Some(None)
            && self
                .0
                .get_app_config()
                .is_ok_and(|app| app.ip_lookup_providers.contains(&provider))
    }
}

fn locate(
    client: &ChimeraClient,
    answer: EgressAnswer<impl Into<IpAddr> + std::fmt::Display>,
) -> EgressAddress {
    let ip = answer.address.into();
    EgressAddress {
        address: ip.to_string(),
        country: client.geo_country(ip),
        source: answer.source.into(),
    }
}

/// Every network destination is checked through the current opt-in + capture
/// settings, and disabled destinations are dropped before attempting network IO.
pub async fn probe_direct_egress(client: &ChimeraClient) -> anyhow::Result<DirectEgress> {
    if let Some(reason) = current_gate(client)? {
        return Ok(reason);
    }
    let snapshot = client.get_app_config()?;
    let sources =
        ProbeSources::from_config(&snapshot, Arc::new(LiveProbePermission(client.clone())));
    let probe = NetworkDirectEgressProbe::default();
    let (ipv4, ipv6) = tokio::join!(probe.ipv4(&sources), probe.ipv6(&sources));
    if let Some(reason) = current_gate(client)? {
        return Ok(reason);
    }
    Ok(DirectEgress::Probed {
        ipv4: ipv4.map(|answer| locate(client, answer)),
        ipv6: ipv6.map(|answer| locate(client, answer)),
    })
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
    Ok(test.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_is_disabled_and_capture_guards_take_priority() {
        let mut app = ChimeraAppConfig::default();
        let mut clash = ClashConfig::default();
        assert_eq!(blocked(&app, &clash), Some(DirectEgress::Disabled));
        app.enable_local_ip_probe = true;
        assert_eq!(blocked(&app, &clash), None);
        clash.enable_tun_mode = true;
        assert_eq!(blocked(&app, &clash), Some(DirectEgress::TunEnabled));
        clash.enable_tun_mode = false;
        clash.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        assert_eq!(
            blocked(&app, &clash),
            Some(DirectEgress::TransparentProxyEnabled)
        );
        clash.transparent_proxy.local = false;
        assert_eq!(blocked(&app, &clash), None);
    }
    #[test]
    fn lookup_source_preserves_which_consented_service_answered() {
        assert_eq!(
            EgressSource::from(NetEgressSource::Lookup {
                provider: IpLookupProvider::Ipify
            }),
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

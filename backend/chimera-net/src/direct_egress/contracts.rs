//! Probe port contracts adapted from the pinned ref for Chimera's existing
//! config layer. Wire DTOs live at the Tauri IPC boundary.
use chimera_config::application::{
    ChimeraAppConfig, IpLookupProvider, StunFamilies, StunServer, StunServerCapabilities,
};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StunTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressAnswer<T> {
    pub address: T,
    pub source: EgressSource,
}

pub trait ProbePermission: Send + Sync {
    fn allows_stun(&self, server: &StunServer) -> bool;
    fn allows_lookup(&self, provider: IpLookupProvider) -> bool;
}

#[derive(Clone)]
pub struct ProbeSources {
    pub stun: Vec<StunServer>,
    pub lookups: Vec<IpLookupProvider>,
    pub permission: Arc<dyn ProbePermission>,
}

impl ProbeSources {
    pub fn from_config(config: &ChimeraAppConfig, permission: Arc<dyn ProbePermission>) -> Self {
        Self {
            stun: config
                .stun_servers
                .iter()
                .filter(|s| s.enabled)
                .cloned()
                .collect(),
            lookups: config.ip_lookup_providers.clone(),
            permission,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StunAttempt {
    Answered { address: IpAddr, latency_ms: u64 },
    NoAddress,
    NoAnswer,
}

impl StunAttempt {
    fn answered(self) -> bool {
        matches!(self, Self::Answered { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StunServerTest {
    pub udp_ipv4: StunAttempt,
    pub udp_ipv6: StunAttempt,
    pub tcp_ipv4: StunAttempt,
    pub tcp_ipv6: StunAttempt,
    pub capabilities: StunServerCapabilities,
}

impl StunServerTest {
    pub fn new(
        udp_ipv4: StunAttempt,
        udp_ipv6: StunAttempt,
        tcp_ipv4: StunAttempt,
        tcp_ipv6: StunAttempt,
    ) -> Self {
        Self {
            udp_ipv4,
            udp_ipv6,
            tcp_ipv4,
            tcp_ipv6,
            capabilities: StunServerCapabilities {
                udp: StunFamilies {
                    ipv4: udp_ipv4.answered(),
                    ipv6: udp_ipv6.answered(),
                },
                tcp: StunFamilies {
                    ipv4: tcp_ipv4.answered(),
                    ipv6: tcp_ipv6.answered(),
                },
            },
        }
    }
}

#[async_trait::async_trait]
pub trait DirectEgressProbe: Send + Sync {
    async fn ipv4(&self, sources: &ProbeSources) -> Option<EgressAnswer<Ipv4Addr>>;
    async fn ipv6(&self, sources: &ProbeSources) -> Option<EgressAnswer<Ipv6Addr>>;
    async fn test_stun_server(&self, host: &str, port: u16) -> StunServerTest;
}

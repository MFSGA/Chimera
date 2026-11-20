//! Public-IP probing with an explicit opt-in and ref's capture-mode guards.
//! The STUN transport lives in chimera-net, not in the Tauri IPC layer.

use chimera_config::{
    application::{ChimeraAppConfig, StunServer},
    clash::config::{ClashConfig, TransparentProxyMode},
};
use chimera_net::direct_egress::{Family, probe_stun};
use serde::Serialize;
use specta::Type;

use crate::client::ChimeraClient;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StunTransport {
    Udp,
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

async fn probe_family(
    client: &ChimeraClient,
    allowed: &[StunServer],
    family: Family,
) -> Option<EgressAddress> {
    for server in allowed {
        // A settings change during a running probe must prevent its NEXT
        // network request, not merely hide the final result.
        if current_gate(client).ok()?.is_some() {
            return None;
        }

        let current = client.get_app_config().ok()?;
        if !current.stun_servers.iter().any(|candidate| {
            candidate.enabled
                && candidate.port == server.port
                && candidate.host.eq_ignore_ascii_case(&server.host)
        }) {
            continue;
        }
        if let Some(address) = probe_stun(&server.host, server.port, family).await {
            return Some(EgressAddress {
                address: address.to_string(),
                country: None,
                source: EgressSource::Stun {
                    host: server.host.clone(),
                    port: server.port,
                    transport: StunTransport::Udp,
                },
            });
        }
    }
    None
}

/// Direct egress has a privacy cost: do not probe unless the user opted in.
/// It is never safe to treat a TUN/transparent-capture response as the direct
/// egress address, because the response may be from a proxy exit.
pub async fn probe_direct_egress(client: &ChimeraClient) -> anyhow::Result<DirectEgress> {
    if let Some(reason) = current_gate(client)? {
        return Ok(reason);
    }
    let allowed: Vec<_> = client
        .get_app_config()?
        .stun_servers
        .into_iter()
        .filter(|server| server.enabled)
        .collect();
    let (ipv4, ipv6) = tokio::join!(
        probe_family(client, &allowed, Family::Ipv4),
        probe_family(client, &allowed, Family::Ipv6),
    );
    // Never report a result if the user's opt-in or capture mode changed.
    if let Some(reason) = current_gate(client)? {
        return Ok(reason);
    }
    Ok(DirectEgress::Probed { ipv4, ipv6 })
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
}

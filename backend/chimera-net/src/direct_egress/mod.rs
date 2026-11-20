//! Ref-aligned STUN direct-egress adapter. The caller must gate on explicit opt-in
//! and on TUN/transparent-capture state *before every outbound attempt*.
mod stun;

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use tokio::{net::UdpSocket, time::Instant};

use self::stun::{TransactionId, binding_request, mapped_address};

const STUN_DEADLINE: Duration = Duration::from_millis(2_500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Ipv4,
    Ipv6,
}

impl Family {
    fn matches(self, address: IpAddr) -> bool {
        matches!(
            (self, address),
            (Self::Ipv4, IpAddr::V4(_)) | (Self::Ipv6, IpAddr::V6(_))
        )
    }

    fn unspecified(self) -> SocketAddr {
        match self {
            Self::Ipv4 => (Ipv4Addr::UNSPECIFIED, 0).into(),
            Self::Ipv6 => (Ipv6Addr::UNSPECIFIED, 0).into(),
        }
    }
}

/// Never let an application-controlled server list reach loopback or private
/// addresses, even after DNS rebinding. This mirrors ref's STUN destination
/// guard and avoids private-network scanning from a dashboard click.
fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast())
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(ip) => is_public(ip.into()),
            None => {
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local())
            }
        },
    }
}

/// A single bounded UDP Binding attempt. This uses OS sockets, not an HTTP
/// proxy. The caller has already verified the user's probe consent and capture
/// mode, and must check both again before reporting the result.
pub async fn probe_stun(host: &str, port: u16, family: Family) -> Option<IpAddr> {
    if host.is_empty() || port == 0 || host.len() > 253 {
        return None;
    }

    let deadline = Instant::now() + STUN_DEADLINE;
    let addresses = tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port)))
        .await
        .ok()?
        .ok()?;
    let remote = addresses
        .filter(|server| family.matches(server.ip()) && is_public(server.ip()))
        .next()?;

    let socket = tokio::time::timeout_at(deadline, UdpSocket::bind(family.unspecified()))
        .await
        .ok()?
        .ok()?;
    tokio::time::timeout_at(deadline, socket.connect(remote))
        .await
        .ok()?
        .ok()?;

    let id = TransactionId::random().ok()?;
    let request = binding_request(id);
    let mut response = [0u8; 1024];
    // Retry with the upstream's bounded initial 500 ms + 1000 ms intervals.
    for interval in [
        Duration::from_millis(500),
        Duration::from_millis(1_000),
        STUN_DEADLINE,
    ] {
        tokio::time::timeout_at(deadline, socket.send(&request))
            .await
            .ok()?
            .ok()?;
        let attempt_deadline = (Instant::now() + interval).min(deadline);
        if let Ok(Ok(size)) =
            tokio::time::timeout_at(attempt_deadline, socket.recv(&mut response)).await
        {
            if let Ok(mapped) = mapped_address(&response[..size], id) {
                let address = mapped.ip();
                if family.matches(address) && is_public(address) {
                    return Some(address);
                }
            }
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_stun_targets_reject_private_and_local_destinations() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "169.254.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(address.parse().unwrap()), "{address}");
        }
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(is_public(address.parse().unwrap()), "{address}");
        }
    }

    #[test]
    fn family_does_not_mislabel_an_address() {
        assert!(Family::Ipv4.matches("1.1.1.1".parse().unwrap()));
        assert!(!Family::Ipv4.matches("2606:4700::1111".parse().unwrap()));
        assert!(Family::Ipv6.matches("2606:4700::1111".parse().unwrap()));
    }
}

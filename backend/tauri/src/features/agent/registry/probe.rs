use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use url::{Host, Url};

use super::{AgentToolError, AgentToolErrorCode};
use crate::features::agent::AgentNetworkProbeRequest;

const NETWORK_PROBE_DEFAULT_TIMEOUT_MS: u32 = 5_000;
const NETWORK_PROBE_MIN_TIMEOUT_MS: u32 = 1_000;
pub(super) const NETWORK_PROBE_REQUEST_TIMEOUT_MS: u32 = 10_000;
pub(super) const MAX_NETWORK_PROBE_URL_BYTES: usize = 2_048;
pub(crate) const DNS_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const MAX_RESOLVED_ADDRESSES: usize = 16;

pub(crate) struct ProbeTarget {
    pub(crate) url: Url,
    pub(crate) expected_status: Option<u16>,
    pub(crate) timeout: Duration,
}

pub(crate) fn validate_probe_request(
    request: AgentNetworkProbeRequest,
) -> Result<ProbeTarget, AgentToolError> {
    if request.url.len() > MAX_NETWORK_PROBE_URL_BYTES {
        return Err(AgentToolError::new(
            AgentToolErrorCode::InvalidRequest,
            "network probe URL is too long",
        ));
    }
    let url = Url::parse(&request.url).map_err(|_| invalid_target())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid_target());
    }

    if let Some(status) = request.expected_status
        && !(100..=599).contains(&status)
    {
        return Err(AgentToolError::new(
            AgentToolErrorCode::InvalidRequest,
            "expected_status must be between 100 and 599",
        ));
    }

    let timeout_ms = request
        .timeout_ms
        .unwrap_or(NETWORK_PROBE_DEFAULT_TIMEOUT_MS);
    if !(NETWORK_PROBE_MIN_TIMEOUT_MS..=NETWORK_PROBE_REQUEST_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(AgentToolError::new(
            AgentToolErrorCode::InvalidRequest,
            "timeout_ms is outside the allowed range",
        ));
    }

    match url.host().ok_or_else(invalid_target)? {
        Host::Domain(domain) if is_blocked_hostname(domain) => return Err(blocked_target()),
        Host::Ipv4(address) if is_blocked_ip(IpAddr::V4(address)) => {
            return Err(blocked_target());
        }
        Host::Ipv6(address) if is_blocked_ip(IpAddr::V6(address)) => {
            return Err(blocked_target());
        }
        _ => {}
    }

    Ok(ProbeTarget {
        url,
        expected_status: request.expected_status,
        timeout: Duration::from_millis(u64::from(timeout_ms)),
    })
}

pub(crate) fn collect_safe_addresses(
    resolved: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, AgentToolError> {
    let mut addresses = Vec::with_capacity(MAX_RESOLVED_ADDRESSES);
    for address in resolved {
        if is_blocked_ip(address.ip()) {
            return Err(blocked_target());
        }
        if !addresses.contains(&address) {
            addresses.push(address);
            if addresses.len() == MAX_RESOLVED_ADDRESSES {
                break;
            }
        }
    }
    if addresses.is_empty() {
        return Err(resolution_failed());
    }
    Ok(addresses)
}

pub(super) fn is_blocked_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host == "metadata.amazonaws.com"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".arpa")
        || host.ends_with(".test")
        || host.ends_with(".invalid")
        || host.ends_with(".example")
        || host.ends_with(".onion")
}

pub(super) fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(address) => is_blocked_ipv4(address),
        IpAddr::V6(address) => is_blocked_ipv6(address),
    }
}

fn is_blocked_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (18..=19).contains(&b))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
}

fn is_blocked_ipv6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_blocked_ipv4(mapped);
    }
    let segments = address.segments();
    let is_current_global_unicast = (0x2000..=0x3fff).contains(&segments[0]);
    address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || !is_current_global_unicast
        || (segments[0] == 0x2001 && segments[1] <= 0x01ff)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || segments[0] == 0x2002
        || (segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
}

pub(crate) fn invalid_target() -> AgentToolError {
    AgentToolError::new(
        AgentToolErrorCode::InvalidTarget,
        "only public HTTP and HTTPS targets are allowed",
    )
}

fn blocked_target() -> AgentToolError {
    AgentToolError::new(
        AgentToolErrorCode::TargetBlocked,
        "local, private, reserved, and special-use targets are blocked",
    )
}

pub(crate) fn probe_timed_out() -> AgentToolError {
    AgentToolError::new(AgentToolErrorCode::TimedOut, "network probe timed out")
}

pub(crate) fn resolution_failed() -> AgentToolError {
    AgentToolError::new(
        AgentToolErrorCode::ResolutionFailed,
        "target hostname could not be safely resolved",
    )
}

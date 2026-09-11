use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};

use reqwest::redirect::Policy;
use tokio::{net::lookup_host, time::timeout};
use url::{Host, Url};

use super::{AgentNetworkProbeRequest, AgentNetworkProbeResult, AgentToolError};

const NETWORK_PROBE_DEFAULT_TIMEOUT_MS: u32 = 5_000;
const NETWORK_PROBE_MIN_TIMEOUT_MS: u32 = 1_000;
const NETWORK_PROBE_MAX_TIMEOUT_MS: u32 = 10_000;
const MAX_NETWORK_PROBE_URL_BYTES: usize = 2_048;
const DNS_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESOLVED_ADDRESSES: usize = 16;

struct ProbeTarget {
    url: Url,
    expected_status: Option<u16>,
    timeout: Duration,
}

pub(crate) async fn execute_network_probe(
    request: AgentNetworkProbeRequest,
) -> Result<AgentNetworkProbeResult, AgentToolError> {
    let target = validate_probe_request(request)?;
    let deadline = Instant::now() + target.timeout;
    let (domain, addresses) = resolve_probe_target(&target.url, target.timeout).await?;
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(AgentToolError::TimedOut)?;

    let mut client = reqwest::Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .connect_timeout(remaining)
        .timeout(remaining)
        .pool_max_idle_per_host(0)
        .user_agent("Chimera-Agent/1");

    if let Some(domain) = domain.as_deref() {
        client = client.resolve_to_addrs(domain, &addresses);
    }

    let client = client
        .build()
        .map_err(|_| AgentToolError::ExecutionFailed)?;
    let started = Instant::now();
    let response = client.get(target.url).send().await.map_err(|error| {
        if error.is_timeout() {
            AgentToolError::TimedOut
        } else {
            AgentToolError::ExecutionFailed
        }
    })?;
    let status = response.status().as_u16();
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    Ok(AgentNetworkProbeResult {
        status,
        expected_status: target.expected_status,
        matches_expected_status: target.expected_status.map(|expected| expected == status),
        latency_ms,
    })
}

fn validate_probe_request(
    request: AgentNetworkProbeRequest,
) -> Result<ProbeTarget, AgentToolError> {
    if request.url.len() > MAX_NETWORK_PROBE_URL_BYTES {
        return Err(AgentToolError::InvalidRequest);
    }
    let url = Url::parse(&request.url).map_err(|_| AgentToolError::InvalidTarget)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AgentToolError::InvalidTarget);
    }

    if let Some(status) = request.expected_status
        && !(100..=599).contains(&status)
    {
        return Err(AgentToolError::InvalidRequest);
    }

    let timeout_ms = request
        .timeout_ms
        .unwrap_or(NETWORK_PROBE_DEFAULT_TIMEOUT_MS);
    if !(NETWORK_PROBE_MIN_TIMEOUT_MS..=NETWORK_PROBE_MAX_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(AgentToolError::InvalidRequest);
    }

    match url.host().ok_or(AgentToolError::InvalidTarget)? {
        Host::Domain(domain) if is_blocked_hostname(domain) => {
            return Err(AgentToolError::TargetBlocked);
        }
        Host::Ipv4(address) if is_blocked_ip(IpAddr::V4(address)) => {
            return Err(AgentToolError::TargetBlocked);
        }
        Host::Ipv6(address) if is_blocked_ip(IpAddr::V6(address)) => {
            return Err(AgentToolError::TargetBlocked);
        }
        _ => {}
    }

    Ok(ProbeTarget {
        url,
        expected_status: request.expected_status,
        timeout: Duration::from_millis(u64::from(timeout_ms)),
    })
}

async fn resolve_probe_target(
    url: &Url,
    timeout_budget: Duration,
) -> Result<(Option<String>, Vec<SocketAddr>), AgentToolError> {
    let port = url
        .port_or_known_default()
        .ok_or(AgentToolError::InvalidTarget)?;
    match url.host().ok_or(AgentToolError::InvalidTarget)? {
        Host::Ipv4(address) => Ok((None, vec![SocketAddr::new(IpAddr::V4(address), port)])),
        Host::Ipv6(address) => Ok((None, vec![SocketAddr::new(IpAddr::V6(address), port)])),
        Host::Domain(domain) => {
            let resolved = timeout(
                DNS_RESOLUTION_TIMEOUT.min(timeout_budget),
                lookup_host((domain, port)),
            )
            .await
            .map_err(|_| AgentToolError::ResolutionFailed)?
            .map_err(|_| AgentToolError::ResolutionFailed)?;
            let addresses = collect_safe_addresses(resolved)?;
            Ok((Some(domain.to_owned()), addresses))
        }
    }
}

fn collect_safe_addresses(
    resolved: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, AgentToolError> {
    let mut addresses = Vec::with_capacity(MAX_RESOLVED_ADDRESSES);
    for address in resolved {
        if is_blocked_ip(address.ip()) {
            return Err(AgentToolError::TargetBlocked);
        }
        if !addresses.contains(&address) {
            addresses.push(address);
            if addresses.len() == MAX_RESOLVED_ADDRESSES {
                break;
            }
        }
    }
    if addresses.is_empty() {
        return Err(AgentToolError::ResolutionFailed);
    }
    Ok(addresses)
}

fn is_blocked_hostname(host: &str) -> bool {
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

fn is_blocked_ip(ip: IpAddr) -> bool {
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use super::{
        collect_safe_addresses, is_blocked_hostname, is_blocked_ip, validate_probe_request,
    };
    use crate::features::agent::{AgentNetworkProbeRequest, AgentToolError};

    #[test]
    fn blocks_local_private_and_special_use_targets() {
        for host in [
            "localhost",
            "metadata.amazonaws.com",
            "service.local",
            "service.internal",
            "example.test",
        ] {
            assert!(is_blocked_hostname(host), "expected {host} to be blocked");
        }

        for ip in [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6("fd00::1".parse().unwrap()),
        ] {
            assert!(is_blocked_ip(ip), "expected {ip} to be blocked");
        }
    }

    #[test]
    fn accepts_public_http_targets_with_bounded_timeout() {
        let target = validate_probe_request(AgentNetworkProbeRequest {
            url: "https://1.1.1.1/".to_owned(),
            expected_status: Some(200),
            timeout_ms: Some(1_500),
        })
        .expect("public target should be accepted");

        assert_eq!(target.expected_status, Some(200));
        assert_eq!(target.timeout.as_millis(), 1_500);
    }

    #[test]
    fn rejects_credentials_invalid_status_and_out_of_range_timeout() {
        for request in [
            AgentNetworkProbeRequest {
                url: "https://user:pass@example.com/".to_owned(),
                expected_status: None,
                timeout_ms: None,
            },
            AgentNetworkProbeRequest {
                url: "https://example.com/".to_owned(),
                expected_status: Some(99),
                timeout_ms: None,
            },
            AgentNetworkProbeRequest {
                url: "https://example.com/".to_owned(),
                expected_status: None,
                timeout_ms: Some(100),
            },
        ] {
            assert!(validate_probe_request(request).is_err());
        }
    }

    #[test]
    fn rejects_resolution_if_any_address_is_not_public() {
        let result = collect_safe_addresses([
            SocketAddr::from(([1, 1, 1, 1], 443)),
            SocketAddr::from(([127, 0, 0, 1], 443)),
        ]);
        assert!(matches!(result, Err(AgentToolError::TargetBlocked)));
    }
}

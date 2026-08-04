use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use reqwest::redirect::Policy;
use tokio::{net::lookup_host, time::timeout};
use url::{Host, Url};

use super::super::{
    model::{AgentNetworkProbeRequest, AgentNetworkProbeResult},
    ports::NetworkProbePort,
    registry::{
        AgentToolError, AgentToolErrorCode,
        probe::{
            DNS_RESOLUTION_TIMEOUT, collect_safe_addresses, invalid_target, probe_timed_out,
            resolution_failed, validate_probe_request,
        },
    },
};

pub(crate) struct HttpNetworkProbe;

impl HttpNetworkProbe {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl NetworkProbePort for HttpNetworkProbe {
    async fn execute(
        &self,
        request: AgentNetworkProbeRequest,
    ) -> Result<AgentNetworkProbeResult, AgentToolError> {
        let target = validate_probe_request(request)?;
        let deadline = Instant::now() + target.timeout;
        let (domain, addresses) = resolve_probe_target(&target.url, target.timeout).await?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(probe_timed_out)?;
        let mut client = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(remaining)
            .timeout(remaining)
            .pool_max_idle_per_host(0)
            .user_agent("Chimera-Agent-Bridge/1");

        if let Some(domain) = domain.as_deref() {
            client = client.resolve_to_addrs(domain, &addresses);
        }

        let client = client.build().map_err(|_| {
            AgentToolError::new(
                AgentToolErrorCode::ExecutionFailed,
                "failed to prepare network probe",
            )
        })?;
        let started = Instant::now();
        let response = client.get(target.url).send().await.map_err(|error| {
            if error.is_timeout() {
                probe_timed_out()
            } else {
                AgentToolError::new(AgentToolErrorCode::ExecutionFailed, "network probe failed")
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
}

async fn resolve_probe_target(
    url: &Url,
    timeout_budget: Duration,
) -> Result<(Option<String>, Vec<SocketAddr>), AgentToolError> {
    let port = url.port_or_known_default().ok_or_else(invalid_target)?;
    match url.host().ok_or_else(invalid_target)? {
        Host::Ipv4(address) => Ok((None, vec![SocketAddr::new(IpAddr::V4(address), port)])),
        Host::Ipv6(address) => Ok((None, vec![SocketAddr::new(IpAddr::V6(address), port)])),
        Host::Domain(domain) => {
            let resolved = timeout(
                DNS_RESOLUTION_TIMEOUT.min(timeout_budget),
                lookup_host((domain, port)),
            )
            .await
            .map_err(|_| resolution_failed())?
            .map_err(|_| resolution_failed())?;
            let addresses = collect_safe_addresses(resolved)?;
            Ok((Some(domain.to_owned()), addresses))
        }
    }
}

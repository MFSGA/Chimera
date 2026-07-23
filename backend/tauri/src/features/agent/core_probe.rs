use std::{net::SocketAddr, time::Duration};

use serde::Deserialize;

use crate::config::core::Config;

use super::model::AgentRoutingMode;

const CORE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct CoreConfigResponse {
    mode: Option<String>,
}

/// Reads only the routing mode from the loopback controller.
/// The controller secret never leaves this module or enters an error message.
pub(super) async fn observed_routing_mode() -> Result<AgentRoutingMode, ()> {
    let info = Config::clash().latest().get_client_info();
    let url = loopback_controller_url(&info.server)?;
    let client = reqwest::ClientBuilder::new()
        .no_proxy()
        .timeout(CORE_PROBE_TIMEOUT)
        .build()
        .map_err(|_| ())?;
    let mut request = client.get(url);
    if let Some(secret) = info.secret.filter(|secret| !secret.is_empty()) {
        request = request.bearer_auth(secret);
    }
    let response = request.send().await.map_err(|_| ())?;
    let config = response
        .error_for_status()
        .map_err(|_| ())?
        .json::<CoreConfigResponse>()
        .await
        .map_err(|_| ())?;
    config
        .mode
        .as_deref()
        .and_then(AgentRoutingMode::parse)
        .ok_or(())
}

/// Prevents the controller credential from ever being sent outside this host.
fn loopback_controller_url(server: &str) -> Result<url::Url, ()> {
    let address = server.parse::<SocketAddr>().map_err(|_| ())?;
    if !address.ip().is_loopback() {
        return Err(());
    }
    url::Url::parse(&format!("http://{server}/configs")).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::loopback_controller_url;

    #[test]
    fn controller_probe_accepts_only_loopback_socket_addresses() {
        assert!(loopback_controller_url("127.0.0.1:9090").is_ok());
        assert!(loopback_controller_url("[::1]:9090").is_ok());
        assert!(loopback_controller_url("192.168.1.2:9090").is_err());
        assert!(loopback_controller_url("example.com:9090").is_err());
    }
}

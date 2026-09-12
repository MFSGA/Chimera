use std::{net::SocketAddr, time::Duration};

use serde::Deserialize;

use crate::core::clash::core::CoreManager;

use super::model::AgentRoutingMode;

const CORE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct CoreTunResponse {
    enable: Option<bool>,
    device: Option<String>,
}

#[derive(Deserialize)]
struct CoreConfigResponse {
    mode: Option<String>,
    tun: Option<CoreTunResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedCoreConfig {
    pub routing_mode: Option<AgentRoutingMode>,
    pub tun_enabled: Option<bool>,
    pub tun_device: Option<String>,
}

/// Reads the small runtime projection Agent needs from the loopback controller.
/// The controller secret never leaves this module or enters an error message.
pub(super) async fn observed_core_config() -> Result<ObservedCoreConfig, ()> {
    let info = CoreManager::global().effective_clash_info();
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
    Ok(project_core_config(config))
}

pub(super) async fn observed_routing_mode() -> Result<AgentRoutingMode, ()> {
    observed_core_config().await?.routing_mode.ok_or(())
}

pub(super) async fn observed_tun_enabled() -> Result<bool, ()> {
    observed_core_config().await?.tun_enabled.ok_or(())
}

fn project_core_config(config: CoreConfigResponse) -> ObservedCoreConfig {
    let (tun_enabled, tun_device) = config
        .tun
        .map(|tun| {
            (
                tun.enable,
                tun.device.filter(|device| !device.trim().is_empty()),
            )
        })
        .unwrap_or((None, None));
    ObservedCoreConfig {
        routing_mode: config.mode.as_deref().and_then(AgentRoutingMode::parse),
        tun_enabled,
        tun_device,
    }
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
    use super::{
        CoreConfigResponse, CoreTunResponse, loopback_controller_url, project_core_config,
    };
    use crate::features::agent::model::AgentRoutingMode;

    #[test]
    fn controller_probe_accepts_only_loopback_socket_addresses() {
        assert!(loopback_controller_url("127.0.0.1:9090").is_ok());
        assert!(loopback_controller_url("[::1]:9090").is_ok());
        assert!(loopback_controller_url("192.168.1.2:9090").is_err());
        assert!(loopback_controller_url("example.com:9090").is_err());
    }

    #[test]
    fn core_projection_keeps_tun_and_routing_observation_together() {
        let observed = project_core_config(CoreConfigResponse {
            mode: Some("rule".into()),
            tun: Some(CoreTunResponse {
                enable: Some(true),
                device: Some("Meta".into()),
            }),
        });

        assert_eq!(observed.routing_mode, Some(AgentRoutingMode::Rule));
        assert_eq!(observed.tun_enabled, Some(true));
        assert_eq!(observed.tun_device.as_deref(), Some("Meta"));
    }

    #[test]
    fn core_projection_preserves_unknown_tun_state() {
        let observed = project_core_config(CoreConfigResponse {
            mode: Some("invalid".into()),
            tun: None,
        });

        assert_eq!(observed.routing_mode, None);
        assert_eq!(observed.tun_enabled, None);
        assert_eq!(observed.tun_device, None);
    }
}

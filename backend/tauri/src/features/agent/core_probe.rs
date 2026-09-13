use std::{net::SocketAddr, time::Duration};

use crate::core::clash::{
    api::{self, ClashRuntimeConfig},
    core::CoreManager,
};

use super::model::AgentRoutingMode;

const CORE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedCoreConfig {
    pub routing_mode: Option<AgentRoutingMode>,
    pub tun_enabled: Option<bool>,
    pub tun_device: Option<String>,
    pub tun_auto_route: Option<bool>,
    pub tun_route_addresses: Vec<String>,
}

/// Reads the small runtime projection Agent needs through the shared Clash API.
/// The loopback guard keeps the controller credential on-host even if the
/// effective controller endpoint is unexpectedly changed.
pub(super) async fn observed_core_config() -> Result<ObservedCoreConfig, ()> {
    let info = CoreManager::global().effective_clash_info();
    loopback_controller_url(&info.server)?;
    let config = tokio::time::timeout(CORE_PROBE_TIMEOUT, api::get_configs())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    Ok(project_core_config(config))
}

pub(super) async fn observed_routing_mode() -> Result<AgentRoutingMode, ()> {
    observed_core_config().await?.routing_mode.ok_or(())
}

fn project_core_config(config: ClashRuntimeConfig) -> ObservedCoreConfig {
    let (tun_enabled, tun_device, tun_auto_route, tun_route_addresses) = config
        .tun
        .map(|tun| {
            let mut routes = tun.route_address;
            routes.extend(tun.inet4_route_address);
            routes.extend(tun.inet6_route_address);
            routes.sort_unstable();
            routes.dedup();
            (
                Some(tun.enable),
                (!tun.device.trim().is_empty()).then_some(tun.device),
                Some(tun.auto_route),
                routes,
            )
        })
        .unwrap_or((None, None, None, Vec::new()));
    ObservedCoreConfig {
        routing_mode: config.mode.as_deref().and_then(AgentRoutingMode::parse),
        tun_enabled,
        tun_device,
        tun_auto_route,
        tun_route_addresses,
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
    use super::{loopback_controller_url, project_core_config};
    use crate::{
        core::clash::api::{ClashRuntimeConfig, RuntimeTun},
        features::agent::model::AgentRoutingMode,
    };

    #[test]
    fn controller_probe_accepts_only_loopback_socket_addresses() {
        assert!(loopback_controller_url("127.0.0.1:9090").is_ok());
        assert!(loopback_controller_url("[::1]:9090").is_ok());
        assert!(loopback_controller_url("192.168.1.2:9090").is_err());
        assert!(loopback_controller_url("example.com:9090").is_err());
    }

    #[test]
    fn core_projection_keeps_tun_and_routing_observation_together() {
        let observed = project_core_config(ClashRuntimeConfig {
            mode: Some("rule".into()),
            tun: Some(RuntimeTun {
                enable: true,
                device: "Meta".into(),
                auto_route: true,
                route_address: vec!["0.0.0.0/1".into()],
                inet4_route_address: vec!["128.0.0.0/1".into()],
                inet6_route_address: vec!["::/1".into(), "0.0.0.0/1".into()],
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(observed.routing_mode, Some(AgentRoutingMode::Rule));
        assert_eq!(observed.tun_enabled, Some(true));
        assert_eq!(observed.tun_device.as_deref(), Some("Meta"));
        assert_eq!(observed.tun_auto_route, Some(true));
        assert_eq!(
            observed.tun_route_addresses,
            vec!["0.0.0.0/1", "128.0.0.0/1", "::/1"]
        );
    }

    #[test]
    fn core_projection_preserves_unknown_tun_state() {
        let observed = project_core_config(ClashRuntimeConfig {
            mode: Some("invalid".into()),
            ..Default::default()
        });

        assert_eq!(observed.routing_mode, None);
        assert_eq!(observed.tun_enabled, None);
        assert_eq!(observed.tun_device, None);
        assert_eq!(observed.tun_auto_route, None);
        assert!(observed.tun_route_addresses.is_empty());
    }
}

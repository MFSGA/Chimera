//! Session-scoped network port resolution helpers.
//!
//! Chimera still stores port strategies in the legacy config model, so this
//! module centralizes the existing mixed-port and external-controller
//! selection behavior behind the same client boundary used by REF.

use std::{net::TcpListener, sync::Mutex};

use anyhow::{Context as _, Result, anyhow, bail};
use chimera_config::{
    clash::config::{
        ClashConfig,
        clash_strategy::{ExternalControllerStrategy, PortStrategy, PortStrategyKind},
    },
    runtime::executor::ResolvedPortBindings,
};

use crate::{
    client::ChimeraClient,
    config::{
        chimera::{ExternalControllerPortStrategy, IVerge},
        core::Config,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortsFingerprint {
    mixed: PortStrategy,
    socks: Option<PortStrategy>,
    http: Option<PortStrategy>,
    external: ExternalControllerStrategy,
}

impl PortsFingerprint {
    fn of(clash: &ClashConfig) -> Self {
        Self {
            mixed: clash.mixed_port.clone(),
            socks: clash.socks_port.clone(),
            http: clash.http_port.clone(),
            external: clash.external_controller.clone(),
        }
    }
}

/// Resolves port strategies once per session and keeps unchanged picks stable.
///
/// The executor receives concrete bindings and never performs port probing. A
/// later config change only re-probes the strategy fields that changed, so a
/// running core's existing listeners are not mistaken for collisions.
#[derive(Debug, Default)]
pub(crate) struct SessionPortResolver {
    cached: Mutex<Option<(PortsFingerprint, ResolvedPortBindings)>>,
}

impl SessionPortResolver {
    pub(crate) fn resolve(&self, clash: &ClashConfig) -> Result<ResolvedPortBindings> {
        let fingerprint = PortsFingerprint::of(clash);
        let mut cached = self
            .cached
            .lock()
            .expect("port resolver cache should not poison");
        if let Some((previous, ports)) = cached.as_ref()
            && *previous == fingerprint
        {
            return Ok(ports.clone());
        }

        let previous = cached.clone();
        let unchanged = |same: bool| -> Option<&ResolvedPortBindings> {
            match previous.as_ref() {
                Some((_, ports)) if same => Some(ports),
                _ => None,
            }
        };

        let mixed_port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.mixed == fingerprint.mixed),
        ) {
            Some(ports) => ports.mixed_port,
            None => *clash
                .mixed_port
                .pick_and_try_port()
                .context("failed to resolve mixed port")?,
        };
        let port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.http == fingerprint.http),
        ) {
            Some(ports) => ports.port,
            None => clash
                .http_port
                .as_ref()
                .map(|strategy| strategy.pick_and_try_port())
                .transpose()
                .context("failed to resolve http port")?
                .map(|picked| *picked),
        };
        let socks_port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.socks == fingerprint.socks),
        ) {
            Some(ports) => ports.socks_port,
            None => clash
                .socks_port
                .as_ref()
                .map(|strategy| strategy.pick_and_try_port())
                .transpose()
                .context("failed to resolve socks port")?
                .map(|picked| *picked),
        };

        // A host-only external-controller change keeps the picked port. Only
        // a changed port strategy is allowed to probe a new listener.
        let external_port = match unchanged(
            previous
                .as_ref()
                .is_some_and(|(prev, _)| prev.external.port == fingerprint.external.port),
        ) {
            Some(ports) => ports
                .external_controller
                .as_deref()
                .and_then(|addr| addr.rsplit(':').next())
                .and_then(|raw| raw.parse::<u16>().ok()),
            None => None,
        };
        let external_port = match external_port {
            Some(port) => port,
            None => *clash
                .external_controller
                .port
                .pick_and_try_port()
                .context("failed to resolve external-controller port")?,
        };
        let external_controller = Some(format!(
            "{}:{}",
            clash.external_controller.host, external_port
        ));

        let ports = ResolvedPortBindings {
            mixed_port,
            port,
            socks_port,
            external_controller,
        };
        *cached = Some((fingerprint, ports.clone()));
        Ok(ports)
    }

    pub(crate) fn cached_ports(&self) -> Option<ResolvedPortBindings> {
        self.cached
            .lock()
            .expect("port resolver cache should not poison")
            .as_ref()
            .map(|(_, ports)| ports.clone())
    }
}

fn find_unused_port(fallback_port: u16) -> Result<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => Ok(listener.local_addr()?.port()),
        Err(_) => {
            log::warn!(target: "app", "use default mixed port: {fallback_port}");
            Ok(fallback_port)
        }
    }
}

pub(crate) fn resolve_random_mixed_port(client: &ChimeraClient) -> Result<()> {
    let clash = client.get_clash_config()?;
    if clash.mixed_port.kind != PortStrategyKind::Random {
        return Ok(());
    }

    let fallback_port = clash.mixed_port.start_port;
    let port = find_unused_port(fallback_port).unwrap_or(fallback_port);

    Config::verge().data().patch_config(IVerge {
        verge_mixed_port: Some(port),
        ..IVerge::default()
    });
    let _ = Config::verge().data().save_file();

    let mut mapping = serde_yaml::Mapping::new();
    mapping.insert("mixed-port".into(), port.into());
    Config::clash().data().patch_config(mapping);
    let _ = Config::clash().latest().prepare_external_controller_port();
    let _ = Config::clash().data().save_config();
    Ok(())
}

pub(crate) fn get_clash_external_port(
    strategy: &ExternalControllerPortStrategy,
    port: u16,
) -> Result<u16> {
    match strategy {
        ExternalControllerPortStrategy::Fixed => {
            if !port_scanner::local_port_available(port) {
                bail!("Port {} is not available", port);
            }
        }
        ExternalControllerPortStrategy::Random | ExternalControllerPortStrategy::AllowFallback => {
            if ExternalControllerPortStrategy::AllowFallback == *strategy
                && port_scanner::local_port_available(port)
            {
                return Ok(port);
            }
            let new_port = port_scanner::request_open_port()
                .ok_or_else(|| anyhow!("Can't find an open port"))?;
            return Ok(new_port);
        }
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(port: u16) -> PortStrategy {
        PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: port,
        }
    }

    #[test]
    fn resolves_fixed_strategies_and_formats_external_controller() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(48231);
        clash.socks_port = Some(fixed(48232));
        clash.http_port = None;
        clash.external_controller.port = fixed(48233);

        let ports = resolver.resolve(&clash).unwrap();
        assert_eq!(ports.mixed_port, 48231);
        assert_eq!(ports.socks_port, Some(48232));
        assert_eq!(ports.port, None);
        assert_eq!(
            ports.external_controller.as_deref(),
            Some("127.0.0.1:48233")
        );
        assert_eq!(resolver.cached_ports(), Some(ports));
    }

    #[test]
    fn random_pick_is_sticky_until_fingerprint_changes() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 0,
        };

        let first = resolver.resolve(&clash).unwrap();
        let second = resolver.resolve(&clash).unwrap();
        assert_eq!(
            first, second,
            "same fingerprint must reuse the session pick"
        );

        clash.socks_port = Some(fixed(48234));
        let third = resolver.resolve(&clash).unwrap();
        assert_eq!(third.socks_port, Some(48234));
        assert_eq!(
            third.mixed_port, first.mixed_port,
            "unchanged mixed strategy must keep the session pick"
        );
    }

    #[test]
    fn unchanged_fields_are_not_reprobed_while_core_occupies_them() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let mixed = probe.local_addr().unwrap().port();
        drop(probe);
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let socks = probe.local_addr().unwrap().port();
        drop(probe);

        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(mixed);
        let first = resolver.resolve(&clash).unwrap();
        assert_eq!(first.mixed_port, mixed);

        let _core = TcpListener::bind(("127.0.0.1", mixed)).unwrap();
        clash.socks_port = Some(fixed(socks));
        let second = resolver
            .resolve(&clash)
            .expect("unchanged mixed port must not be re-probed");
        assert_eq!(second.mixed_port, mixed);
        assert_eq!(second.socks_port, Some(socks));
    }

    #[test]
    fn external_host_change_keeps_port_pick() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let external = probe.local_addr().unwrap().port();
        drop(probe);

        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.external_controller.port = fixed(external);
        let first = resolver.resolve(&clash).unwrap();
        assert_eq!(
            first.external_controller.as_deref(),
            Some(format!("127.0.0.1:{external}").as_str())
        );

        let _core = TcpListener::bind(("127.0.0.1", external)).unwrap();
        clash.external_controller.host = "0.0.0.0".parse().unwrap();
        let second = resolver
            .resolve(&clash)
            .expect("host-only change must not re-probe the external port");
        assert_eq!(
            second.external_controller.as_deref(),
            Some(format!("0.0.0.0:{external}").as_str())
        );
    }

    #[test]
    fn allow_fallback_moves_off_an_occupied_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = listener.local_addr().unwrap().port();
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy::new_allow_fallback(taken);

        let ports = resolver.resolve(&clash).unwrap();
        assert_ne!(ports.mixed_port, taken);
    }
}

//! Session-scoped network port resolution helpers.
//!
//! Port strategies still come from Chimera's config model. Candidate picks are
//! separate from bindings confirmed by the running core, matching REF's v2
//! §6.2 ownership boundary.

use std::{net::TcpListener, sync::Arc};

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
pub struct PortsFingerprint {
    mixed: PortStrategy,
    socks: Option<PortStrategy>,
    http: Option<PortStrategy>,
    external: ExternalControllerStrategy,
}

impl PortsFingerprint {
    pub fn of(clash: &ClashConfig) -> Self {
        Self {
            mixed: clash.mixed_port.clone(),
            socks: clash.socks_port.clone(),
            http: clash.http_port.clone(),
            external: clash.external_controller.clone(),
        }
    }
}

/// One candidate resolution. It is inert until the core confirms its apply.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidatePortBindings {
    fingerprint: PortsFingerprint,
    bindings: ResolvedPortBindings,
}

impl CandidatePortBindings {
    pub fn bindings(&self) -> &ResolvedPortBindings {
        &self.bindings
    }
}

impl Default for CandidatePortBindings {
    fn default() -> Self {
        let config = ClashConfig::default();
        Self {
            fingerprint: PortsFingerprint::of(&config),
            bindings: ResolvedPortBindings::default(),
        }
    }
}

/// Resolves port strategies from the last confirmed apply and keeps unchanged
/// picks stable. Candidate picks do not change that baseline.
///
/// The executor receives concrete bindings and never performs port probing. A
/// later config change only re-probes the strategy fields that changed, so a
/// running core's existing listeners are not mistaken for collisions.
#[derive(Debug)]
pub(crate) struct SessionPortResolver {
    lifecycle: Arc<super::runtime::RuntimeLifecycle>,
}

impl SessionPortResolver {
    pub(crate) fn new(lifecycle: Arc<super::runtime::RuntimeLifecycle>) -> Self {
        Self { lifecycle }
    }

    pub(crate) fn resolve_candidate(&self, clash: &ClashConfig) -> Result<CandidatePortBindings> {
        let fingerprint = PortsFingerprint::of(clash);
        let previous = self.lifecycle.accepted_confirmed_receipt().map(|receipt| {
            (
                receipt.ports.fingerprint.clone(),
                receipt.ports.bindings.clone(),
            )
        });
        if let Some((previous, ports)) = previous.as_ref()
            && *previous == fingerprint
        {
            return Ok(CandidatePortBindings {
                fingerprint,
                bindings: ports.clone(),
            });
        }

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
            None => *clash.mixed_port.pick_and_try_port().with_context(|| {
                format!(
                    "failed to resolve mixed port (strategy: {:?}, start port: {})",
                    clash.mixed_port.kind, clash.mixed_port.start_port
                )
            })?,
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
        Ok(CandidatePortBindings {
            fingerprint,
            bindings: ports,
        })
    }

    #[cfg(test)]
    pub(crate) fn confirm(&self, candidate: &CandidatePortBindings) {
        self.lifecycle
            .publish_confirmed(Arc::new(test_runtime_receipt(candidate.clone())));
    }

    #[cfg(test)]
    pub(crate) fn confirmed(&self) -> Option<ResolvedPortBindings> {
        self.lifecycle
            .accepted_confirmed_receipt()
            .map(|receipt| receipt.ports.bindings.clone())
    }

    pub(crate) fn invalidate(&self) {
        self.lifecycle.invalidate_confirmed();
    }
}

impl Default for SessionPortResolver {
    fn default() -> Self {
        Self::new(Arc::new(super::runtime::RuntimeLifecycle::default()))
    }
}

#[cfg(test)]
fn test_runtime_receipt(ports: CandidatePortBindings) -> super::runtime::RuntimeApplyReceipt {
    use crate::{
        client::application_workflow::policy::CoreRunIntent, config::chimera::ClashCore,
        core::actor_v2::control_endpoint::ExecutionHost,
    };

    super::runtime::RuntimeApplyReceipt {
        revision: super::runtime::RuntimeRevisionAllocator::default()
            .allocate()
            .expect("test runtime revision should allocate"),
        config_text: Arc::from(""),
        config_digest: String::new(),
        target_core: ClashCore::Mihomo,
        core_spec: chimera_core_manager::CoreSpec {
            kind: chimera_core_manager::CoreKind::Mihomo,
            binary_path: camino::Utf8PathBuf::from("/test/mihomo"),
            version: None,
            features: Vec::new(),
        },
        host: ExecutionHost::Local,
        run_intent: CoreRunIntent::Running,
        local_ipc: chimera_core_manager::LocalIpcSettings {
            policy: chimera_core_manager::LocalIpcPolicy::Disable,
            keep_http_controller: true,
        },
        applied_revision: chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 1,
            generation: 1,
            source_hash: String::new(),
            effective_hash: String::new(),
        },
        ports,
        artifact: None,
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

    fn resolve_and_confirm(
        resolver: &SessionPortResolver,
        clash: &ClashConfig,
    ) -> CandidatePortBindings {
        let candidate = resolver.resolve_candidate(clash).unwrap();
        resolver.confirm(&candidate);
        candidate
    }

    fn fixed(port: u16) -> PortStrategy {
        PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: port,
        }
    }

    #[test]
    fn resolves_fixed_strategies_and_formats_external_controller() {
        // Reserve three distinct available ports instead of relying on fixed
        // ports that may already be occupied by other CI workers.
        let mixed_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let socks_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let external_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let mixed = mixed_probe.local_addr().unwrap().port();
        let socks = socks_probe.local_addr().unwrap().port();
        let external = external_probe.local_addr().unwrap().port();
        drop((mixed_probe, socks_probe, external_probe));

        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(mixed);
        clash.socks_port = Some(fixed(socks));
        clash.http_port = None;
        clash.external_controller.port = fixed(external);

        let candidate = resolver.resolve_candidate(&clash).unwrap();
        let ports = candidate.bindings();
        assert_eq!(ports.mixed_port, mixed);
        assert_eq!(ports.socks_port, Some(socks));
        assert_eq!(ports.port, None);
        assert_eq!(
            ports.external_controller.as_deref(),
            Some(format!("127.0.0.1:{external}").as_str())
        );
        assert_eq!(resolver.confirmed(), None);
        resolver.confirm(&candidate);
        assert_eq!(resolver.confirmed(), Some(ports.clone()));
    }

    #[test]
    fn random_pick_is_sticky_until_fingerprint_changes() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 0,
        };

        let first = resolve_and_confirm(&resolver, &clash);
        let second = resolver.resolve_candidate(&clash).unwrap();
        assert_eq!(
            first.bindings(),
            second.bindings(),
            "same fingerprint must reuse the session pick"
        );

        clash.socks_port = Some(fixed(48234));
        let third = resolver.resolve_candidate(&clash).unwrap();
        assert_eq!(third.bindings().socks_port, Some(48234));
        assert_eq!(
            third.bindings().mixed_port,
            first.bindings().mixed_port,
            "unchanged mixed strategy must keep the session pick"
        );
        assert_eq!(
            resolver.confirmed(),
            Some(first.bindings().clone()),
            "an unconfirmed candidate must not replace the last applied binding"
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
        let first = resolve_and_confirm(&resolver, &clash);
        assert_eq!(first.bindings().mixed_port, mixed);

        let _core = TcpListener::bind(("127.0.0.1", mixed)).unwrap();
        clash.socks_port = Some(fixed(socks));
        let second = resolver
            .resolve_candidate(&clash)
            .expect("unchanged mixed port must not be re-probed");
        assert_eq!(second.bindings().mixed_port, mixed);
        assert_eq!(second.bindings().socks_port, Some(socks));
    }

    #[test]
    fn external_host_change_keeps_port_pick() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let external = probe.local_addr().unwrap().port();
        drop(probe);

        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.external_controller.port = fixed(external);
        let first = resolve_and_confirm(&resolver, &clash);
        assert_eq!(
            first.bindings().external_controller.as_deref(),
            Some(format!("127.0.0.1:{external}").as_str())
        );

        let _core = TcpListener::bind(("127.0.0.1", external)).unwrap();
        clash.external_controller.host = "0.0.0.0".parse().unwrap();
        let second = resolver
            .resolve_candidate(&clash)
            .expect("host-only change must not re-probe the external port");
        assert_eq!(
            second.bindings().external_controller.as_deref(),
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

        let ports = resolver.resolve_candidate(&clash).unwrap();
        assert_ne!(ports.bindings().mixed_port, taken);
    }

    #[test]
    // Contract: invalidate the accepted pick while another listener owns its
    // port; the next AllowFallback candidate must choose a new free port.
    // Reusing an unproven cached pick fails the final assertion.
    fn invalidation_drops_unproven_binding_before_the_next_candidate() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = probe.local_addr().unwrap().port();
        drop(probe);
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy::new_allow_fallback(taken);

        let confirmed = resolve_and_confirm(&resolver, &clash);
        assert_eq!(confirmed.bindings().mixed_port, taken);
        let _core = TcpListener::bind(("127.0.0.1", taken)).unwrap();
        resolver.invalidate();
        assert_eq!(resolver.confirmed(), None);
        assert!(
            resolver.lifecycle.confirmed_receipt().is_some(),
            "invalidating the accepted binding must preserve the receipt for recovery"
        );

        let next = resolver
            .resolve_candidate(&clash)
            .expect("invalidating an uncertain binding permits a fresh fallback pick");
        assert_ne!(
            next.bindings().mixed_port,
            taken,
            "a stale confirmed pick must not be reused after invalidation"
        );
    }

    #[test]
    // Contract: keep one accepted binding, resolve a different config without
    // applying it, and prove the accepted binding remains the resolver baseline.
    // A resolver that commits candidate picks during preparation fails here.
    fn candidate_resolution_does_not_change_confirmed_binding() {
        let resolver = SessionPortResolver::default();
        let mut baseline = ClashConfig::default();
        baseline.mixed_port = PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 0,
        };
        let confirmed = resolve_and_confirm(&resolver, &baseline);

        let mut candidate_config = baseline.clone();
        candidate_config.socks_port = Some(fixed(48235));
        let candidate = resolver.resolve_candidate(&candidate_config).unwrap();

        assert_eq!(candidate.bindings().socks_port, Some(48235));
        assert_eq!(resolver.confirmed(), Some(confirmed.bindings().clone()));
    }
}

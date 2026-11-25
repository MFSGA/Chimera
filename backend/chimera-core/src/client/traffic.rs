//! Committed state readers for the shared traffic observer.
use super::{application::ApplicationClient, clash_config::ClashConfigClient};
use crate::traffic::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy,
};
use chimera_contracts::net::DirectEgress;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct SelectedProfile(crate::state::StateSnapshot<chimera_config::profile::Profiles>);

impl SelectedProfile {
    pub fn new(profiles: crate::state::StateSnapshot<chimera_config::profile::Profiles>) -> Self {
        Self(profiles)
    }
}

impl ProfileSelection for SelectedProfile {
    fn current(&self) -> Option<String> {
        self.0
            .load()
            .state
            .current
            .as_ref()
            .map(ToString::to_string)
    }
}

pub struct SettingsRetention(ApplicationClient);

impl SettingsRetention {
    pub fn new(application: ApplicationClient) -> Self {
        Self(application)
    }
}

impl RetentionPolicy for SettingsRetention {
    fn retention(&self) -> Option<Duration> {
        self.0.get_typed().traffic_retention.duration()
    }
}

/// Ref's traffic source cache: only an explicitly requested direct-egress
/// probe can supply a public local address. A failed or disallowed probe
/// supplies none; the traffic recorder never performs network probing itself.
///
/// Keep the committed application and Clash owners, not ChimeraClient, so the
/// cache cannot introduce a ClientInner reference cycle.
pub struct LocalSourceCache {
    application: ApplicationClient,
    clash: ClashConfigClient,
    observed: parking_lot::RwLock<Option<LocalSourceObservation>>,
}

/// Bind a completed probe to its committed opt-in/capture settings. This
/// Chimera-specific fence avoids reviving an old proxy egress after a user
/// disables and later re-enables direct IP probing.
#[derive(Clone, Copy)]
struct LocalSourceObservation {
    addresses: LocalSourceIps,
    source_versions: (u64, u64),
}

impl LocalSourceCache {
    pub fn new(application: ApplicationClient, clash: ClashConfigClient) -> Self {
        Self {
            application,
            clash,
            observed: parking_lot::RwLock::new(None),
        }
    }

    pub fn source_versions(&self) -> (u64, u64) {
        (
            self.application.committed_runtime_source().1,
            self.clash.committed_runtime_source().1,
        )
    }

    /// Store only a user's explicit, completed probe, if the opt-in and
    /// capture settings have not advanced while its network IO was pending.
    /// Parse by family rather than trusting remote echo text.
    pub fn set(&self, result: &DirectEgress, started_at: (u64, u64)) {
        let (app, app_version) = self.application.committed_runtime_source();
        let (clash, clash_version) = self.clash.committed_runtime_source();
        let observed = if (app_version, clash_version) == started_at
            && super::direct_egress::blocked(&app, &clash).is_none()
        {
            match result {
                DirectEgress::Probed { ipv4, ipv6 } => Some(LocalSourceObservation {
                    addresses: LocalSourceIps {
                        ipv4: ipv4.as_ref().and_then(|egress| egress.address.parse().ok()),
                        ipv6: ipv6.as_ref().and_then(|egress| egress.address.parse().ok()),
                    },
                    source_versions: started_at,
                }),
                _ => None,
            }
        } else {
            None
        };
        *self.observed.write() = observed;
    }
}

impl LocalSourceLocation for LocalSourceCache {
    fn addresses(&self) -> LocalSourceIps {
        // A source ceases to be authoritative when consent, TUN, transparent
        // capture or *any* committed setting has changed since the probe.
        // Fail closed until another explicitly requested direct-egress probe.
        let (app, app_version) = self.application.committed_runtime_source();
        let (clash, clash_version) = self.clash.committed_runtime_source();
        if super::direct_egress::blocked(&app, &clash).is_some() {
            return LocalSourceIps::default();
        }
        self.observed
            .read()
            .as_ref()
            .filter(|source| source.source_versions == (app_version, clash_version))
            .map_or(LocalSourceIps::default(), |source| source.addresses)
    }
}

impl chimera_contracts::net::ProbePermission for LocalSourceCache {
    fn allows_stun(&self, server: &chimera_config::application::StunServer) -> bool {
        let (app, _) = self.application.committed_runtime_source();
        let (clash, _) = self.clash.committed_runtime_source();
        super::direct_egress::blocked(&app, &clash).is_none()
            && server.enabled
            && app.stun_servers.contains(server)
    }
    fn allows_lookup(&self, provider: chimera_config::application::IpLookupProvider) -> bool {
        let (app, _) = self.application.committed_runtime_source();
        let (clash, _) = self.clash.committed_runtime_source();
        super::direct_egress::blocked(&app, &clash).is_none()
            && app.ip_lookup_providers.contains(&provider)
    }
}
impl LocalSourceCache {
    pub fn blocked(&self) -> Option<DirectEgress> {
        super::direct_egress::blocked(&self.application.get_typed(), &self.clash.get_typed())
    }
    pub fn probe_sources(self: &Arc<Self>) -> chimera_contracts::net::ProbeSources {
        chimera_contracts::net::ProbeSources::from_config(
            &self.application.get_typed(),
            self.clone(),
        )
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            })
    }
}

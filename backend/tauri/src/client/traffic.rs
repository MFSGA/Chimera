//! Traffic recording client and adapters for committed Chimera state.
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chimera_traffic::{
    ClosedCursor, ClosedPage, Dimension, Metric, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficReport, TrafficSummary, UsageCursor, UsageGroup, UsagePage,
};
use tokio::sync::watch;

use super::{
    ChimeraClient, application::ApplicationClient, clash_config::ClashConfigClient,
    profiles::ProfilesClient,
};
use crate::core::geo::GeodataMode;
use crate::core::{
    geo::{CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexClient},
    traffic::{
        Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficArgs,
        TrafficClient,
    },
};
use crate::network_info::DirectEgress;
use chimera_core::actor_v2::{
    CoreClient,
    router::{CoreStatusProjection, EndpointConnectivity},
};
use chimera_core::clash::ws::ClashConnectionsFrame;
use chimera_ipc::api::status::CoreStateDetail;

struct SelectedProfile(ProfilesClient);

impl ProfileSelection for SelectedProfile {
    fn current(&self) -> Option<String> {
        self.0.snapshot().current.as_ref().map(ToString::to_string)
    }
}

struct SettingsRetention(ApplicationClient);

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
pub(super) struct LocalSourceCache {
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
    pub(super) fn new(application: ApplicationClient, clash: ClashConfigClient) -> Self {
        Self {
            application,
            clash,
            observed: parking_lot::RwLock::new(None),
        }
    }

    pub(super) fn source_versions(&self) -> (u64, u64) {
        (
            self.application.committed_runtime_source().1,
            self.clash.committed_runtime_source().1,
        )
    }

    /// Store only a user's explicit, completed probe, if the opt-in and
    /// capture settings have not advanced while its network IO was pending.
    /// Parse by family rather than trusting remote echo text.
    pub(super) fn set(&self, result: &DirectEgress, started_at: (u64, u64)) {
        let (app, app_version) = self.application.committed_runtime_source();
        let (clash, clash_version) = self.clash.committed_runtime_source();
        let observed = if (app_version, clash_version) == started_at
            && crate::network_info::blocked(&app, &clash).is_none()
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
        if crate::network_info::blocked(&app, &clash).is_some() {
            return LocalSourceIps::default();
        }
        self.observed
            .read()
            .as_ref()
            .filter(|source| source.source_versions == (app_version, clash_version))
            .map_or(LocalSourceIps::default(), |source| source.addresses)
    }
}

/// Geodata selection must follow the same ref CoreActor instance as
/// connection traffic. Never use a second Facade status or a stale controller.
pub(super) struct ActiveCoreGeodataMode(pub(super) CoreClient);

fn running_actor_geodata_instance(projection: &CoreStatusProjection) -> anyhow::Result<&str> {
    anyhow::ensure!(
        matches!(projection.connectivity, EndpointConnectivity::Connected),
        "the CoreActor host is not connected for geodata inspection"
    );
    let snapshot = projection
        .snapshot
        .as_ref()
        .context("the CoreActor has no authoritative geodata status")?;
    anyhow::ensure!(
        matches!(snapshot.state, Some(CoreStateDetail::Running { .. })),
        "the CoreActor is not running for geodata inspection"
    );
    snapshot
        .instance_id
        .as_deref()
        .filter(|instance| !instance.is_empty())
        .context("the CoreActor has no confirmed geodata instance")
}

fn actor_geodata_lease_still_owned(
    before: &CoreStatusProjection,
    after: &CoreStatusProjection,
    api_instance: &str,
) -> bool {
    if before.host != after.host || before.generation != after.generation {
        return false;
    }
    let (Ok(first), Ok(last)) = (
        running_actor_geodata_instance(before),
        running_actor_geodata_instance(after),
    ) else {
        return false;
    };
    first == api_instance
        && last == api_instance
        && before.snapshot.as_ref().and_then(|s| s.revision.as_ref())
            == after.snapshot.as_ref().and_then(|s| s.revision.as_ref())
}

#[async_trait]
impl CoreGeodataModeSource for ActiveCoreGeodataMode {
    async fn mode(&self) -> anyhow::Result<CoreGeodataBinding> {
        let before = self.0.refresh_status().await?;
        let running_instance = running_actor_geodata_instance(&before)?;
        let api = self.0.api_client().await?;
        anyhow::ensure!(
            api.instance_id() == Some(running_instance) && !api.is_revoked(),
            "the CoreActor changed while acquiring the geodata API lease"
        );
        let config = api.get_configs().await?;
        let after = self.0.refresh_status().await?;
        anyhow::ensure!(
            !api.is_revoked()
                && actor_geodata_lease_still_owned(
                    &before,
                    &after,
                    api.instance_id().unwrap_or_default(),
                ),
            "the CoreActor changed host, instance, or applied revision while reading geodata mode"
        );
        Ok(CoreGeodataBinding {
            instance_id: running_instance.to_owned(),
            mode: if config.geodata_mode == Some(true) {
                GeodataMode::Dat
            } else {
                GeodataMode::Mmdb
            },
        })
    }
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            })
    }
}

impl ChimeraClient {
    /// Publish an explicit, privacy-gated direct-egress observation for
    /// traffic source attribution. No extra probe or timer is started.
    pub(crate) fn direct_egress_source_versions(&self) -> (u64, u64) {
        self.inner.local_source.source_versions()
    }

    pub(crate) fn record_direct_egress(&self, result: &DirectEgress, started_at: (u64, u64)) {
        self.inner.local_source.set(result, started_at);
    }

    /// Uses the same published country index as traffic; no network geolocation
    /// request is made and a missing core database remains explicitly unknown.
    pub(crate) fn geo_country(&self, ip: std::net::IpAddr) -> Option<String> {
        let geo = self.inner.geo_index.read().clone()?;
        let published = geo.subscribe();
        let current = published.borrow();
        let snapshot = current.as_ref()?;
        snapshot
            .index
            .lookup(ip)?
            .country()
            .map(str::to_ascii_uppercase)
    }

    pub(crate) fn start_traffic(
        &self,
        frames: tokio::sync::watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    ) -> Result<()> {
        let Some(store) = self.inner.traffic_store.lock().take() else {
            return Ok(());
        };
        let profiles = self
            .profiles_client()
            .context("traffic recording needs committed profiles")?
            .clone();
        let geo_index = self.inner.geo_index.read().clone();
        let geo = geo_index
            .as_ref()
            .map(GeoIndexClient::subscribe)
            .unwrap_or_else(|| {
                let (_, receiver) = watch::channel(None::<Arc<CountryIndexSnapshot>>);
                receiver
            });
        let geo_mode_source = geo_index
            .as_ref()
            .and_then(|_| self.inner.core_api.as_ref())
            .map(|core| {
                Arc::new(ActiveCoreGeodataMode(core.clone())) as Arc<dyn CoreGeodataModeSource>
            });
        let args = TrafficArgs {
            store,
            profiles: Arc::new(SelectedProfile(profiles)),
            retention: Arc::new(SettingsRetention(self.inner.application.clone())),
            clock: Arc::new(SystemClock),
            frames,
            geo,
            geo_mode_source,
            geo_mode_refresh: geo_index,
            local_source: self.inner.local_source.clone(),
        };
        let traffic = tauri::async_runtime::block_on(crate::core::traffic::spawn(
            args,
            self.inner.shutdown.child_token(),
            &self.inner.tasks,
        ))
        .context("failed to start traffic recording actor")?;
        *self.inner.traffic.write() = Some(traffic);
        Ok(())
    }

    fn traffic(&self) -> Result<TrafficClient> {
        self.inner
            .traffic
            .read()
            .clone()
            .ok_or_else(|| anyhow!("traffic recording is unavailable"))
    }

    pub(crate) async fn traffic_summary(&self) -> Result<TrafficSummary> {
        self.traffic()?.summary().await
    }

    pub(crate) async fn query_traffic_report(
        &self,
        request: ReportRequest,
    ) -> Result<TrafficReport> {
        self.traffic()?.report(request).await
    }

    pub(crate) async fn query_traffic_usage(
        &self,
        query: TrafficQuery,
        group_by: Dimension,
        metric: Metric,
        after: Option<UsageCursor>,
        limit: usize,
    ) -> Result<UsagePage> {
        self.traffic()?
            .usage(query, group_by, metric, after, limit)
            .await
    }

    pub(crate) async fn query_traffic_usage_by_keys(
        &self,
        query: TrafficQuery,
        group_by: Dimension,
        keys: Vec<String>,
    ) -> Result<Vec<UsageGroup>> {
        self.traffic()?.usage_by_keys(query, group_by, keys).await
    }

    pub(crate) async fn query_traffic_closed_connections(
        &self,
        range: TrafficRange,
        filters: Vec<TrafficFilter>,
        before: Option<ClosedCursor>,
        limit: usize,
    ) -> Result<ClosedPage> {
        self.traffic()?
            .closed_connections(range, filters, before, limit)
            .await
    }

    pub(crate) async fn query_traffic_active_connection_ids(
        &self,
        filters: Vec<TrafficFilter>,
    ) -> Result<Vec<String>> {
        self.traffic()?.active_connection_ids(filters).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::actor_v2::endpoint::{CoreStatusSnapshot, ExecutionHost};
    use chimera_ipc::api::status::RevisionIdInfo;

    fn connected_running() -> CoreStatusProjection {
        CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 3,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("core-A".into()),
                controller: None,
                state: Some(CoreStateDetail::Running { epoch: 7, pid: 42 }),
                state_changed_at: 11,
                revision: Some(RevisionIdInfo {
                    epoch: 7,
                    generation: 2,
                    effective_hash: "effective-A".into(),
                }),
                source_hash: Some("source-A".into()),
                healthy: Some(true),
                applied_kind: None,
            }),
        }
    }

    #[test]
    fn geodata_mode_requires_connected_running_ref_core() {
        let running = connected_running();
        assert_eq!(running_actor_geodata_instance(&running).unwrap(), "core-A");

        let mut unknown = running.clone();
        unknown.snapshot = None;
        assert!(running_actor_geodata_instance(&unknown).is_err());

        let mut stopped = running.clone();
        stopped.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Stopped { reason: None });
        assert!(running_actor_geodata_instance(&stopped).is_err());

        let mut switching = running;
        switching.connectivity = EndpointConnectivity::HandingOff {
            from: ExecutionHost::Local,
            to: ExecutionHost::Service,
        };
        assert!(running_actor_geodata_instance(&switching).is_err());
    }

    #[test]
    fn geodata_mode_lease_is_fenced_by_host_generation_instance_and_revision() {
        let before = connected_running();
        let mut sampled = before.clone();
        sampled.snapshot.as_mut().unwrap().state_changed_at += 1;
        assert!(actor_geodata_lease_still_owned(&before, &sampled, "core-A"));

        let mut other_host = before.clone();
        other_host.host = ExecutionHost::Service;
        assert!(!actor_geodata_lease_still_owned(
            &before,
            &other_host,
            "core-A"
        ));

        let mut newer_generation = before.clone();
        newer_generation.generation += 1;
        assert!(!actor_geodata_lease_still_owned(
            &before,
            &newer_generation,
            "core-A"
        ));

        let mut new_instance = before.clone();
        new_instance.snapshot.as_mut().unwrap().instance_id = Some("core-B".into());
        assert!(!actor_geodata_lease_still_owned(
            &before,
            &new_instance,
            "core-A"
        ));

        let mut revised = before.clone();
        revised
            .snapshot
            .as_mut()
            .unwrap()
            .revision
            .as_mut()
            .unwrap()
            .generation += 1;
        assert!(!actor_geodata_lease_still_owned(
            &before, &revised, "core-A"
        ));

        let mut degraded = before.clone();
        degraded.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Local,
            reason: "endpoint unavailable".into(),
        };
        assert!(!actor_geodata_lease_still_owned(
            &before, &degraded, "core-A"
        ));
    }
}

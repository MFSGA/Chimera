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

use super::{ChimeraClient, application::ApplicationClient, profiles::ProfilesClient};
use crate::core::geo::GeodataMode;
use crate::core::{
    geo::{CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexClient},
    traffic::{
        Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficArgs,
        TrafficClient,
    },
};
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

struct NoLocalSource;

impl LocalSourceLocation for NoLocalSource {
    fn addresses(&self) -> LocalSourceIps {
        LocalSourceIps::default()
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
            local_source: Arc::new(NoLocalSource),
        };
        let traffic = tauri::async_runtime::block_on(TrafficClient::spawn(args))
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

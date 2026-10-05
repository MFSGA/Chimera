//! Traffic recording client and adapters for committed Chimera state.
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use chimera_traffic::{
    ClosedCursor, ClosedPage, Dimension, Metric, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficReport, TrafficSummary, UsageCursor, UsageGroup, UsagePage,
};

use super::{ChimeraClient, application::ApplicationClient, profiles::ProfilesClient};
use crate::core::{
    clash::ws::ClashConnectionsFrame,
    traffic::{
        Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficArgs,
        TrafficClient,
    },
};

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
        let args = TrafficArgs {
            store,
            profiles: Arc::new(SelectedProfile(profiles)),
            retention: Arc::new(SettingsRetention(self.inner.application.clone())),
            clock: Arc::new(SystemClock),
            frames,
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

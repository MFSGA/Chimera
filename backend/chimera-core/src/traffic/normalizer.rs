//! Preserve the existing Core-instance mode proof while using ref TrafficFeed.
use crate::geo::{CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexClient};
use crate::{
    clash::ws::ClashConnectionsFrame,
    client::traffic_frames::{frame_from_snapshot, geo::CountryLookup},
    traffic::{FrameNormalizer, LocalSourceIps},
};
use chimera_geodata::IpIndex;
use chimera_traffic::Frame;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, watch};

#[derive(Default)]
struct ModeState {
    core_instance_id: Option<String>,
    last_mode_check: Option<tokio::time::Instant>,
}
pub(super) struct InstanceBoundNormalizer {
    geo: watch::Receiver<Option<Arc<CountryIndexSnapshot>>>,
    geo_mode_source: Option<Arc<dyn CoreGeodataModeSource>>,
    geo_mode_refresh: Option<GeoIndexClient>,
    state: Mutex<ModeState>,
}

impl InstanceBoundNormalizer {
    pub(super) fn new(
        geo: watch::Receiver<Option<Arc<CountryIndexSnapshot>>>,
        geo_mode_source: Option<Arc<dyn CoreGeodataModeSource>>,
        geo_mode_refresh: Option<GeoIndexClient>,
    ) -> Self {
        Self {
            geo,
            geo_mode_source,
            geo_mode_refresh,
            state: Mutex::new(ModeState::default()),
        }
    }
}

#[async_trait::async_trait]
impl FrameNormalizer for InstanceBoundNormalizer {
    type Raw = ClashConnectionsFrame;

    fn normalize(
        &self,
        raw: &Self::Raw,
        wall_ms: i64,
        mono: Duration,
        _: Option<&IpIndex>,
        local: LocalSourceIps,
    ) -> Frame {
        let index = self
            .geo
            .borrow()
            .clone()
            .filter(|index| index.instance_id == raw.instance_id);
        frame_from_snapshot(
            raw,
            wall_ms,
            mono,
            index
                .as_ref()
                .map(|index| index.index.as_ref() as &dyn CountryLookup),
            local,
        )
    }

    async fn normalize_async(
        &self,
        raw: &Self::Raw,
        wall_ms: i64,
        mono: Duration,
        _: Option<&IpIndex>,
        local: LocalSourceIps,
    ) -> Frame {
        let mut state = self.state.lock().await;
        let frame_instance_changed =
            state.core_instance_id.as_deref() != Some(raw.instance_id.as_str());
        let mut mode_ready = true;
        if frame_instance_changed {
            if let (Some(source), Some(refresh)) = (&self.geo_mode_source, &self.geo_mode_refresh) {
                let retry_ready = state
                    .last_mode_check
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(1));
                if retry_ready {
                    state.last_mode_check = Some(tokio::time::Instant::now());
                    let mode = tokio::time::timeout(Duration::from_secs(3), source.mode()).await;
                    match mode {
                        Ok(Ok(binding)) if binding.instance_id == raw.instance_id => {
                            match refresh.set_mode(binding).await {
                                Ok(()) => {
                                    state.core_instance_id = Some(raw.instance_id.clone());
                                    state.last_mode_check = None;
                                }
                                Err(error) => {
                                    tracing::debug!(%error, "waiting for the matching country index before recording traffic");
                                    mode_ready = false;
                                }
                            }
                        }
                        Ok(Ok(binding)) => {
                            tracing::debug!(
                                frame_core = %raw.instance_id,
                                active_core = %binding.instance_id,
                                "ignoring a stale traffic frame from a different core instance"
                            );
                            mode_ready = false;
                        }
                        Ok(Err(error)) => {
                            tracing::debug!(%error, "waiting for the new core's geodata mode before recording traffic");
                            mode_ready = false;
                        }
                        Err(error) => {
                            tracing::debug!(%error, "new core geodata mode query timed out");
                            mode_ready = false;
                        }
                    }
                } else {
                    mode_ready = false;
                }
            } else {
                state.core_instance_id = Some(raw.instance_id.clone());
            }
        }
        let index = mode_ready
            .then(|| self.geo.borrow().clone())
            .flatten()
            .filter(|index| index.instance_id == raw.instance_id)
            .map(|index| index.index.clone());
        frame_from_snapshot(
            raw,
            wall_ms,
            mono,
            index.as_deref().map(|index| index as &dyn CountryLookup),
            local,
        )
    }

    async fn disconnected(&self) {
        *self.state.lock().await = ModeState::default();
    }
}

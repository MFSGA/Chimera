//! Shared ref Traffic file owner and resumable v3 migration.
mod storage;
pub use chimera_observers::traffic::*;
pub use chimera_tsdb::Profile;
pub use storage::TrafficFileStorage;

mod normalizer;
#[cfg(test)]
mod tests;
use crate::{
    clash::ws::ClashConnectionsFrame,
    geo::{CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexClient},
};
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct TrafficArgs {
    pub store: Arc<TrafficFileStorage>,
    pub profiles: Arc<dyn ProfileSelection>,
    pub retention: Arc<dyn RetentionPolicy>,
    pub clock: Arc<dyn Clock>,
    pub frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    pub geo: watch::Receiver<Option<Arc<CountryIndexSnapshot>>>,
    pub geo_mode_source: Option<Arc<dyn CoreGeodataModeSource>>,
    pub geo_mode_refresh: Option<GeoIndexClient>,
    pub local_source: Arc<dyn LocalSourceLocation>,
}

pub async fn spawn(
    args: TrafficArgs,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> anyhow::Result<TrafficClient> {
    let normalizer = Arc::new(normalizer::InstanceBoundNormalizer::new(
        args.geo,
        args.geo_mode_source,
        args.geo_mode_refresh,
    ));
    let (_, geo) = watch::channel(None);
    Ok(args
        .store
        .spawn(
            TrafficStartArgs {
                profile: Profile::desktop(),
                profiles: args.profiles,
                retention: args.retention,
                clock: args.clock,
                frames: TrafficFeed::new(args.frames, normalizer),
                geo,
                local_source: args.local_source,
            },
            shutdown,
            tasks,
        )
        .await?)
}

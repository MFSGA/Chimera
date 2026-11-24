//! Desktop adapter for the shared ref traffic observer.
mod normalizer;
#[cfg(test)]
mod tests;

use crate::core::geo::{CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexClient};
use chimera_core::{
    clash::ws::ClashConnectionsFrame,
    traffic::{Profile, TrafficFeed, TrafficFileStorage, TrafficOwnerFactory, TrafficStartArgs},
};
use std::sync::Arc;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub use chimera_core::traffic::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficClient,
};

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

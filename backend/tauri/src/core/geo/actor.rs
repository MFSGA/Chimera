//! Owns the country index and keeps it in step with the core.
use std::{sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::{sync::watch, task::JoinHandle};

use super::ports::{
    CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, CountryIndexSource,
    GeodataMode, IndexKey, Loaded, OnChange,
};

/// How long to wait before asking a core that did not answer again.
const RETRY: Duration = Duration::from_secs(1);
/// Recheck database contents in case the filesystem watcher missed a write.
const INDEX_FALLBACK_RECHECK_INTERVAL: Duration = Duration::from_secs(60);

pub struct GeoIndexArgs {
    pub source: Arc<dyn CountryIndexSource>,
    pub mode_source: Arc<dyn CoreGeodataModeSource>,
}

pub(super) enum Message {
    /// A core instance answered `GET /configs`.
    Mode(CoreGeodataBinding),
    /// A new traffic-feed instance has reported its mode and needs a synchronous index update.
    SetMode(CoreGeodataBinding, RpcReplyPort<()>),
    /// A country database in the core's home was written.
    FilesChanged,
    /// A low-frequency fallback for filesystems where the watcher missed a write.
    Refresh,
}

pub(super) struct GeoIndexActor;

pub(super) struct State {
    source: Arc<dyn CountryIndexSource>,
    instance_id: Option<String>,
    mode: Option<GeodataMode>,
    key: Option<IndexKey>,
    published: watch::Sender<Option<Arc<CountryIndexSnapshot>>>,
    /// Dropping it ends the watch.
    _watch: Option<Box<dyn Send>>,
    follower: JoinHandle<()>,
    refresher: JoinHandle<()>,
}

impl Actor for GeoIndexActor {
    type Msg = Message;
    type State = State;
    type Arguments = (
        GeoIndexArgs,
        watch::Sender<Option<Arc<CountryIndexSnapshot>>>,
    );

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        (args, published): Self::Arguments,
    ) -> Result<State, ActorProcessingErr> {
        let GeoIndexArgs {
            source,
            mode_source,
        } = args;
        let changed = {
            let myself = myself.clone();
            OnChange::new(move || {
                let _ = myself.cast(Message::FilesChanged);
            })
        };
        // Without the watch the index still follows every new core instance.
        let watch = match source.watch(changed) {
            Ok(guard) => Some(guard),
            Err(error) => {
                tracing::warn!(
                    "geo database updates are not watched: {}",
                    snafu::Report::from_error(error)
                );
                None
            }
        };
        let follower = tokio::spawn(follow_core(myself.clone(), mode_source));
        let refresher = tokio::spawn(refresh_index(myself, INDEX_FALLBACK_RECHECK_INTERVAL));
        Ok(State {
            source,
            instance_id: None,
            mode: None,
            key: None,
            published,
            _watch: watch,
            follower,
            refresher,
        })
    }

    async fn handle(
        &self,
        _: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Mode(binding) => {
                state.set_initial_mode(binding).await;
            }
            Message::SetMode(binding, reply) => {
                state.set_instance_mode(binding).await;
                let _ = reply.send(());
            }
            Message::FilesChanged | Message::Refresh => state.reload().await,
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.follower.abort();
        state.refresher.abort();
        if let Err(error) = (&mut state.follower).await
            && let Ok(panic) = error.try_into_panic()
        {
            std::panic::resume_unwind(panic);
        }
        if let Err(error) = (&mut state.refresher).await
            && let Ok(panic) = error.try_into_panic()
        {
            std::panic::resume_unwind(panic);
        }
        Ok(())
    }
}

impl State {
    async fn set_initial_mode(&mut self, binding: CoreGeodataBinding) {
        // A traffic frame may have already bound a newer core while startup polling was in flight.
        if self.mode.is_none() {
            self.instance_id = Some(binding.instance_id);
            self.mode = Some(binding.mode);
            self.reload().await;
        }
    }

    async fn set_instance_mode(&mut self, binding: CoreGeodataBinding) {
        let new_instance = self.instance_id.as_deref() != Some(binding.instance_id.as_str());
        if !new_instance && self.mode == Some(binding.mode) {
            return;
        }
        let changed = self.mode != Some(binding.mode);
        self.instance_id = Some(binding.instance_id);
        self.mode = Some(binding.mode);
        if new_instance
            || (changed
                && self
                    .key
                    .as_ref()
                    .is_some_and(|key| key.mode != binding.mode))
        {
            // Never expose a prior core's index while the newly bound core's database loads.
            self.key = None;
            self.published.send_replace(None);
        }
        self.reload().await;
    }

    async fn reload(&mut self) {
        // Nothing loads before the core reports which database it reads.
        let Some(mode) = self.mode else {
            return;
        };
        let (source, current) = (self.source.clone(), self.key.clone());
        let loaded = match tokio::task::spawn_blocking(move || source.load(mode, current)).await {
            Ok(loaded) => loaded,
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => {
                    tracing::warn!(%error, "the country index loader did not complete");
                    return;
                }
            },
        };
        match loaded {
            Ok(Loaded::Unchanged) => {}
            Ok(Loaded::Missing) => {
                self.key = None;
                self.published.send_replace(None);
            }
            Ok(Loaded::Index { key, index }) => {
                self.key = Some(key);
                if let Some(instance_id) = &self.instance_id {
                    self.published
                        .send_replace(Some(Arc::new(CountryIndexSnapshot {
                            instance_id: instance_id.clone(),
                            index,
                        })));
                }
            }
            // The previous index describes an older database, which beats none.
            Err(error) => tracing::warn!(
                "the country index was not reloaded: {}",
                snafu::Report::from_error(error)
            ),
        }
    }
}

/// Reads the startup core's `geodata-mode` until its API becomes available. Later core bindings
/// report their mode with the first connection frame for that instance. Cores without the field
/// (clash-rs, Clash Premium) read the mmdb.
async fn follow_core(actor: ActorRef<Message>, source: Arc<dyn CoreGeodataModeSource>) {
    loop {
        match source.mode().await {
            Ok(binding) => {
                if actor.cast(Message::Mode(binding)).is_err() {
                    return;
                }
                return;
            }
            Err(error) => {
                tracing::debug!(%error, "the core did not report its geodata mode");
                tokio::time::sleep(RETRY).await;
            }
        }
    }
}

async fn refresh_index(actor: ActorRef<Message>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    // The first tick is immediate; leave the initial load to the core mode message.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        if actor.cast(Message::Refresh).is_err() {
            return;
        }
    }
}

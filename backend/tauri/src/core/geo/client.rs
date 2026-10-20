//! Typed client for the country index owner.
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    actor::{GeoIndexActor, GeoIndexArgs, Message},
    ports::{CoreGeodataBinding, CountryIndexSnapshot},
};

#[derive(Clone)]
pub struct GeoIndexClient(Arc<Inner>);

struct Inner {
    actor: ActorRef<Message>,
    index: watch::Receiver<Option<Arc<CountryIndexSnapshot>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

impl GeoIndexClient {
    /// Publish the mode reported by the newly bound core before accepting its traffic frame.
    pub async fn set_mode(&self, binding: CoreGeodataBinding) -> Result<()> {
        match self
            .0
            .actor
            .call(
                |reply| Message::SetMode(binding, reply),
                Some(std::time::Duration::from_secs(5)),
            )
            .await
            .context("could not call the geo index actor")?
        {
            CallResult::Success(()) => Ok(()),
            CallResult::SenderError => bail!("geo index actor reply dropped"),
            CallResult::Timeout => bail!("geo index mode update timed out"),
        }
    }

    #[cfg(test)]
    pub(crate) async fn stop_for_test(&self) -> Result<()> {
        self.0.actor.stop_and_wait(None, None).await?;
        Ok(())
    }

    pub async fn spawn(
        args: GeoIndexArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self> {
        let (published, index) = watch::channel(None::<Arc<CountryIndexSnapshot>>);
        let (actor, _) = Actor::spawn(None, GeoIndexActor, (args, published)).await?;
        let cell = actor.get_cell();
        tasks.spawn(async move {
            shutdown.cancelled().await;
            if let Err(error) = cell.stop_and_wait(None, None).await {
                tracing::warn!(%error, "the country index actor did not stop cleanly");
            }
        });
        Ok(Self(Arc::new(Inner { actor, index })))
    }

    /// The index of the database the running core reads; `None` until it loads, or while the
    /// core's home has no such database.
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<CountryIndexSnapshot>>> {
        self.0.index.clone()
    }

    #[cfg(test)]
    pub(super) fn send(&self, message: Message) {
        self.0
            .actor
            .cast(message)
            .expect("the geo index actor is running");
    }
}

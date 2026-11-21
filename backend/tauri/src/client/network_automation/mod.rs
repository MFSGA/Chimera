//! Network automation actor behind a typed client.
//!
//! Ref-aligned actor and injected ports are migrated first. Production does
//! not yet spawn this actor: host observation, validated settings commits,
//! privacy-scoped RPC and one existing capture owner must be wired together
//! before the feature can change the machine's network configuration.

mod actor;
pub(crate) mod ports;
mod switches;

#[cfg(test)]
mod tests;

use chimera_config::application::NetworkAutomation;
use chimera_contracts::network_automation::NetworkAutomationStatus;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::task::TaskTracker;

use self::actor::{Message, NetworkAutomationActor};

pub(crate) use self::{actor::Args as NetworkAutomationArgs, switches::ConfigCaptureSwitches};

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

#[derive(Clone)]
pub(crate) struct NetworkAutomationClient {
    actor: ActorRef<Message>,
}

impl NetworkAutomationClient {
    pub(crate) async fn spawn(
        args: NetworkAutomationArgs,
        tasks: &TaskTracker,
    ) -> Result<Self, Error> {
        let shutdown = args.shutdown.clone();
        let (actor, _handle) = Actor::spawn(None, NetworkAutomationActor, args)
            .await
            .map_err(|source| Error::SpawnActor { source })?;
        // Match the existing Chimera system-proxy actor's ownership. There is
        // exactly one mailbox per instance, never a detached polling task.
        let cell = actor.get_cell();
        tasks.spawn(async move {
            shutdown.cancelled().await;
            let _ = cell.drain_and_wait(None).await;
        });
        Ok(Self { actor })
    }

    pub(crate) fn set_policy(&self, policy: NetworkAutomation) {
        let _ = self.actor.cast(Message::PolicyChanged(policy));
    }

    pub(crate) async fn status(&self) -> NetworkAutomationStatus {
        self.call(Message::Status).await
    }

    pub(crate) async fn apply_now(&self) -> NetworkAutomationStatus {
        self.call(Message::ApplyNow).await
    }

    pub(crate) async fn request_wifi_name(&self) -> NetworkAutomationStatus {
        self.call(Message::RequestWifiName).await
    }

    async fn call(
        &self,
        message: impl FnOnce(RpcReplyPort<NetworkAutomationStatus>) -> Message,
    ) -> NetworkAutomationStatus {
        match self.actor.call(message, None).await {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the network automation actor did not answer: {other:?}");
                NetworkAutomationStatus::unsupported()
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn poll(&self) {
        let _ = self.actor.cast(Message::Poll);
        let _ = self.status().await;
    }
}

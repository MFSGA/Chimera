//! Network automation: rules that switch system proxy, TUN and transparent
//! proxy when the machine arrives on a network.
//!
//! One actor owns the observation and what was last applied; callers reach
//! it through [`NetworkAutomationClient`] and never see a `ractor::ActorRef`.

mod actor;
pub mod ports;

#[cfg(test)]
mod tests;

use chimera_config::application::NetworkAutomation;
use chimera_runtime_values::network_automation::NetworkAutomationStatus;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::task::TaskTracker;

use self::actor::{Message, NetworkAutomationActor};

pub use self::actor::Args as NetworkAutomationArgs;

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

#[derive(Clone)]
pub struct NetworkAutomationClient {
    actor: ActorRef<Message>,
}

impl NetworkAutomationClient {
    /// The actor stops reading the network and writing switches once
    /// `args.shutdown` is cancelled, and `tasks` waits for it.
    pub async fn spawn(args: NetworkAutomationArgs, tasks: &TaskTracker) -> Result<Self, Error> {
        let shutdown = args.shutdown.clone();
        let (actor, _handle) = Actor::spawn(None, NetworkAutomationActor, args)
            .await
            .map_err(|source| Error::SpawnActor { source })?;
        chimera_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self { actor })
    }

    /// Hands the actor the committed policy. Nothing waits for it: the
    /// settings commit that produced it has already answered its caller.
    pub fn set_policy(&self, policy: NetworkAutomation) {
        let _ = self.actor.cast(Message::PolicyChanged(policy));
    }

    pub async fn status(&self) -> NetworkAutomationStatus {
        self.call(Message::Status).await
    }

    /// Reads the network and applies its actions again; a manual switch made
    /// since the last application is overwritten.
    pub async fn apply_now(&self) -> NetworkAutomationStatus {
        self.call(Message::ApplyNow).await
    }

    /// Reads the Wi-Fi name now, starting the platform's consent request
    /// where it needs one, and answers a status holding that reading.
    pub async fn request_wifi_name(&self) -> NetworkAutomationStatus {
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

    /// Delivers one poll and waits for it to be handled, so a test
    /// synchronises on the mailbox instead of on a clock.
    #[cfg(test)]
    pub async fn poll(&self) {
        let _ = self.actor.cast(Message::Poll);
        let _ = self.status().await;
    }
}

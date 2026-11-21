//! Temporary boundary for replacing the legacy Service command lane with the
//! ref ServiceClient in the desktop composition root.
//!
//! The *production* port owns only a handle to the ref Service Actor; every
//! actual elevated OS call goes through its serialized mailbox. The legacy
//! implementation remains for tests of the not-yet-migrated CoreFacade.

use std::sync::atomic::{AtomicBool, Ordering};

use chimera_contracts::service::{ServiceCommand, ServiceCommandError};
use chimera_ipc::types::StatusInfo;

use super::{
    service_command_actor::ServiceCommandActor,
    service_router::{ServiceClient, ServicePhase},
};

#[cfg(test)]
#[path = "service_command_port_tests.rs"]
mod tests;

#[async_trait::async_trait]
pub trait ServiceCommandPort: Send + Sync + 'static {
    fn has_outstanding(&self) -> bool;
    fn has_unsettled_outcome(&self) -> bool;
    async fn probe(&self) -> Result<StatusInfo<'static>, ServiceCommandError>;
    async fn command(&self, command: ServiceCommand) -> Result<(), ServiceCommandError>;
    async fn command_settled(&self) -> bool;
    async fn try_reap_finished(&self) -> bool;
    /// Only the legacy fallback needs this extra ownership drain: the ref
    /// ServiceActor already owns its shutdown through TaskTracker.
    async fn drain_legacy_on_shutdown(&self) {}
    async fn uninstall(&self) -> anyhow::Result<()>;
    async fn daemon_not_running(&self) -> bool;
}

#[async_trait::async_trait]
impl ServiceCommandPort for ServiceCommandActor {
    fn has_outstanding(&self) -> bool {
        Self::has_outstanding(self)
    }
    fn has_unsettled_outcome(&self) -> bool {
        Self::has_unsettled_outcome(self)
    }
    async fn probe(&self) -> Result<StatusInfo<'static>, ServiceCommandError> {
        Self::probe(self).await
    }
    async fn command(&self, command: ServiceCommand) -> Result<(), ServiceCommandError> {
        Self::command(self, command).await
    }
    async fn command_settled(&self) -> bool {
        Self::command_settled(self).await
    }
    async fn try_reap_finished(&self) -> bool {
        Self::try_reap_finished(self).await
    }
    async fn drain_legacy_on_shutdown(&self) {
        Self::drain_legacy_on_shutdown(self).await
    }
    async fn uninstall(&self) -> anyhow::Result<()> {
        Self::uninstall(self).await
    }
    async fn daemon_not_running(&self) -> bool {
        Self::daemon_not_running(self).await
    }
}

/// One ref ServiceClient is the only authority allowed to execute elevated
/// commands in production. A failed/ambiguous call latches until the actor
/// positively confirms that all previously admitted commands have settled.
pub struct ServiceClientCommandPort {
    actor: ServiceClient,
    unsettled: AtomicBool,
}

impl ServiceClientCommandPort {
    pub fn new(actor: ServiceClient) -> Self {
        Self {
            actor,
            unsettled: AtomicBool::new(false),
        }
    }

    fn observe_command(
        &self,
        command: ServiceCommand,
        result: Result<(), chimera_core_manager::CoreError>,
    ) -> Result<(), ServiceCommandError> {
        result.map_err(|error| {
            self.unsettled.store(true, Ordering::Release);
            ServiceCommandError::RunElevated {
                command,
                source: std::io::Error::other(error),
            }
        })
    }
}

#[async_trait::async_trait]
impl ServiceCommandPort for ServiceClientCommandPort {
    fn has_outstanding(&self) -> bool {
        // The ref actor owns outstanding-helper state. The adapter's latch
        // is conservative: a failed RPC cannot prove that the helper ended.
        self.unsettled.load(Ordering::Acquire)
    }

    fn has_unsettled_outcome(&self) -> bool {
        self.unsettled.load(Ordering::Acquire)
    }

    async fn probe(&self) -> Result<StatusInfo<'static>, ServiceCommandError> {
        let state =
            self.actor
                .probe()
                .await
                .map_err(|error| ServiceCommandError::RunServiceStatus {
                    source: std::io::Error::other(error),
                })?;
        // The ref status projection deliberately retains its last known
        // StatusInfo during uncertain transitions and after restart-budget
        // exhaustion. That cached coarse value is NOT a fresh OS stop proof.
        if !matches!(
            state.phase,
            ServicePhase::Ready
                | ServicePhase::NotInstalled
                | ServicePhase::DaemonStopped
                | ServicePhase::Incompatible
        ) {
            return Err(ServiceCommandError::RunServiceStatus {
                source: std::io::Error::other(
                    "the Service Actor did not publish a settled daemon state",
                ),
            });
        }
        Ok(StatusInfo {
            name: state.name,
            version: state.version,
            status: state.status,
            server: state.server,
        })
    }

    async fn command(&self, command: ServiceCommand) -> Result<(), ServiceCommandError> {
        let result = match command {
            ServiceCommand::Install => self.actor.install().await,
            ServiceCommand::Update => self.actor.update().await,
            ServiceCommand::Uninstall => self.actor.uninstall().await,
            ServiceCommand::Start => self.actor.start_daemon().await,
            ServiceCommand::Stop => self.actor.stop_daemon().await,
            ServiceCommand::Restart => self.actor.restart_daemon().await,
        };
        self.observe_command(command, result)
    }

    async fn command_settled(&self) -> bool {
        match self.actor.command_settled().await {
            Ok(true) => {
                self.unsettled.store(false, Ordering::Release);
                true
            }
            Ok(false) | Err(_) => {
                self.unsettled.store(true, Ordering::Release);
                false
            }
        }
    }

    async fn try_reap_finished(&self) -> bool {
        // The try-begin path is intentionally nonblocking: until a full
        // CommandSettled mailbox turn provides proof, refuse admission.
        !self.unsettled.load(Ordering::Acquire)
    }

    async fn uninstall(&self) -> anyhow::Result<()> {
        self.command(ServiceCommand::Uninstall)
            .await
            .map_err(Into::into)
    }

    async fn daemon_not_running(&self) -> bool {
        match self.actor.daemon_not_running().await {
            Ok(proof) => proof,
            Err(error) => {
                tracing::warn!(%error, "Service Actor could not prove the daemon stopped");
                false
            }
        }
    }
}

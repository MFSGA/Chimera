//! Startup-injected connection to the application mutation participant.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core::state::{DecisionHandle, StateParticipant};
use chimera_core_manager::OperationId;
use tokio::sync::{Mutex, watch};

use crate::client::{
    application_workflow::{
        ApplicationWorkflowClient, impact::MutationHints,
        participant::ApplicationMutationParticipant, policy::CommandClass,
    },
    core_lifecycle::CoreLifecycleClient,
    runtime::{CommitReceipt, Degradation, DegradationPhase, RuntimeCommitStatus},
};

#[derive(Clone)]
enum Connection {
    Pending,
    Ready(ApplicationWorkflowClient),
    #[cfg(test)]
    Isolated,
}

#[derive(Clone)]
pub(crate) struct MutationCoordinator {
    connection: watch::Sender<Connection>,
    outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
    recovery_required: Arc<Mutex<Option<String>>>,
}

impl Default for MutationCoordinator {
    fn default() -> Self {
        Self::pending()
    }
}

impl MutationCoordinator {
    pub fn pending() -> Self {
        Self {
            connection: watch::channel(Connection::Pending).0,
            outcomes: Arc::new(Mutex::new(HashMap::new())),
            recovery_required: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub fn isolated() -> Self {
        let coordinator = Self::pending();
        coordinator.connection.send_replace(Connection::Isolated);
        coordinator
    }

    pub async fn connect(&self, core: CoreLifecycleClient) -> anyhow::Result<()> {
        assert!(matches!(*self.connection.borrow(), Connection::Pending));
        let workflow = ApplicationWorkflowClient::spawn(
            core,
            self.outcomes.clone(),
            self.recovery_required.clone(),
        )
        .await?;
        self.connection.send_replace(Connection::Ready(workflow));
        Ok(())
    }

    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !matches!(*self.connection.borrow(), Connection::Pending),
            "Profile mutation workflow is not ready"
        );
        Ok(())
    }

    pub fn participant(
        &self,
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<impl FnOnce(DecisionHandle) -> StateParticipant<Profiles>> {
        let connection = self.connection.borrow().clone();
        anyhow::ensure!(
            !matches!(&connection, Connection::Pending),
            "Profile mutation workflow is not ready"
        );
        Ok(move |decision| -> StateParticipant<Profiles> {
            match connection {
                Connection::Ready(workflow) => ApplicationMutationParticipant::<Profiles>::new(
                    operation_id,
                    hints,
                    class,
                    decision,
                    workflow,
                ),
                #[cfg(test)]
                Connection::Isolated => Arc::new(IsolatedParticipant),
                Connection::Pending => unreachable!("checked before opening a source transaction"),
            }
        })
    }

    pub async fn finish(
        &self,
        operation_id: OperationId,
        domain: &str,
        source_version: u64,
    ) -> (CommitReceipt, Vec<Degradation>) {
        let connection = self.connection.borrow().clone();
        let runtime = match connection {
            #[cfg(test)]
            Connection::Isolated => RuntimeCommitStatus::Unchanged,
            Connection::Pending | Connection::Ready(_) => {
                let outcome = self.outcomes.lock().await.remove(&operation_id);
                match outcome {
                    Some(outcome) => outcome,
                    None if self.recovery_required.lock().await.is_some() => {
                        RuntimeCommitStatus::RecoveryRequired
                    }
                    None => RuntimeCommitStatus::Pending,
                }
            }
        };
        let receipt = CommitReceipt {
            operation_id: Some(operation_id.to_string()),
            domain: domain.into(),
            source_version,
            runtime,
        };
        let degradations = match receipt.runtime {
            RuntimeCommitStatus::Deferred => vec![Degradation {
                phase: DegradationPhase::RuntimeApply,
                code: "runtime_deferred".into(),
                message: "the profile was saved; runtime reconciliation was queued for retry"
                    .into(),
                retryable: true,
            }],
            RuntimeCommitStatus::RecoveryRequired => vec![Degradation {
                phase: DegradationPhase::RuntimeApply,
                code: "runtime_recovery_required".into(),
                message: self
                    .recovery_required
                    .lock()
                    .await
                    .clone()
                    .unwrap_or_else(|| "Profile runtime state requires recovery".into()),
                retryable: false,
            }],
            _ => Vec::new(),
        };
        (receipt, degradations)
    }
}

#[cfg(test)]
struct IsolatedParticipant;

#[cfg(test)]
#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> chimera_core::state::StateAckSubscriber<T>
    for IsolatedParticipant
{
    fn name(&self) -> chimera_core::state::SubscriberName<'_> {
        chimera_core::state::SubscriberName("isolated-domain-test".into())
    }

    fn ack_options(&self) -> chimera_core::state::AckOptions {
        chimera_core::state::AckOptions::required(std::time::Duration::from_secs(1))
    }

    async fn on_prepare(&self, _: chimera_core::state::StateChange<T>) -> chimera_core::state::Ack {
        chimera_core::state::Ack::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn latched_profile_recovery_is_reported_without_an_operation_outcome_entry() {
        let coordinator = MutationCoordinator::pending();
        *coordinator.recovery_required.lock().await =
            Some("failed to restore the prior Profile runtime".into());

        let (receipt, degradations) = coordinator
            .finish(OperationId::generate(), "profiles", 7)
            .await;

        assert_eq!(receipt.runtime, RuntimeCommitStatus::RecoveryRequired);
        assert_eq!(degradations.len(), 1);
        assert_eq!(degradations[0].code, "runtime_recovery_required");
        assert_eq!(
            degradations[0].message,
            "failed to restore the prior Profile runtime"
        );
    }
}

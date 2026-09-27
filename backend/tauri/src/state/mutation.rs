//! Startup-injected connection to the application mutation participant.

use std::{collections::HashMap, sync::Arc};

use chimera_config::profile::Profiles;
use chimera_core::state::{DecisionHandle, StateParticipant};
use tokio::sync::{Mutex, watch};

use crate::client::{
    application_workflow::{
        impact::MutationHints, participant::ApplicationMutationParticipant, policy::CommandClass,
    },
    core_lifecycle::CoreLifecycleClient,
    runtime::{CommitReceipt, Degradation, DegradationPhase, RuntimeCommitStatus},
};

#[derive(Clone)]
enum Connection {
    Pending,
    Ready(CoreLifecycleClient),
}

#[derive(Clone)]
pub(crate) struct MutationCoordinator {
    connection: watch::Sender<Connection>,
    outcomes: Arc<Mutex<HashMap<String, RuntimeCommitStatus>>>,
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

    pub fn connect(&self, core: CoreLifecycleClient) {
        assert!(matches!(*self.connection.borrow(), Connection::Pending));
        self.connection.send_replace(Connection::Ready(core));
    }

    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(*self.connection.borrow(), Connection::Ready(_)),
            "Profile mutation workflow is not ready"
        );
        Ok(())
    }

    pub fn participant(
        &self,
        operation_id: String,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<impl FnOnce(DecisionHandle) -> StateParticipant<Profiles>> {
        let connection = self.connection.borrow().clone();
        let Connection::Ready(core) = connection else {
            anyhow::bail!("Profile mutation workflow is not ready");
        };
        let outcomes = self.outcomes.clone();
        let recovery_required = self.recovery_required.clone();
        Ok(move |decision| {
            let participant: StateParticipant<Profiles> =
                ApplicationMutationParticipant::<Profiles>::new(
                    operation_id,
                    hints,
                    class,
                    decision,
                    core,
                    outcomes,
                    recovery_required,
                );
            participant
        })
    }

    pub async fn finish(
        &self,
        operation_id: String,
        domain: &str,
        source_version: u64,
    ) -> (CommitReceipt, Vec<Degradation>) {
        let runtime = self
            .outcomes
            .lock()
            .await
            .remove(&operation_id)
            .unwrap_or(RuntimeCommitStatus::Pending);
        let receipt = CommitReceipt {
            operation_id: Some(operation_id),
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
            _ => Vec::new(),
        };
        (receipt, degradations)
    }
}

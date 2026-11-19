//! Startup-injected connection to the application mutation participant.

mod profile_content;
pub(crate) use profile_content::{ProfileContentLock, ProfileContentReader};

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use chimera_config::profile::Profiles;
use chimera_core::state::{DecisionHandle, StateParticipant};
use chimera_core_manager::OperationId;
use tokio::sync::{Mutex, OnceCell, watch};

use crate::client::{
    application_workflow::{
        ApplicationWorkflowClient, RecoveryContext, impact::MutationHints,
        mutation::MutationReceipt, participant::ApplicationMutationParticipant,
        policy::CommandClass,
    },
    core_lifecycle::CoreLifecycleClient,
    effects::ports::CommitNotifications,
    runtime::{CommitReceipt, Degradation, DegradationPhase, RuntimeCommitStatus},
};

/// Source-owned candidate validation is independent of the Core lifecycle's
/// mutating mailbox. Its runtime check never applies the candidate.
pub(crate) struct RuntimeCandidate {
    pub profiles: Arc<Profiles>,
    pub staged: BTreeMap<String, String>,
}

#[async_trait::async_trait]
pub(crate) trait RuntimeGate: Send + Sync + 'static {
    async fn check(&self, candidate: RuntimeCandidate) -> anyhow::Result<()>;
}

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
    outcomes: Arc<Mutex<HashMap<OperationId, MutationReceipt>>>,
    recovery_required: Arc<Mutex<Option<RecoveryContext>>>,
    notifications: Option<Arc<dyn CommitNotifications>>,
    runtime_gate: Arc<OnceCell<Arc<dyn RuntimeGate>>>,
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
            notifications: None,
            runtime_gate: Arc::new(OnceCell::new()),
        }
    }

    pub fn pending_with_notifications(notifications: Arc<dyn CommitNotifications>) -> Self {
        Self {
            notifications: Some(notifications),
            ..Self::pending()
        }
    }

    #[cfg(test)]
    pub fn isolated() -> Self {
        let coordinator = Self::pending();
        coordinator.connection.send_replace(Connection::Isolated);
        coordinator
    }

    /// Production connects the checker before making Profiles writes ready.
    pub(crate) async fn connect_with_runtime_gate(
        &self,
        core: CoreLifecycleClient,
        gate: Arc<dyn RuntimeGate>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(*self.connection.borrow(), Connection::Pending),
            "Profiles mutation workflow is already connected"
        );
        self.runtime_gate
            .set(gate)
            .map_err(|_| anyhow::anyhow!("Profiles RuntimeGate was already connected"))?;
        self.connect(core).await
    }

    pub async fn connect(&self, core: CoreLifecycleClient) -> anyhow::Result<()> {
        assert!(matches!(*self.connection.borrow(), Connection::Pending));
        let workflow = ApplicationWorkflowClient::spawn(
            core,
            self.outcomes.clone(),
            self.recovery_required.clone(),
            self.notifications.clone(),
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

    pub(crate) fn runtime_gate(&self) -> anyhow::Result<Arc<dyn RuntimeGate>> {
        match self.connection.borrow().clone() {
            Connection::Pending => anyhow::bail!("Profiles RuntimeGate is not ready"),
            #[cfg(test)]
            Connection::Isolated => Ok(self
                .runtime_gate
                .get()
                .cloned()
                .unwrap_or_else(|| Arc::new(IsolatedRuntimeGate))),
            Connection::Ready(_) => self
                .runtime_gate
                .get()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Profiles RuntimeGate is unavailable")),
        }
    }

    #[cfg(test)]
    pub(crate) fn install_test_runtime_gate(&self, gate: Arc<dyn RuntimeGate>) {
        assert!(matches!(*self.connection.borrow(), Connection::Isolated));
        assert!(self.runtime_gate.set(gate).is_ok());
    }

    pub fn participant(
        &self,
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<impl FnOnce(DecisionHandle) -> StateParticipant<Profiles> + use<>> {
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

    /// Ref Profiles source-only path: after an ordinary versioned document
    /// commit, publish its version without creating a TCC participant or
    /// attempting to start/reconcile the Core. The domain actor has already
    /// proved that no runtime target or explicit activation was requested.
    pub(crate) fn committed_without_runtime(&self, source_version: u64) -> CommitReceipt {
        match self.connection.borrow().clone() {
            Connection::Ready(workflow) => workflow.profiles_source_committed(source_version),
            #[cfg(test)]
            Connection::Isolated => {}
            Connection::Pending => {
                tracing::error!("Profiles source commit was published before mutation readiness");
            }
        }
        CommitReceipt {
            operation_id: None,
            domain: "profiles".into(),
            source_version,
            runtime: RuntimeCommitStatus::Unchanged,
        }
    }

    /// A source-only Profiles file commit still publishes its changed content
    /// to the shared UI/effects owner after the durable source CAS.
    pub(crate) fn profiles_committed(&self, files: Vec<chimera_config::profile::ProfileId>) {
        if let Some(notifications) = &self.notifications {
            notifications.profiles_committed(files);
        }
    }

    pub async fn finish(
        &self,
        operation_id: OperationId,
        domain: &str,
        source_version: u64,
    ) -> (CommitReceipt, Vec<Degradation>) {
        let connection = self.connection.borrow().clone();
        let committed_workflow = match &connection {
            Connection::Ready(workflow) => Some(workflow.clone()),
            Connection::Pending => None,
            #[cfg(test)]
            Connection::Isolated => None,
        };
        let (runtime, mut degradations) = match connection {
            #[cfg(test)]
            Connection::Isolated => (RuntimeCommitStatus::Unchanged, Vec::new()),
            Connection::Pending | Connection::Ready(_) => {
                let outcome = self.outcomes.lock().await.remove(&operation_id);
                match outcome {
                    Some(outcome) => (outcome.runtime_status(), outcome.degradations),
                    None if self.recovery_required.lock().await.is_some() => {
                        (RuntimeCommitStatus::RecoveryRequired, Vec::new())
                    }
                    None => (RuntimeCommitStatus::Pending, Vec::new()),
                }
            }
        };
        let receipt = CommitReceipt {
            operation_id: Some(operation_id.to_string()),
            domain: domain.into(),
            source_version,
            runtime,
        };
        // Called only after the Profiles source actor committed its document
        // and finished its participant decision. The participant still owns
        // retry policy until its runtime mutation path is migrated to ref.
        if domain == "profiles"
            && let Some(workflow) = committed_workflow
        {
            workflow.profiles_source_committed(source_version);
        }
        if degradations.is_empty() {
            degradations = match receipt.runtime {
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
                        .map(|context| context.error)
                        .unwrap_or_else(|| "Profile runtime state requires recovery".into()),
                    retryable: false,
                }],
                _ => Vec::new(),
            };
        }
        (receipt, degradations)
    }
}

#[cfg(test)]
struct IsolatedRuntimeGate;

#[cfg(test)]
#[async_trait::async_trait]
impl RuntimeGate for IsolatedRuntimeGate {
    async fn check(&self, _: RuntimeCandidate) -> anyhow::Result<()> {
        Ok(())
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
        *coordinator.recovery_required.lock().await = Some(RecoveryContext {
            operation_id: None,
            runtime_operation: None,
            domain: crate::client::application_workflow::mutation::ConfigDomain::Profiles,
            stage: crate::client::application_workflow::mutation::MutationStage::TryingCritical,
            decision: None,
            baseline: None,
            target: None,
            error: "failed to restore the prior Profile runtime".into(),
        });

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

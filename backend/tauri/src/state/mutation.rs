//! Startup-injected Profiles candidate checks and committed-source notifications.

mod change;
pub(crate) use change::DomainChange;
pub(crate) mod impact;
mod ports;
pub(crate) use ports::{RuntimeCandidate, RuntimeGate};
mod profile_content;
pub(crate) use profile_content::{ProfileContentLock, ProfileContentReader};

use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
use chimera_config::profile::Profiles;
use chimera_contracts::{
    mutation::ConfigDomain,
    runtime_error::{RuntimeCheckUnavailable, RuntimeError},
};
use chimera_core::state::Version;
#[cfg(test)]
use chimera_core::state::{DecisionHandle, StateParticipant};
use chimera_core_manager::OperationId;
use tokio::sync::{Mutex, OnceCell, watch};

use crate::client::{
    application_workflow::{ApplicationWorkflowClient, RecoveryContext, mutation::MutationReceipt},
    core_lifecycle::CoreLifecycleClient,
    effects::ports::CommitNotifications,
    runtime::{CommitReceipt, RuntimeCommitStatus},
};

#[cfg(test)]
use crate::client::{
    application_workflow::{
        impact::MutationHints, participant::ApplicationMutationParticipant, policy::CommandClass,
    },
    runtime::{Degradation, DegradationPhase},
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

    pub(crate) fn runtime_gate(&self) -> Result<Arc<dyn RuntimeGate>, Arc<RuntimeError>> {
        match self.connection.borrow().clone() {
            Connection::Pending => Err(Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::NotChecked {
                    reason: "Profiles RuntimeGate is not ready".into(),
                },
            })),
            #[cfg(test)]
            Connection::Isolated => Ok(self
                .runtime_gate
                .get()
                .cloned()
                .unwrap_or_else(|| Arc::new(IsolatedRuntimeGate))),
            Connection::Ready(_) => self.runtime_gate.get().cloned().ok_or_else(|| {
                Arc::new(RuntimeError::CheckUnavailable {
                    reason: RuntimeCheckUnavailable::NotChecked {
                        reason: "Profiles RuntimeGate is unavailable".into(),
                    },
                })
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn install_test_runtime_gate(&self, gate: Arc<dyn RuntimeGate>) {
        assert!(matches!(*self.connection.borrow(), Connection::Isolated));
        assert!(self.runtime_gate.set(gate).is_ok());
    }

    /// Retained only for legacy transaction migration regression fixtures.
    /// Profiles production writes never enlist this participant.
    #[cfg(test)]
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

    /// A completed source owns its own commit notification and runtime hint.
    /// The runtime still reads the authoritative latest committed source, not
    /// the candidate supplied by the source actor before persistence.
    pub(crate) fn source_committed(
        &self,
        source_version: u64,
        reaches_runtime: bool,
        operation: Option<OperationId>,
        files: Vec<chimera_config::profile::ProfileId>,
    ) -> CommitReceipt {
        if let Some(notifications) = &self.notifications {
            notifications.profiles_committed(files);
        }
        match self.connection.borrow().clone() {
            Connection::Ready(workflow) => {
                if reaches_runtime {
                    // Ref RuntimeGate::committed observes the durable version.
                    // Legacy test composition roots may connect without a gate.
                    if let Some(gate) = self.runtime_gate.get() {
                        gate.committed(ConfigDomain::Profiles, Version::new(source_version));
                    } else {
                        workflow.profiles_runtime_committed(source_version);
                    }
                } else {
                    workflow.profiles_source_committed(source_version);
                }
            }
            #[cfg(test)]
            Connection::Isolated => {
                if reaches_runtime && let Some(gate) = self.runtime_gate.get() {
                    gate.committed(ConfigDomain::Profiles, Version::new(source_version));
                }
            }
            Connection::Pending => {
                tracing::error!("Profiles source committed before mutation readiness");
            }
        }
        CommitReceipt {
            operation_id: operation.map(|id| id.to_string()),
            domain: "profiles".into(),
            source_version,
            runtime: RuntimeCommitStatus::Unchanged,
        }
    }

    /// Compatibility readout for transaction participant regression tests.
    #[cfg(test)]
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
    async fn check(&self, _: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
        Ok(())
    }

    fn committed(&self, _: ConfigDomain, _: Version) {}
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
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct RecordingRuntimeGate {
        checked: StdMutex<usize>,
        committed: StdMutex<Vec<(ConfigDomain, Version)>>,
    }

    #[async_trait::async_trait]
    impl RuntimeGate for RecordingRuntimeGate {
        async fn check(&self, candidate: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
            assert!(matches!(candidate.change, DomainChange::Profiles { .. }));
            *self.checked.lock().unwrap() += 1;
            Ok(())
        }

        fn committed(&self, domain: ConfigDomain, version: Version) {
            self.committed.lock().unwrap().push((domain, version));
        }
    }

    #[tokio::test]
    async fn runtime_gate_observes_only_runtime_affecting_committed_profiles_version() {
        let coordinator = MutationCoordinator::isolated();
        let gate = Arc::new(RecordingRuntimeGate::default());
        coordinator.install_test_runtime_gate(gate.clone());

        coordinator
            .runtime_gate()
            .unwrap()
            .check(RuntimeCandidate {
                change: DomainChange::Profiles {
                    candidate: Arc::new(chimera_config::profile::Profiles::default()),
                },
                staged: Default::default(),
            })
            .await
            .unwrap();
        assert_eq!(*gate.checked.lock().unwrap(), 1);
        assert!(gate.committed.lock().unwrap().is_empty());

        coordinator.source_committed(3, false, None, Vec::new());
        assert!(gate.committed.lock().unwrap().is_empty());

        coordinator.source_committed(4, true, None, Vec::new());
        assert_eq!(
            *gate.committed.lock().unwrap(),
            vec![(ConfigDomain::Profiles, Version::new(4))]
        );
    }

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

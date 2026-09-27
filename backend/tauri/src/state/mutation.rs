//! Startup-injected Profile transaction participant.
//!
//! The domain actor owns the document transaction. This coordinator only
//! admits its candidate to the serialized core lifecycle and compensates the
//! previous Profile snapshot if the source transaction rolls back.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chimera_config::profile::Profiles;
use chimera_core::state::{
    Ack, AckOptions, DecisionHandle, StateAckSubscriber, StateChange, StateDecision,
    StateParticipant, SubscriberName,
};
use tokio::sync::{Mutex, watch};

use crate::client::{
    application_workflow::{
        impact::{ActivationIntent, MutationHints, RuntimeImpact, classify_profiles},
        policy::{CommandClass, CommandPolicy, CoreRunIntent, policy_for},
    },
    core_lifecycle::CoreLifecycleClient,
    runtime::{CommitReceipt, Degradation, RuntimeCommitStatus},
};

const MUTATION_ACK_TIMEOUT: Duration = Duration::from_secs(90);

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
            let participant: StateParticipant<Profiles> = Arc::new(ProfileMutationParticipant {
                name: format!("profiles-mutation/{operation_id}"),
                operation_id,
                hints,
                class,
                core,
                outcomes,
                recovery_required,
                decision,
                attempted_runtime: std::sync::atomic::AtomicBool::new(false),
            });
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
        (receipt, Vec::new())
    }
}

struct ProfileMutationParticipant {
    name: String,
    operation_id: String,
    hints: MutationHints,
    class: CommandClass,
    core: CoreLifecycleClient,
    outcomes: Arc<Mutex<HashMap<String, RuntimeCommitStatus>>>,
    recovery_required: Arc<Mutex<Option<String>>>,
    decision: DecisionHandle,
    attempted_runtime: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl StateAckSubscriber<Profiles> for ProfileMutationParticipant {
    fn name(&self) -> SubscriberName<'_> {
        SubscriberName(self.name.as_str().into())
    }

    fn ack_options(&self) -> AckOptions {
        AckOptions::required(MUTATION_ACK_TIMEOUT)
    }

    async fn on_prepare(&self, change: StateChange<Profiles>) -> Ack {
        if let Some(message) = self.recovery_required.lock().await.clone() {
            return Ack::Failed(anyhow::anyhow!(
                "Profile writes require runtime recovery first: {message}"
            ));
        }
        if self.decision.decision() != StateDecision::Undecided {
            return Ack::Rejected(
                "Profile source transaction settled before runtime admission".into(),
            );
        }

        let previous = change
            .previous
            .as_ref()
            .map(|state| state.state.clone())
            .unwrap_or_default();
        let candidate = change.current.as_ref().clone();
        let impact = classify_profiles(&previous, &candidate, &self.hints);
        let explicit_switch = self.class == CommandClass::ExplicitSwitch
            || self.hints.activation != ActivationIntent::None;

        if !explicit_switch && impact == RuntimeImpact::None {
            self.outcomes
                .lock()
                .await
                .insert(self.operation_id.clone(), RuntimeCommitStatus::Unchanged);
            return Ack::Ok;
        }

        let status = match self.core.core_status().await {
            Ok(status) => status,
            Err(error) => return Ack::Failed(error),
        };
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            self.outcomes.lock().await.insert(
                self.operation_id.clone(),
                RuntimeCommitStatus::SavedInactive,
            );
            return Ack::Ok;
        }

        let policy = policy_for(
            self.class,
            impact,
            &Default::default(),
            CoreRunIntent::Running,
        );
        if matches!(
            policy,
            CommandPolicy::SaveOnly | CommandPolicy::SaveThenNotify
        ) {
            self.outcomes
                .lock()
                .await
                .insert(self.operation_id.clone(), RuntimeCommitStatus::Unchanged);
            return Ack::Ok;
        }

        self.attempted_runtime
            .store(true, std::sync::atomic::Ordering::Release);
        match self
            .core
            .reconcile_profiles(Arc::new(candidate), self.hints.staged_content.clone())
            .await
        {
            Ok(()) => {
                self.outcomes
                    .lock()
                    .await
                    .insert(self.operation_id.clone(), RuntimeCommitStatus::Applied);
                Ack::Ok
            }
            Err(error) => Ack::Failed(error),
        }
    }

    async fn on_rolled_back(
        &self,
        change: StateChange<Profiles>,
        _reason: chimera_core::state::RollbackReason,
    ) {
        if !self
            .attempted_runtime
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.outcomes.lock().await.remove(&self.operation_id);
            return;
        }

        let Some(previous) = change.previous.map(|state| Arc::new(state.state.clone())) else {
            *self.recovery_required.lock().await =
                Some("no previous Profile snapshot is available for rollback".into());
            return;
        };
        if let Err(error) = self
            .core
            .reconcile_profiles(previous, Default::default())
            .await
        {
            let message = format!("failed to restore the previous Profile runtime: {error}");
            *self.recovery_required.lock().await = Some(message);
        }
        self.outcomes.lock().await.remove(&self.operation_id);
    }
}

//! The per-mutation Required participant of a Profile source transaction.
//!
//! The source transaction owns persistence and the authoritative decision.
//! The tracked application workflow owns the runtime attempt and its
//! confirmation or compensation.

use std::{marker::PhantomData, sync::Arc, time::Duration};

use chimera_core::state::{
    Ack, AckOptions, DecisionHandle, StateAckSubscriber, StateChange, SubscriberName,
};
use chimera_core_manager::OperationId;
use tokio::sync::oneshot;

use super::{
    ApplicationWorkflowClient,
    impact::MutationHints,
    mutation::{MutationDomain, MutationRequest},
    policy::CommandClass,
};

const MUTATION_ACK_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) struct ApplicationMutationParticipant<T: MutationDomain> {
    operation_id: OperationId,
    hints: MutationHints,
    class: CommandClass,
    decision: DecisionHandle,
    workflow: ApplicationWorkflowClient,
    completion: std::sync::Mutex<Option<oneshot::Receiver<Result<(), String>>>>,
    ack_timeout: Duration,
    name: String,
    _state: PhantomData<T>,
}

impl<T: MutationDomain> ApplicationMutationParticipant<T> {
    pub(crate) fn new(
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        decision: DecisionHandle,
        workflow: ApplicationWorkflowClient,
    ) -> Arc<Self> {
        Self::with_ack_timeout(
            operation_id,
            hints,
            class,
            decision,
            workflow,
            MUTATION_ACK_TIMEOUT,
        )
    }

    pub(crate) fn with_ack_timeout(
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        decision: DecisionHandle,
        workflow: ApplicationWorkflowClient,
        ack_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            name: format!("application-mutation/{operation_id}"),
            operation_id,
            hints,
            class,
            decision,
            workflow,
            completion: std::sync::Mutex::new(None),
            ack_timeout,
            _state: PhantomData,
        })
    }

    fn take_completion(&self) -> Option<oneshot::Receiver<Result<(), String>>> {
        self.completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    async fn await_completion(&self) -> Result<(), String> {
        let Some(completion) = self.take_completion() else {
            return Err(format!(
                "application workflow completion for operation {} is unavailable",
                self.operation_id
            ));
        };
        completion.await.map_err(|_| {
            format!(
                "application workflow abandoned operation {}; its runtime outcome is unknown",
                self.operation_id
            )
        })?
    }
}

#[async_trait::async_trait]
impl<T: MutationDomain> StateAckSubscriber<T> for ApplicationMutationParticipant<T> {
    fn name(&self) -> SubscriberName<'_> {
        SubscriberName(std::borrow::Cow::Borrowed(&self.name))
    }

    fn ack_options(&self) -> AckOptions {
        AckOptions::required(self.ack_timeout)
    }

    async fn on_prepare(&self, change: StateChange<T>) -> Ack {
        let (ack, verdict) = oneshot::channel();
        let (completion, settled) = oneshot::channel();
        *self
            .completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(settled);

        if let Err(error) = self.workflow.begin_mutation(MutationRequest {
            operation_id: self.operation_id.clone(),
            change: T::domain_change(change),
            hints: self.hints.clone(),
            class: self.class,
            decision: self.decision.clone(),
            ack: Some(ack),
            completion: Some(completion),
        }) {
            return Ack::Failed(error);
        }

        match verdict.await {
            Ok(ack) => ack.into(),
            Err(_) => Ack::Failed(anyhow::anyhow!(
                "the application workflow abandoned the verdict of operation {}; its outcome is unknown",
                self.operation_id
            )),
        }
    }

    async fn on_committed(&self, _change: StateChange<T>) -> Ack {
        match self.await_completion().await {
            Ok(()) => Ack::Ok,
            Err(error) => Ack::Failed(anyhow::anyhow!(error)),
        }
    }

    async fn on_rolled_back(
        &self,
        _change: StateChange<T>,
        _reason: chimera_core::state::RollbackReason,
    ) {
        if let Err(error) = self.await_completion().await {
            tracing::warn!(%error, operation_id = %self.operation_id, "Profile mutation cancellation did not settle cleanly");
        }
    }
}

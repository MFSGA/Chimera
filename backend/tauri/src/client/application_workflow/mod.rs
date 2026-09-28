//! Application mutation coordination and Profile workflow modules.

use std::{collections::HashMap, panic::AssertUnwindSafe, sync::Arc};

use anyhow::Context as _;
use chimera_core_manager::OperationId;
use futures_util::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use tokio::sync::Mutex;

use crate::client::{core_lifecycle::CoreLifecycleClient, runtime::RuntimeCommitStatus};

use workflow::ApplicationWorkflow;

pub(crate) mod impact;
pub(in crate::client) mod inputs;
pub(crate) mod mutation;
pub(crate) mod participant;
pub(crate) mod policy;
pub(in crate::client) mod profiles;
mod tcc;
mod workflow;

use mutation::{MutationRequest, TryAck};

enum Message {
    BeginMutation(Box<MutationRequest>),
}

struct ApplicationWorkflowActor;

impl Actor for ApplicationWorkflowActor {
    type Msg = Message;
    type State = ApplicationWorkflow;
    type Arguments = ApplicationWorkflow;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        workflow: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(workflow)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        workflow: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::BeginMutation(mut request) => {
                let operation_id = request.operation_id.clone();
                if AssertUnwindSafe(workflow.run_mutation(&mut request))
                    .catch_unwind()
                    .await
                    .is_err()
                {
                    let message = format!(
                        "application workflow operation {operation_id} panicked; runtime outcome requires recovery"
                    );
                    workflow.mark_recovery_required(message.clone()).await;
                    request.answer(TryAck::Failed(message.clone()));
                    let _ = tokio::time::timeout(
                        tcc::MUTATION_DECISION_TIMEOUT,
                        request.decision.wait(),
                    )
                    .await;
                    request.settle(Err(message));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct ApplicationWorkflowClient(ActorRef<Message>);

impl ApplicationWorkflowClient {
    pub(crate) async fn spawn(
        core: CoreLifecycleClient,
        outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
        recovery_required: Arc<Mutex<Option<String>>>,
    ) -> anyhow::Result<Self> {
        let workflow = ApplicationWorkflow::new(core, outcomes, recovery_required);
        Self::spawn_workflow(workflow).await
    }

    #[cfg(test)]
    pub(super) async fn spawn_with_runtime(
        runtime: Arc<dyn workflow::ProfileRuntime>,
        outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
        recovery_required: Arc<Mutex<Option<String>>>,
    ) -> anyhow::Result<Self> {
        let workflow = ApplicationWorkflow::with_runtime(runtime, outcomes, recovery_required);
        Self::spawn_workflow(workflow).await
    }

    async fn spawn_workflow(workflow: ApplicationWorkflow) -> anyhow::Result<Self> {
        let (actor_ref, _actor_handle) =
            Actor::spawn(None, ApplicationWorkflowActor, workflow).await?;
        Ok(Self(actor_ref))
    }

    pub(crate) fn begin_mutation(&self, request: MutationRequest) -> anyhow::Result<()> {
        self.0
            .cast(Message::BeginMutation(Box::new(request)))
            .context("application workflow actor is unavailable")
    }
}

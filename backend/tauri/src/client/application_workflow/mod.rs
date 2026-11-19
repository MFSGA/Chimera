//! Application mutation coordination and Profile workflow modules.

use std::{collections::HashMap, sync::Arc};

use chimera_core_manager::OperationId;
use tokio::sync::Mutex;

use crate::client::core_lifecycle::CoreLifecycleClient;

use mutation::MutationRequest;
use workflow::ApplicationWorkflow;
pub(crate) use workflow::RecoveryContext;

pub(crate) mod attempt;
pub(crate) mod impact;
pub(in crate::client) mod inputs;
pub(crate) mod mutation;
pub(crate) mod participant;
pub(crate) mod policy;
pub(crate) mod ports;
pub(in crate::client) mod profiles;
pub(crate) mod startup;
pub(in crate::client) mod tcc;
pub(in crate::client) mod workflow;

#[derive(Clone)]
enum ApplicationWorkflowClientInner {
    CoreLifecycle(CoreLifecycleClient),
    #[cfg(test)]
    TestActor(ractor::ActorRef<TestMessage>),
}

#[derive(Clone)]
pub(crate) struct ApplicationWorkflowClient(ApplicationWorkflowClientInner);

impl ApplicationWorkflowClient {
    pub(crate) async fn spawn(
        core: CoreLifecycleClient,
        outcomes: Arc<Mutex<HashMap<OperationId, mutation::MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<workflow::RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    ) -> anyhow::Result<Self> {
        Self::spawn_inner(core, outcomes, recovery_required, notifications, None).await
    }

    #[cfg(test)]
    pub(super) async fn spawn_with_registration(
        core: CoreLifecycleClient,
        outcomes: Arc<Mutex<HashMap<OperationId, mutation::MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<workflow::RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
        registered: tokio::sync::oneshot::Sender<()>,
    ) -> anyhow::Result<Self> {
        Self::spawn_inner(
            core,
            outcomes,
            recovery_required,
            notifications,
            Some(registered),
        )
        .await
    }

    async fn spawn_inner(
        core: CoreLifecycleClient,
        outcomes: Arc<Mutex<HashMap<OperationId, mutation::MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<workflow::RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
        registered: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> anyhow::Result<Self> {
        core.connect_application_workflow_with_registration(
            ApplicationWorkflow::new(outcomes, recovery_required, notifications),
            registered,
        )
        .await?;
        Ok(Self(ApplicationWorkflowClientInner::CoreLifecycle(core)))
    }

    #[cfg(test)]
    pub(super) async fn spawn_with_runtime_and_notifications(
        runtime: Arc<dyn workflow::ProfileRuntime>,
        outcomes: Arc<Mutex<HashMap<OperationId, mutation::MutationReceipt>>>,
        recovery_required: Arc<Mutex<Option<workflow::RecoveryContext>>>,
        notifications: Option<Arc<dyn crate::client::effects::ports::CommitNotifications>>,
    ) -> anyhow::Result<Self> {
        use ractor::Actor;

        let (actor_ref, _actor_handle) = Actor::spawn(
            None,
            TestApplicationWorkflowActor,
            TestApplicationWorkflowState {
                workflow: ApplicationWorkflow::new(outcomes, recovery_required, notifications),
                runtime,
            },
        )
        .await?;
        Ok(Self(ApplicationWorkflowClientInner::TestActor(actor_ref)))
    }

    /// The legacy Profiles participant still owns its transaction, but the
    /// durable version is also observed by the one lifecycle mailbox.
    /// Profile runtime retries are already scheduled by the participant;
    /// notifying here must not schedule a duplicate reconcile.
    pub(crate) fn profiles_source_committed(&self, version: u64) {
        match &self.0 {
            ApplicationWorkflowClientInner::CoreLifecycle(core) => core.source_committed(
                crate::client::reconciler::committed::SourceDomain::Profiles,
                version,
                false,
            ),
            #[cfg(test)]
            ApplicationWorkflowClientInner::TestActor(_) => {}
        }
    }

    pub(crate) fn begin_mutation(&self, request: MutationRequest) -> anyhow::Result<()> {
        match &self.0 {
            ApplicationWorkflowClientInner::CoreLifecycle(core) => {
                core.begin_profile_mutation(request)
            }
            #[cfg(test)]
            ApplicationWorkflowClientInner::TestActor(actor) => actor
                .cast(TestMessage::BeginMutation(Box::new(request)))
                .map_err(|error| {
                    anyhow::anyhow!("application workflow actor is unavailable: {error}")
                }),
        }
    }
}

#[cfg(test)]
enum TestMessage {
    BeginMutation(Box<MutationRequest>),
}

#[cfg(test)]
struct TestApplicationWorkflowActor;

#[cfg(test)]
struct TestApplicationWorkflowState {
    workflow: ApplicationWorkflow,
    runtime: Arc<dyn workflow::ProfileRuntime>,
}

#[cfg(test)]
impl ractor::Actor for TestApplicationWorkflowActor {
    type Msg = TestMessage;
    type State = TestApplicationWorkflowState;
    type Arguments = TestApplicationWorkflowState;

    async fn pre_start(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        state: Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        Ok(state)
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match message {
            TestMessage::BeginMutation(mut request) => {
                let operation_id = request.operation_id.clone();
                match std::panic::AssertUnwindSafe(
                    state
                        .workflow
                        .run_mutation(&mut request, state.runtime.as_ref()),
                )
                .catch_unwind()
                .await
                {
                    Ok((rebuild, _)) if rebuild => state.runtime.request_runtime_rebuild(),
                    Ok(_) => {}
                    Err(_) => {
                        let message = format!(
                            "application workflow operation {operation_id} panicked; runtime outcome requires recovery"
                        );
                        state.workflow.mark_recovery_required(message.clone()).await;
                        request.answer(mutation::TryAck::Failed(message.clone()));
                        let _ = tokio::time::timeout(
                            tcc::MUTATION_DECISION_TIMEOUT,
                            request.decision.wait(),
                        )
                        .await;
                        request.settle(Err(message));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
use futures_util::FutureExt;

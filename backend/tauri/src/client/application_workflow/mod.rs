//! Application mutation coordination and Profile workflow modules.

#[cfg(test)]
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
use chimera_core_manager::OperationId;
#[cfg(test)]
use tokio::sync::Mutex;

#[cfg(test)]
use crate::client::core_lifecycle::CoreLifecycleClient;

#[cfg(test)]
use mutation::MutationRequest;
#[cfg(test)]
use workflow::ApplicationWorkflow;
pub(crate) use workflow::RecoveryContext;

pub(crate) mod attempt;
pub(crate) mod impact;
pub(in crate::client) mod inputs;
pub(crate) mod mutation;
#[cfg(test)]
pub(crate) mod participant;
pub(crate) mod policy;
pub(crate) mod ports;
pub(in crate::client) mod profiles;
pub(crate) mod startup;
pub(in crate::client) mod tcc;
pub(in crate::client) mod workflow;

#[cfg(test)]
#[derive(Clone)]
enum ApplicationWorkflowClientInner {
    CoreLifecycle(CoreLifecycleClient),
    #[cfg(test)]
    TestActor(ractor::ActorRef<TestMessage>),
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ApplicationWorkflowClient(ApplicationWorkflowClientInner);

#[cfg(test)]
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

    /// Source-only changes publish a version but do not request a runtime round.
    pub(crate) fn profiles_source_committed(&self, version: u64) {
        self.profiles_committed(version, false);
    }

    /// A runtime-affecting Profiles source commit asks the *existing*
    /// CoreLifecycle mailbox to reconcile the latest committed target.
    /// No candidate or pre-commit transaction participant applies the Core.
    pub(crate) fn profiles_runtime_committed(&self, version: u64) {
        self.profiles_committed(version, true);
    }

    fn profiles_committed(&self, version: u64, reconcile: bool) {
        match &self.0 {
            ApplicationWorkflowClientInner::CoreLifecycle(core) => core.source_committed(
                crate::client::reconciler::committed::SourceDomain::Profiles,
                version,
                reconcile,
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
                match chimera_panics::catch_recoverable_future(
                    state
                        .workflow
                        .run_mutation(&mut request, state.runtime.as_ref()),
                )
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

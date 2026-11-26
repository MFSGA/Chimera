//! Runtime application and lifecycle operations share one actor mailbox.
//! The desktop compatibility client is re-exported by core_lifecycle.
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
    time::Duration,
};

#[cfg(test)]
use crate::client::reconciler::apply::checked_mode_effect_can_finish;
#[cfg(test)]
use crate::client::reconciler::checked::{
    CheckedRuntimeCandidate, CheckedRuntimeInbox, CheckedSelection, MAX_CHECKED_PENDING,
};
#[cfg(test)]
use crate::client::reconciler::committed::SourceVersions;
use crate::client::reconciler::{committed::SourceDomain, status::ReconcilerStatus};
use chimera_core::client::reconciler::worker::{WorkerEvidence, WorkerWorkflow};
#[cfg(test)]
use chimera_core::client::reconciler::{
    completion::checked_clash_notice, plan::RunIntent, workflow::RuntimeReconcilerState,
};
#[cfg(test)]
use chimera_core::client::reconciler::{
    completion::refresh_effect_ports,
    worker::{WorkerResult, run_lifecycle},
};
use ractor::Actor;
#[cfg(test)]
use ractor::{ActorProcessingErr, rpc::CallResult};
#[cfg(test)]
use std::sync::atomic::Ordering;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[cfg(test)]
use crate::client::application_workflow::{
    startup::{StartupOutcome, StartupWorkflowState},
    workflow::ProfileRuntime,
};
#[cfg(test)]
use crate::client::core_lifecycle::{
    BinaryInstallProgress, LegacyServiceBridge, ServiceTransitionLease,
};
use crate::client::core_lifecycle::{
    Command, CoreLifecyclePort, CoreLifecycleWorkflow, CoreStatusSnapshot, PreparedCoreBinary,
    RuntimeTransformDiagnostics, ServiceLifecyclePort, ports, workflow,
};
use crate::client::{
    ChimeraClient, application::ApplicationClient, application_workflow::startup::StartupReport,
    application_workflow::workflow::ApplicationWorkflow, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};

#[cfg(test)]
use chimera_core::client::reconciler::{
    dispatch::is_source_only_application_patch,
    mailbox::{MAX_PENDING, OperationId, Response},
    service_evidence::{service_followup_evidence_changed, startup_recovery_evidence_changed},
};

#[cfg(test)]
use chimera_core::client::core_lifecycle::RECOVERY_INTERVAL;
use chimera_core::client::reconciler::worker::CALL_WAIT;

#[cfg(test)]
use chimera_core::client::reconciler::mailbox::{DIRTY_WINDOW, DirtyTick, Mailbox, MailboxTimers};

pub use chimera_core::client::app_lifecycle::CompletedLifecycleOperation;

fn completed_from_startup_report(report: StartupReport) -> CompletedLifecycleOperation {
    let result = report
        .outcome
        .failure_reason()
        .map_or(Ok(()), |reason| Err(anyhow::anyhow!(reason.to_owned())));
    CompletedLifecycleOperation { result, report }
}

#[async_trait::async_trait]
impl WorkerWorkflow<Command> for ApplicationWorkflow {
    async fn execute_lifecycle_command(&mut self, command: Command) -> anyhow::Result<bool> {
        self.execute_lifecycle(command).await
    }

    async fn recover_worker_panic(&mut self, message: String) {
        self.mark_recovery_required(message).await;
    }

    fn observe_worker(&self) -> WorkerEvidence {
        WorkerEvidence {
            lower_outcome_uncertain: self.lifecycle().outcome_uncertain(),
            startup_owner_unproven: self.startup_owner_unproven(),
            service_restart_policy: self.lifecycle().service_restart_policy(),
        }
    }

    async fn probe_worker_service(
        &self,
    ) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.lifecycle().probe_service().await
    }

    #[cfg(test)]
    async fn run_command(&mut self, command: Command) -> WorkerResult {
        match command {
            Command::ProfileMutation(mut mutation) => {
                // Ref's recoverable panic flag must cover each future
                // poll, not remain set across a Tokio .await. Preserve
                // the uncertainty/recovery settlement on panic.
                let run = chimera_panics::catch_recoverable_future(
                    self.run_profile_mutation(&mut mutation),
                )
                .await;
                let (result, panicked, rebuild) = match run {
                    Ok((request_runtime_rebuild, settlement)) => (
                        settlement.map_err(anyhow::Error::msg),
                        false,
                        request_runtime_rebuild,
                    ),
                    Err(_) => {
                        let message = format!(
                            "application workflow operation {} panicked; runtime outcome requires recovery",
                            mutation.operation_id
                        );
                        self.mark_recovery_required(message.clone()).await;
                        mutation.answer(
                            crate::client::application_workflow::mutation::TryAck::Failed(
                                message.clone(),
                            ),
                        );
                        let _ = tokio::time::timeout(
                            crate::client::application_workflow::tcc::MUTATION_DECISION_TIMEOUT,
                            mutation.decision.wait(),
                        )
                        .await;
                        mutation.settle(Err(message.clone()));
                        (Err::<(), _>(anyhow::anyhow!(message)), true, false)
                    }
                };
                WorkerResult {
                    result,
                    workflow_panicked: panicked,
                    request_runtime_rebuild: rebuild,
                }
            }
            command => run_lifecycle(self, command).await,
        }
    }
}

pub use chimera_core::client::reconciler::mailbox::{
    CoreLifecycleOperationResult, CoreLifecycleStatus,
};

#[cfg(test)]
use chimera_core::actor_v2::service_policy::ServicePhase;
#[cfg(test)]
use chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot;
use chimera_core::actor_v2::service_status::{ServiceHostStatus, ServiceHostWatch};

// OS-specific runtime ownership evidence enters the core through an input.
#[cfg(test)]
fn classify_service_probe(info: chimera_ipc::types::StatusInfo<'static>) -> ServiceHostStatus {
    let owned = crate::core::service::is_service_runtime_owned(&info);
    ServiceHostStatus::from_probe(info, owned)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
type ProxyHandle = crate::client::system_proxy::SystemProxyClient;
#[cfg(any(target_os = "android", target_os = "ios"))]
type ProxyHandle = ();
type Message = chimera_core::client::reconciler::client::Message<ApplicationWorkflow, ProxyHandle>;
type ActorClient =
    chimera_core::client::reconciler::client::ActorClient<ApplicationWorkflow, ProxyHandle>;

type RuntimeReconcilerMailbox =
    chimera_core::client::reconciler::actor_state::RuntimeReconcilerMailbox<
        ApplicationWorkflow,
        ProxyHandle,
    >;

#[async_trait::async_trait]
impl chimera_core::client::reconciler::actor_state::MailboxWorkflow<ProxyHandle>
    for ApplicationWorkflow
{
    type Lifecycle = CoreLifecycleWorkflow;
    fn lifecycle(&self) -> &Self::Lifecycle {
        ApplicationWorkflow::lifecycle(self)
    }
    fn progress(&self) -> &chimera_core::client::reconciler::workflow_progress::WorkflowProgress {
        ApplicationWorkflow::progress(self)
    }
    fn progress_mut(
        &mut self,
    ) -> &mut chimera_core::client::reconciler::workflow_progress::WorkflowProgress {
        ApplicationWorkflow::progress_mut(self)
    }
    fn recovery_command_allowed(&self, command: &Command) -> bool {
        ApplicationWorkflow::recovery_command_allowed(self, command)
    }
    fn legacy_runtime_owned(info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }
    async fn service_release_observation(
        &self,
    ) -> chimera_core::client::reconciler::release::ReleaseObservation {
        self.lifecycle().service_release_observation().await
    }
    fn reject_workflow_command(command: &mut Command, error: &anyhow::Error) {
        #[cfg(test)]
        if let Command::ProfileMutation(mutation) = command {
            let message = error.to_string();
            mutation.answer(
                crate::client::application_workflow::mutation::TryAck::Failed(message.clone()),
            );
            mutation.settle(Err(message));
        }
        #[cfg(not(test))]
        let _ = (command, error);
    }
    fn admit_workflow_command(&self, command: &Command) -> anyhow::Result<()> {
        #[cfg(test)]
        if matches!(command, Command::ProfileMutation(_)) && !self.mutation_connected() {
            anyhow::bail!("application workflow is not connected");
        }
        #[cfg(not(test))]
        let _ = command;
        Ok(())
    }
    fn lifecycle_handle(&self) -> Arc<Self::Lifecycle> {
        ApplicationWorkflow::lifecycle_handle(self)
    }
    fn install_candidate_gate(
        &self,
        gate: Arc<chimera_core::client::reconciler::check::CandidateCheck>,
    ) -> anyhow::Result<()> {
        self.lifecycle().install_checked_gate(gate)
    }
    fn legacy_service_connected() -> bool {
        crate::core::service::ipc::get_ipc_state().is_connected()
    }
    fn checked_clash_notice(notice: Result<(), String>) {
        #[cfg(not(test))]
        crate::core::handle::Handle::notice_message(&crate::core::handle::Message::SetConfig(
            notice,
        ));
        #[cfg(test)]
        let _ = notice;
    }
    fn take_prepared_outcome(
        &self,
    ) -> Option<chimera_core::client::reconciler::mutation::ApplyOutcome> {
        self.lifecycle().take_prepared_outcome()
    }
    fn take_checked_connection_effect(
        &self,
    ) -> Option<chimera_core::client::reconciler::apply::CheckedConnectionEffect> {
        self.lifecycle().take_checked_connection_effect()
    }
    async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        self.lifecycle().effect_port_binding().await
    }
    fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
    ) {
        ApplicationWorkflow::runtime_bound(self, ports);
    }
    async fn service_endpoint_recovery_due(&self) -> bool {
        ApplicationWorkflow::service_endpoint_recovery_due(self).await
    }
    #[cfg(test)]
    async fn handle_test_message(
        message: Message,
        state: &mut RuntimeReconcilerMailbox,
    ) -> Result<bool, ActorProcessingErr> {
        match message {
            Message::ConnectApplicationWorkflow {
                workflow: incoming,
                reply,
                registered,
            } => {
                let validation = if state.mailbox.shutting_down {
                    Err(anyhow::anyhow!("core lifecycle is shutting down"))
                } else if state.pending_mutation_connection.is_some()
                    || state
                        .workflow
                        .as_ref()
                        .is_some_and(ApplicationWorkflow::mutation_connected)
                {
                    Err(anyhow::anyhow!("application workflow is already connected"))
                } else if state.workflow.is_none() && !reply.is_closed() {
                    state.pending_mutation_connection = Some((incoming, reply));
                    if let Some(registered) = registered {
                        let _ = registered.send(());
                    }
                    return Ok(false);
                } else if let Some(workflow) = state.workflow.as_ref() {
                    workflow.validate_mutation_connection(&incoming)
                } else {
                    Err(anyhow::anyhow!("core lifecycle workflow is unavailable"))
                };
                if let Some(registered) = registered {
                    let _ = registered.send(());
                }
                match validation {
                    Ok(()) => {
                        if reply.send(Ok(())).is_ok() {
                            state
                                .workflow
                                .as_mut()
                                .expect("validated workflow remains available")
                                .attach_mutation_workflow(incoming)
                                .expect("validated application workflow connection attaches");
                        }
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Message::ProbeService { reply } => {
                let _ = reply.send(state.refresh_service_status().await);
            }
            Message::ReadSourceVersionsForTest { reply } => {
                let _ = reply.send(state.runtime.seen_sources.clone());
            }
            Message::ReadAppliedSelectionForTest { reply } => {
                let selection = state
                    .workflow
                    .as_ref()
                    .and_then(|workflow| workflow.lifecycle().applied_selection_for_tests());
                let _ = reply.send(selection);
            }
            Message::ReadCheckedForTest { reply } => {
                let _ = reply.send(state.runtime.checked.first().map(|checked| {
                    (
                        checked.prepared.target.clone(),
                        checked.prepared.intent.digest.clone(),
                        checked.prepared.snapshot.product_bytes().to_vec(),
                    )
                }));
            }
            Message::ArmStartupRetryLeaseFailureObserver { observer, reply } => {
                if state.startup_retry_lease_failure_observer.is_some() {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "a startup retry lease failure observer is already armed"
                    )));
                } else {
                    state.startup_retry_lease_failure_observer = Some(observer);
                    let _ = reply.send(Ok(()));
                }
            }
            Message::ReadStartupRetryScheduleForTest { reply } => {
                let schedule = state
                    .workflow
                    .as_ref()
                    .and_then(ApplicationWorkflow::startup_retry_schedule);
                let _ = reply.send(schedule);
            }
            Message::SeedUnresolvedApplicationAttemptForTest { reply } => {
                let result = state
                    .workflow
                    .as_mut()
                    .map(ApplicationWorkflow::seed_unresolved_lifecycle_attempt_for_tests);
                let _ = reply.send(
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable")),
                );
            }
            Message::SeedUnresolvedSourceDecisionForTest { decision, reply } => {
                let result = state
                    .workflow
                    .as_mut()
                    .map(|workflow| workflow.seed_unresolved_source_decision_for_tests(decision));
                let _ = reply.send(
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable")),
                );
            }
            Message::SeedUnresolvedAttemptWithDeferredRetryForTest { decision, reply } => {
                let result = state.workflow.as_mut().map(|workflow| {
                    workflow.seed_unresolved_attempt_with_deferred_retry_for_tests(decision)
                });
                let result =
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable"));
                if let Ok((_, digest)) = &result {
                    let id = state.mailbox.allocate_operation_id();
                    state.mailbox.pending.push_back(Request {
                        command: Command::RetryDeferredProfiles {
                            digest: digest.clone(),
                        },
                        response: Response { id, reply: None },
                        deferred_retry: Some(digest.clone()),
                    });
                }
                let _ = reply.send(result);
            }
            Message::SeedUnresolvedCommittedTargetForTest { decision, reply } => {
                let result = state
                    .workflow
                    .as_mut()
                    .map(|workflow| workflow.seed_unresolved_committed_target_for_tests(decision));
                let _ = reply.send(
                    result.ok_or_else(|| anyhow::anyhow!("application workflow is unavailable")),
                );
            }
            Message::ReadApplicationRecoveryStateForTest { reply } => {
                let state = state.workflow.as_ref().map_or(
                    (None, None, None),
                    ApplicationWorkflow::recovery_state_for_tests,
                );
                let _ = reply.send(state);
            }

            _ => {
                return Err(
                    anyhow::anyhow!("unexpected production message in test adapter").into(),
                );
            }
        }
        Ok(true)
    }
    #[cfg(test)]
    fn restore_pending_connection(state: &mut RuntimeReconcilerMailbox) {
        if let Some((incoming, reply)) = state.pending_mutation_connection.take() {
            let validation = if state.mailbox.shutting_down {
                Err(anyhow::anyhow!("core lifecycle is shutting down"))
            } else if reply.is_closed() {
                Err(anyhow::anyhow!(
                    "application workflow connection was cancelled before attachment"
                ))
            } else {
                state
                    .workflow
                    .as_ref()
                    .expect("completed workflow was restored")
                    .validate_mutation_connection(&incoming)
            };
            match validation {
                Ok(()) => {
                    if reply.send(Ok(())).is_ok() {
                        state
                            .workflow
                            .as_mut()
                            .expect("completed workflow was restored")
                            .attach_mutation_workflow(incoming)
                            .expect("validated application workflow connection attaches");
                    }
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            }
        }
    }
}

fn record_startup_retry_lease_failure(
    workflow: Option<&mut ApplicationWorkflow>,
    pending: &mut VecDeque<(String, String)>,
    identity: String,
    reason: String,
) {
    chimera_core::client::reconciler::dispatch::record_startup_retry_lease_failure(
        workflow.map(ApplicationWorkflow::progress_mut),
        pending,
        identity,
        reason,
    );
}

fn apply_pending_startup_retry_lease_failures(
    workflow: Option<&mut ApplicationWorkflow>,
    pending: &mut VecDeque<(String, String)>,
) {
    chimera_core::client::reconciler::dispatch::apply_pending_startup_retry_lease_failures(
        workflow.map(ApplicationWorkflow::progress_mut),
        pending,
    );
}

type RuntimeReconcilerArgs =
    chimera_core::client::reconciler::actor::RuntimeReconcilerArgs<ApplicationWorkflow>;
type RuntimeReconcilerActor = chimera_core::client::reconciler::actor::RuntimeReconcilerActor<
    ApplicationWorkflow,
    ProxyHandle,
>;
type Request = chimera_core::client::reconciler::mailbox::Request<Command>;

enum ClientInner {
    Actor(ActorClient),
    #[cfg(test)]
    Direct {
        workflow: tokio::sync::Mutex<CoreLifecycleWorkflow>,
        service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        core: Arc<dyn CoreLifecyclePort>,
        startup_state: tokio::sync::Mutex<StartupWorkflowState>,
    },
}

#[derive(Clone)]
pub(crate) struct RuntimeReconcilerClient(Arc<ClientInner>);

/// A direct, core-only test has no ServiceActor. Its Service endpoint is
/// explicitly **not installed**, not an ambiguous OS-global legacy probe.
/// This makes the fixture model a real no-daemon environment while keeping
/// production's fail-closed S2 ownership checks fully enabled.
#[cfg(test)]
struct DirectNoDaemonService {
    inner: Arc<dyn ports::ServiceLifecyclePort>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl ports::ServiceLifecyclePort for DirectNoDaemonService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        Ok(chimera_ipc::types::StatusInfo {
            name: "chimera-test-service".into(),
            version: "0".into(),
            status: chimera_ipc::types::ServiceStatus::NotInstalled,
            server: None,
        })
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.begin_transition().await
    }

    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.try_begin_transition().await
    }
}

impl RuntimeReconcilerClient {
    #[cfg(test)]
    pub(in crate::client) async fn spawn(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_service(
            core,
            application,
            clash,
            runtime_paths,
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            Arc::new(LegacyServiceBridge::new(Arc::new(
                crate::core::actor_v2::CoreFacade::new_local(),
            ))),
        )
        .await
    }

    #[cfg(test)]
    async fn spawn_with_installer(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
    ) -> anyhow::Result<Self> {
        let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(LegacyServiceBridge::new(
            Arc::new(crate::core::actor_v2::CoreFacade::new_local()),
        ));
        Self::spawn_with_service(core, application, clash, runtime_paths, installer, service).await
    }

    pub(in crate::client) async fn spawn_with_service(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
    ) -> anyhow::Result<Self> {
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        Self::spawn_with_profiles(
            core,
            application,
            clash,
            None,
            runtime_paths,
            installer,
            service,
            shutdown,
            &tasks,
        )
        .await
    }

    pub(in crate::client) async fn spawn_with_profiles(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_profiles_and_content(
            core,
            application,
            clash,
            profiles,
            None,
            None,
            runtime_paths,
            installer,
            service,
            shutdown,
            tasks,
        )
        .await
    }

    /// Production passes the read fence owned by the Profiles source actor;
    /// test adapters without a ProfilesActor may keep the old snapshot path.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::client) async fn spawn_with_profiles_and_content(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
        content_reader: Option<crate::state::mutation::ProfileContentReader>,
        content_source: Option<
            Arc<dyn chimera_config::runtime::executor::ProfileContentSource + Send + Sync>,
        >,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
        service: Arc<dyn ports::ServiceLifecyclePort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let source_authority =
            content_reader
                .clone()
                .zip(content_source)
                .map(|(reader, content)| {
                    Arc::new(
                        crate::client::reconciler::authority::LiveSourceAuthority::new(
                            application.clone(),
                            (*clash).clone(),
                            reader,
                            content,
                        ),
                    )
                        as Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>
                });
        let recovery_notify = core.recovery_notify();
        let service_status = ServiceHostWatch::new();
        let service_status_rx = service_status.subscribe();
        let lifecycle = CoreLifecycleWorkflow::new(
            application,
            clash,
            profiles,
            core.clone(),
            installer,
            runtime_paths,
            service.clone(),
        )
        .with_shutdown(shutdown.child_token())
        .with_service_status(service_status_rx.clone())
        .with_profile_content_reader(content_reader)
        .with_source_authority(source_authority.clone());
        let startup_owner_unproven = lifecycle.startup_owner_unproven_handle();
        #[cfg(not(test))]
        let startup_owner_unproven_signal = startup_owner_unproven.clone();
        let service_failed_generation = lifecycle.service_failed_generation_handle();
        let workflow = ApplicationWorkflow::with_lifecycle(lifecycle);
        let startup_recovery_pending = Arc::new(AtomicBool::new(false));
        let status = Arc::new(parking_lot::Mutex::new(CoreLifecycleStatus::default()));
        let (reconciler_tx, reconciler) = tokio::sync::watch::channel(ReconcilerStatus::default());
        let next_id = Arc::new(AtomicU64::new(1));
        let (actor_ref, _handle) = Actor::spawn(
            None,
            RuntimeReconcilerActor::new(),
            RuntimeReconcilerArgs {
                reconciler: reconciler_tx,
                workflow,
                next_id: next_id.clone(),
                status: status.clone(),
                service_status,
                service: service.clone(),
                source_authority: source_authority.clone(),
                startup_owner_unproven_shared: startup_owner_unproven.clone(),
                startup_recovery_pending: startup_recovery_pending.clone(),
            },
        )
        .await?;
        if let Some(recovery_notify) = recovery_notify {
            let recovery_actor = actor_ref.clone();
            let recovery_shutdown = shutdown.child_token();
            tasks.spawn(async move {
                loop {
                    tokio::select! {
                        _ = recovery_shutdown.cancelled() => break,
                        _ = recovery_notify.notified() => {
                            if recovery_actor.cast(Message::RecoverCore).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        #[cfg(not(test))]
        let service_actor_updates = service.actor_status_updates();
        #[cfg(not(test))]
        let service_followup_actor = actor_ref.clone();
        let client = Self(Arc::new(ClientInner::Actor(ActorClient {
            actor_ref,
            next_id,
            status,
            reconciler,
            service_status: service_status_rx.clone(),
            service,
            core,
            startup_owner_unproven,
            service_failed_generation,
            startup_recovery_pending,
            source_authority,
        })));
        #[cfg(not(test))]
        {
            let recovery_client = client.clone();
            let recovery_shutdown = shutdown.child_token();
            tasks.spawn(
                chimera_core::client::reconciler::service_observer::observe_service(
                    service_actor_updates,
                    service_status_rx,
                    startup_owner_unproven_signal,
                    recovery_shutdown,
                    crate::client::service_status::project_actor_service_status,
                    move || {
                        service_followup_actor
                            .cast(Message::ServiceReadiness)
                            .is_ok()
                    },
                    move || {
                        if recovery_client.startup_recovery_needed() {
                            recovery_client.request_startup_recovery();
                        }
                    },
                ),
            );
        }
        // Ref ServiceActor already published its initial daemon probe during
        // startup. No second CoreLifecycle probe/cache refresh is needed.
        Ok(client)
    }

    #[cfg(test)]
    pub(in crate::client) fn direct(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> Self {
        let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(DirectNoDaemonService {
            inner: Arc::new(LegacyServiceBridge::new(Arc::new(
                crate::core::actor_v2::CoreFacade::new_local(),
            ))),
        });
        let (_service_status_tx, service_status_rx) =
            tokio::sync::watch::channel(ServiceHostStatus::probing());
        let workflow = CoreLifecycleWorkflow::new(
            application,
            clash,
            None,
            core.clone(),
            Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                chimera_os::elevation::RefuseElevation,
            ))),
            runtime_paths,
            service.clone(),
        );
        let startup_state =
            StartupWorkflowState::with_owner_signal(workflow.startup_owner_unproven_handle());
        Self(Arc::new(ClientInner::Direct {
            workflow: tokio::sync::Mutex::new(workflow),
            service_status: service_status_rx,
            service,
            core,
            startup_state: tokio::sync::Mutex::new(startup_state),
        }))
    }

    pub(in crate::client) async fn install_candidate_gate(
        &self,
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.install_candidate_gate(gate).await,
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                anyhow::bail!("production CandidateCheck requires the lifecycle actor")
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn connect_application_workflow(
        &self,
        workflow: ApplicationWorkflow,
    ) -> anyhow::Result<()> {
        self.connect_application_workflow_with_registration(workflow, None)
            .await
    }

    #[cfg(test)]
    pub(in crate::client) async fn connect_application_workflow_with_registration(
        &self,
        workflow: ApplicationWorkflow,
        registered: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { actor_ref, .. }) => {
                match actor_ref
                    .call(
                        |reply| Message::ConnectApplicationWorkflow {
                            workflow,
                            reply,
                            registered,
                        },
                        Some(CALL_WAIT),
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => {
                        anyhow::bail!("application workflow connection timed out")
                    }
                    CallResult::SenderError => {
                        anyhow::bail!("application workflow connection reply dropped")
                    }
                }
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                anyhow::bail!("application workflow requires the lifecycle actor")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn begin_profile_mutation(
        &self,
        request: crate::client::application_workflow::mutation::MutationRequest,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient {
                actor_ref, next_id, ..
            }) => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                actor_ref
                    .cast(Message::Request(Request {
                        command: Command::ProfileMutation(Box::new(request)),
                        response: Response { id, reply: None },
                        deferred_retry: None,
                    }))
                    .map_err(|error| anyhow::anyhow!("failed to enqueue Profile mutation: {error}"))
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                anyhow::bail!("Profile mutations require the lifecycle actor")
            }
        }
    }

    async fn execute(&self, command: Command) -> anyhow::Result<()> {
        self.execute_with_timeout(command, CALL_WAIT).await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        self.execute(Command::DisableStaleSystemProxy {
            system_proxy,
            expected_port,
            expected_core_state_changed_at,
            desired_before,
            original,
            notifications,
            persist_legacy_mirror,
        })
        .await
        .map_err(|error| {
            error
                .downcast::<crate::client::system_proxy::SystemProxyActionError>()
                .unwrap_or_else(|error| {
                    crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
                })
        })
    }

    async fn execute_with_timeout(
        &self,
        command: Command,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.execute_with_timeout(command, timeout).await,
            #[cfg(test)]
            ClientInner::Direct {
                workflow,
                startup_state,
                ..
            } => {
                let workflow = workflow.lock().await;
                let mut startup_state = startup_state.lock().await;
                workflow
                    .execute(command, startup_state.retry_target_mut())
                    .await
            }
        }
    }

    #[cfg(test)]
    async fn startup_retry_schedule_for_tests(
        &self,
    ) -> anyhow::Result<Option<(String, tokio::time::Instant)>> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("startup retry schedule test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::ReadStartupRetryScheduleForTest { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(schedule) => Ok(schedule),
            CallResult::Timeout => anyhow::bail!("startup retry schedule query timed out"),
            CallResult::SenderError => {
                anyhow::bail!("startup retry schedule query reply was dropped")
            }
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_application_attempt_for_tests(
        &self,
    ) -> anyhow::Result<chimera_core_manager::OperationId> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("application attempt test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedApplicationAttemptForTest { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("application attempt seeding timed out"),
            CallResult::SenderError => anyhow::bail!("application attempt seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_source_decision_for_tests(
        &self,
        decision: chimera_core::state::DecisionHandle,
    ) -> anyhow::Result<chimera_core_manager::OperationId> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("source decision test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedSourceDecisionForTest { decision, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("source decision seeding timed out"),
            CallResult::SenderError => anyhow::bail!("source decision seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_attempt_with_deferred_retry_for_tests(
        &self,
        decision: chimera_core::state::DecisionHandle,
    ) -> anyhow::Result<(chimera_core_manager::OperationId, String)> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("deferred retry test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedAttemptWithDeferredRetryForTest { decision, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("deferred retry test seeding timed out"),
            CallResult::SenderError => anyhow::bail!("deferred retry seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn seed_unresolved_committed_target_for_tests(
        &self,
        decision: chimera_core::state::DecisionHandle,
    ) -> anyhow::Result<chimera_core_manager::OperationId> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("committed target test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::SeedUnresolvedCommittedTargetForTest { decision, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("committed target seeding timed out"),
            CallResult::SenderError => anyhow::bail!("committed target seed reply dropped"),
        }
    }

    #[cfg(test)]
    async fn application_recovery_state_for_tests(
        &self,
    ) -> anyhow::Result<(
        Option<(chimera_core_manager::OperationId, bool)>,
        Option<(String, tokio::time::Instant)>,
        Option<(u8, u32)>,
    )> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("application recovery test requires the lifecycle actor");
        };
        match actor_ref
            .call(
                |reply| Message::ReadApplicationRecoveryStateForTest { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(state) => Ok(state),
            CallResult::Timeout => anyhow::bail!("application recovery state query timed out"),
            CallResult::SenderError => {
                anyhow::bail!("application recovery state reply was dropped")
            }
        }
    }

    #[cfg(test)]
    async fn observe_next_startup_retry_lease_failure(
        &self,
    ) -> anyhow::Result<tokio::sync::oneshot::Receiver<()>> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            anyhow::bail!("startup retry observer requires the lifecycle actor");
        };
        let (observer, observed) = tokio::sync::oneshot::channel();
        match actor_ref
            .call(
                |reply| Message::ArmStartupRetryLeaseFailureObserver { observer, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result.map(|()| observed),
            CallResult::Timeout => anyhow::bail!("startup retry observer arm timed out"),
            CallResult::SenderError => {
                anyhow::bail!("startup retry observer reply was dropped")
            }
        }
    }

    pub(in crate::client) async fn startup_reconcile(
        &self,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        self.startup_reconcile_with_timeout(None).await
    }

    async fn cached_startup_report(&self) -> anyhow::Result<Option<StartupReport>> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.cached_startup_report().await,
            #[cfg(test)]
            ClientInner::Direct { workflow, .. } => Ok(workflow.lock().await.startup_report()),
        }
    }

    async fn startup_reconcile_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.startup_reconcile_with_timeout(timeout).await,
            #[cfg(test)]
            ClientInner::Direct {
                workflow,
                service,
                startup_state,
                ..
            } => {
                if let Some(report) = workflow.lock().await.startup_report() {
                    return Ok(completed_from_startup_report(report));
                }
                let transition = match service.begin_transition().await {
                    Ok(transition) => transition,
                    Err(error) => {
                        let reason =
                            format!("could not acquire the Service transition lease: {error:#}");
                        let workflow = workflow.lock().await;
                        startup_state.lock().await.mark_owner_unproven();
                        let report = StartupReport {
                            operation_id: 1,
                            observation: None,
                            outcome: StartupOutcome::Unsettled { reason },
                        };
                        workflow.cache_startup_report_for_tests(&report);
                        return Ok(completed_from_startup_report(report));
                    }
                };
                if let Some(report) = workflow.lock().await.startup_report() {
                    return Ok(completed_from_startup_report(report));
                }
                let workflow = workflow.lock().await;
                let mut startup_state = startup_state.lock().await;
                let result = workflow
                    .execute(
                        Command::StartupReconcile {
                            operation_id: 1,
                            transition,
                        },
                        startup_state.retry_target_mut(),
                    )
                    .await;
                let report = match workflow.startup_report() {
                    Some(report) => report,
                    None => {
                        let reason = result
                            .as_ref()
                            .err()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| {
                                "direct startup lifecycle completed without a report".into()
                            });
                        startup_state.mark_owner_unproven();
                        let report = StartupReport {
                            operation_id: 1,
                            observation: None,
                            outcome: StartupOutcome::Unsettled { reason },
                        };
                        workflow.cache_startup_report_for_tests(&report);
                        report
                    }
                };
                Ok(CompletedLifecycleOperation { result, report })
            }
        }
    }

    fn startup_recovery_needed(&self) -> bool {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.startup_recovery_needed(),
            #[cfg(test)]
            ClientInner::Direct { .. } => false,
        }
    }

    /// The ref RuntimePreparation must share both its revision sequence and
    /// its confirmed port baseline with this same mutating Core owner.
    /// No new allocator or resolver is synthesized for compatibility ports.
    pub(in crate::client) fn runtime_preparation_session(
        &self,
    ) -> Option<crate::client::ports::RuntimePreparationSession> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.runtime_preparation_session(),
            #[cfg(test)]
            ClientInner::Direct { core, .. } => core.runtime_preparation_session(),
        }
    }

    pub(crate) fn status(&self) -> CoreLifecycleStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.status(),
            #[cfg(test)]
            ClientInner::Direct { .. } => CoreLifecycleStatus::default(),
        }
    }

    pub(crate) fn subscribe_reconciler(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<ReconcilerStatus>> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => Some(client.subscribe_reconciler()),
            #[cfg(test)]
            ClientInner::Direct { .. } => None,
        }
    }

    /// Ref-shaped outcome projection; unlike the legacy lifecycle status,
    /// this carries the last sampled source versions and runtime conclusion.
    pub(crate) fn reconciler_status(&self) -> ReconcilerStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.reconciler_status(),
            #[cfg(test)]
            ClientInner::Direct { .. } => ReconcilerStatus::default(),
        }
    }

    /// Explicitly dispatch an acknowledged, committed checked value through
    /// the same lifecycle queue used by the Profiles production gate. The
    /// canonical authority verifies the latest source content before and
    /// after Core apply; the composition root owns that reader. An ACK is
    /// proof of candidate ownership, not Core apply success.
    pub(in crate::client) async fn apply_checked_committed(
        &self,
        domain: SourceDomain,
        version: u64,
        target: String,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client
                    .apply_checked_committed(domain, version, target)
                    .await
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                anyhow::bail!("authoritative source verification is not connected")
            }
        }
    }

    async fn submit_checked_with_authority(
        &self,
        domain: SourceDomain,
        version: u64,
        target: String,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
    ) -> anyhow::Result<()> {
        let ClientInner::Actor(client) = self.0.as_ref() else {
            anyhow::bail!("checked runtime apply requires the lifecycle actor");
        };
        client
            .submit_checked_with_authority(domain, version, target, authority)
            .await
    }

    /// The ref Source Gate always tags an ACKed candidate with its own
    /// durable commit domain; multiple domains can check concurrently.
    pub(in crate::client) async fn checked_candidate_for_domain(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_inner(domain, prepared, core_spec, None, false)
            .await
    }

    pub(in crate::client) async fn checked_candidate_for_domain_with_request(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: String,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_inner(domain, prepared, core_spec, Some(request), false)
            .await
    }

    pub(in crate::client) async fn checked_candidate_for_domain_with_request_effect(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: String,
        interrupt_mode: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_inner(domain, prepared, core_spec, Some(request), interrupt_mode)
            .await
    }

    async fn checked_candidate_inner(
        &self,
        domain: SourceDomain,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: Option<String>,
        interrupt_mode: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        #[cfg(test)]
        use chimera_contracts::runtime_error::RuntimeError;
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client
                    .checked_candidate_inner(domain, prepared, core_spec, request, interrupt_mode)
                    .await
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => Err(Arc::new(RuntimeError::ShuttingDown)),
        }
    }

    #[cfg(test)]
    async fn checked_candidate(
        &self,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.checked_candidate_for_domain(SourceDomain::Profiles, prepared, core_spec)
            .await
    }

    #[cfg(test)]
    async fn applied_selection_for_tests(
        &self,
    ) -> Option<crate::client::reconciler::committed::Selection> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            return None;
        };
        match actor_ref
            .call(|reply| Message::ReadAppliedSelectionForTest { reply }, None)
            .await
        {
            Ok(CallResult::Success(selection)) => selection,
            _ => None,
        }
    }

    #[cfg(test)]
    async fn checked_for_tests(&self) -> Option<(Option<String>, String, Vec<u8>)> {
        let ClientInner::Actor(ActorClient { actor_ref, .. }) = self.0.as_ref() else {
            return None;
        };
        match actor_ref
            .call(|reply| Message::ReadCheckedForTest { reply }, None)
            .await
        {
            Ok(CallResult::Success(snapshot)) => snapshot,
            _ => None,
        }
    }

    pub(crate) async fn settled(&self, domain: SourceDomain, version: u64) -> ReconcilerStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.settled(domain, version).await,
            #[cfg(test)]
            ClientInner::Direct { .. } => ReconcilerStatus::default(),
        }
    }

    /// Conservative rollback for a failed source CAS after the Core owner
    /// acknowledged check admission. The abort and source notices share its
    /// one ordered mailbox; on cross-request same-domain ambiguity, the next
    /// successful commit falls back to authoritative latest-source recovery.
    pub(in crate::client) fn abort_uncommitted_checked(&self, domain: SourceDomain) {
        self.abort_checked_inner(domain, None);
    }

    pub(in crate::client) fn abort_uncommitted_checked_request(
        &self,
        domain: SourceDomain,
        request: String,
    ) {
        self.abort_checked_inner(domain, Some(request));
    }

    fn abort_checked_inner(&self, domain: SourceDomain, request: Option<String>) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.abort_checked_inner(domain, request),
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    /// Ref checked source notification. The single Actor consumes the cached
    /// checked value through ApplyChecked; it does not enqueue the old
    /// SourceCommitted rebuild when a checked value was available.
    pub(in crate::client) fn checked_source_committed(&self, domain: SourceDomain, version: u64) {
        self.checked_source_committed_inner(domain, version, None);
    }

    pub(in crate::client) fn checked_source_committed_for_request(
        &self,
        domain: SourceDomain,
        version: u64,
        request: String,
    ) {
        self.checked_source_committed_inner(domain, version, Some(request));
    }

    fn checked_source_committed_inner(
        &self,
        domain: SourceDomain,
        version: u64,
        request: Option<String>,
    ) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client.checked_source_committed_inner(domain, version, request)
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    pub(crate) fn source_committed(
        &self,
        domain: SourceDomain,
        version: u64,
        reconcile_requested: bool,
    ) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client.source_committed(domain, version, reconcile_requested)
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    pub(crate) fn request_runtime_rebuild(&self) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.request_runtime_rebuild(),
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    fn request_startup_recovery(&self) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.request_startup_recovery(),
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    pub(in crate::client) async fn request_service_endpoint_down(&self) {
        self.request_service_endpoint_down_if(|| true).await;
    }

    pub(in crate::client) async fn request_service_endpoint_down_if<F>(&self, still_current: F)
    where
        F: Fn() -> bool + Send + 'static,
    {
        match self.0.as_ref() {
            ClientInner::Actor(client) => {
                client.request_service_endpoint_down_if(still_current).await
            }
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    pub(in crate::client) fn service_status(&self) -> ServiceHostStatus {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.service_status(),
            #[cfg(test)]
            ClientInner::Direct { service_status, .. } => service_status.borrow().clone(),
        }
    }

    pub(in crate::client) fn observe_service_status(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
    ) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.observe_service_status(info),
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    pub(in crate::client) fn observe_service_probe_failure(&self) {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.observe_service_probe_failure(),
            #[cfg(test)]
            ClientInner::Direct { .. } => {}
        }
    }

    #[cfg(test)]
    pub(in crate::client) async fn probe_service(&self) -> anyhow::Result<ServiceHostStatus> {
        match self.0.as_ref() {
            ClientInner::Actor(ActorClient { actor_ref, .. }) => {
                match actor_ref
                    .call(|reply| Message::ProbeService { reply }, None)
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => anyhow::bail!("service probe timed out"),
                    CallResult::SenderError => anyhow::bail!("service probe reply dropped"),
                }
            }
            #[cfg(test)]
            ClientInner::Direct { workflow, .. } => workflow
                .lock()
                .await
                .probe_service()
                .await
                .map(classify_service_probe),
        }
    }

    /// Explicitly resume core execution; retrying a source while stopped
    /// never implicitly changes this intent.
    pub(in crate::client) async fn start_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StartCore).await
    }

    pub(in crate::client) async fn stop_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StopCore).await
    }

    pub(in crate::client) async fn shutdown(&self) -> anyhow::Result<()> {
        self.execute(Command::Shutdown).await
    }

    pub(in crate::client) async fn reconcile(&self) -> anyhow::Result<()> {
        if self.startup_recovery_needed() {
            return self.recover_startup().await;
        }
        self.execute(Command::Reconcile).await
    }

    async fn recover_startup(&self) -> anyhow::Result<()> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.recover_startup().await,
            #[cfg(test)]
            ClientInner::Direct {
                workflow,
                service,
                startup_state,
                ..
            } => {
                let transition = service.begin_transition().await?;
                let workflow = workflow.lock().await;
                let mut startup_state = startup_state.lock().await;
                let operation_id = workflow
                    .startup_report()
                    .map_or(1, |report| report.operation_id.saturating_add(1));
                workflow
                    .execute(
                        Command::RecoverStartup {
                            operation_id,
                            transition,
                        },
                        startup_state.retry_target_mut(),
                    )
                    .await
            }
        }
    }

    pub(crate) async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReconcileProfiles {
            profiles,
            staged_content,
        })
        .await
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.core_status().await,
            #[cfg(test)]
            ClientInner::Direct { core, .. } => core.status().await,
        }
    }

    // Only legacy actor tests exercise the original direct-core adapter.
    // Production selects through ChimeraClient::patch_app_config instead.
    #[cfg(test)]
    pub(in crate::client) async fn select_core(
        &self,
        core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.execute(Command::SelectCore(core)).await
    }

    pub(in crate::client) async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<workflow::ApplicationPatchResult> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.patch_application_config(patch).await,
            #[cfg(test)]
            ClientInner::Direct { .. } => {
                let (degradations, settled) = tokio::sync::oneshot::channel();
                self.execute(Command::PatchApplicationConfig {
                    patch,
                    service_connected: false,
                    degradations,
                    runtime_converged: Arc::new(AtomicBool::new(true)),
                })
                .await?;
                settled
                    .await
                    .map_err(|_| anyhow::anyhow!("application runtime result was not delivered"))
            }
        }
    }

    pub(in crate::client) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReplaceCoreBinary(artifact)).await
    }

    async fn execute_service_command<F>(&self, command: F) -> anyhow::Result<()>
    where
        F: FnOnce(Box<dyn ports::ServiceTransitionLease>) -> Command,
    {
        match self.0.as_ref() {
            ClientInner::Actor(client) => client.execute_service_command(command).await,
            #[cfg(test)]
            ClientInner::Direct { service, .. } => {
                let transition = service.begin_transition().await?;
                self.execute(command(transition)).await
            }
        }
    }

    pub(in crate::client) async fn install_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::InstallService).await
    }

    pub(in crate::client) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UninstallService)
            .await
    }

    pub(in crate::client) async fn update_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UpdateService).await
    }

    pub(in crate::client) async fn start_service(
        &self,
        ready_timeout: Duration,
    ) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::StartService {
            transition,
            ready_timeout,
        })
        .await
    }

    pub(in crate::client) async fn restart_service(
        &self,
        ready_timeout: Duration,
    ) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::RestartService {
            transition,
            ready_timeout,
        })
        .await
    }

    pub(in crate::client) async fn stop_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::StopService).await
    }
}

impl RuntimeReconcilerClient {
    pub(in crate::client) fn shared_handle(
        &self,
    ) -> Arc<dyn chimera_core::client::reconciler::facade::RuntimeReconciler> {
        match self.0.as_ref() {
            ClientInner::Actor(client) => Arc::new(client.clone()),
            #[cfg(test)]
            ClientInner::Direct { .. } => Arc::new(self.clone()),
        }
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl chimera_core::client::reconciler::facade::RuntimeReconciler for RuntimeReconcilerClient {
    async fn startup_reconcile(&self) -> anyhow::Result<CompletedLifecycleOperation> {
        RuntimeReconcilerClient::startup_reconcile(self).await
    }

    fn status(&self) -> CoreLifecycleStatus {
        RuntimeReconcilerClient::status(self)
    }

    fn subscribe_reconciler(&self) -> Option<tokio::sync::watch::Receiver<ReconcilerStatus>> {
        RuntimeReconcilerClient::subscribe_reconciler(self)
    }

    fn reconciler_status(&self) -> ReconcilerStatus {
        RuntimeReconcilerClient::reconciler_status(self)
    }

    async fn settled(&self, domain: SourceDomain, version: u64) -> ReconcilerStatus {
        RuntimeReconcilerClient::settled(self, domain, version).await
    }

    fn source_committed(&self, domain: SourceDomain, version: u64, reconcile_requested: bool) {
        RuntimeReconcilerClient::source_committed(self, domain, version, reconcile_requested)
    }

    fn request_runtime_rebuild(&self) {
        RuntimeReconcilerClient::request_runtime_rebuild(self)
    }

    async fn request_service_endpoint_down_if(
        &self,
        still_current: Box<dyn Fn() -> bool + Send + 'static>,
    ) {
        RuntimeReconcilerClient::request_service_endpoint_down_if(self, still_current).await
    }

    fn service_status(&self) -> ServiceHostStatus {
        RuntimeReconcilerClient::service_status(self)
    }

    fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        RuntimeReconcilerClient::observe_service_status(self, info)
    }

    fn observe_service_probe_failure(&self) {
        RuntimeReconcilerClient::observe_service_probe_failure(self)
    }

    async fn start_core(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::start_core(self).await
    }

    async fn stop_core(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::stop_core(self).await
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::shutdown(self).await
    }

    async fn reconcile(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::reconcile(self).await
    }

    async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<workflow::ApplicationPatchResult> {
        RuntimeReconcilerClient::patch_application_config(self, patch).await
    }

    async fn replace_core_binary(&self, artifact: PreparedCoreBinary) -> anyhow::Result<()> {
        RuntimeReconcilerClient::replace_core_binary(self, artifact).await
    }

    async fn install_service(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::install_service(self).await
    }

    async fn uninstall_service(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::uninstall_service(self).await
    }

    async fn update_service(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::update_service(self).await
    }

    async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        RuntimeReconcilerClient::start_service(self, ready_timeout).await
    }

    async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        RuntimeReconcilerClient::restart_service(self, ready_timeout).await
    }

    async fn stop_service(&self) -> anyhow::Result<()> {
        RuntimeReconcilerClient::stop_service(self).await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        RuntimeReconcilerClient::disable_stale_system_proxy(
            self,
            system_proxy,
            expected_port,
            expected_core_state_changed_at,
            desired_before,
            original,
            notifications,
            persist_legacy_mirror,
        )
        .await
    }
}

impl ChimeraClient {
    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        // Legacy frontend IPC only has Running/Stopped. Do not collapse ref's
        // Starting/Switching/Degraded projections into a fake Stopped result;
        // keep this compatibility endpoint until the UI consumes the typed
        // CoreActor projection directly.
        self.inner.shared.core().status().await
    }

    pub(crate) fn service_status(&self) -> ServiceHostStatus {
        if let Some(service) = self.inner.shared.service_actor() {
            return crate::client::service_status::project_actor_service_status(&service.status());
        }
        // Injected legacy lifecycle fixtures deliberately omit ServiceActor.
        // Production always owns the ref ServiceClient from Tauri setup().
        self.inner.reconciler().service_status()
    }

    pub(crate) fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        self.inner.reconciler().observe_service_status(info);
    }

    pub(crate) fn observe_service_probe_failure(&self) {
        self.inner.reconciler().observe_service_probe_failure();
    }

    pub(crate) async fn request_service_endpoint_down(&self) {
        self.inner
            .reconciler()
            .request_service_endpoint_down()
            .await;
    }

    pub(crate) async fn request_service_endpoint_down_if<F>(&self, still_current: F)
    where
        F: Fn() -> bool + Send + 'static,
    {
        self.inner
            .reconciler()
            .request_service_endpoint_down_if(Box::new(still_current))
            .await;
    }

    pub(crate) fn core_lifecycle_status(&self) -> CoreLifecycleStatus {
        self.inner.reconciler().status()
    }

    pub(crate) fn request_runtime_rebuild(&self) {
        self.inner.reconciler().request_runtime_rebuild();
    }

    pub(crate) fn runtime_transform_diagnostics(
        &self,
    ) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        self.inner.shared.core().runtime_transform_diagnostics()
    }

    /// Ref: selecting a core is an Application source transaction, not a
    /// second mutating CoreLifecycle command. Preserve the commit receipt
    /// and post-commit runtime degradations for the UI.
    pub(crate) async fn change_core(
        &self,
        clash_core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<crate::client::runtime::MutationOutcome<()>> {
        self.patch_app_config(chimera_config::application::ChimeraAppConfigPatch {
            core: Some(clash_core),
            ..Default::default()
        })
        .await
    }

    pub(crate) async fn start_core(&self) -> anyhow::Result<()> {
        self.inner.shared.start_core().await
    }

    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        self.inner.shared.stop_core().await
    }

    pub(crate) async fn shutdown_core(&self) -> anyhow::Result<()> {
        self.inner.shared.shutdown_core().await
    }

    pub(crate) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.inner.shared.replace_core_binary(artifact).await
    }

    pub(crate) async fn stop_service(&self) -> anyhow::Result<()> {
        self.inner.reconciler().stop_service().await
    }

    pub(crate) async fn install_service(&self) -> anyhow::Result<()> {
        self.inner.reconciler().install_service().await
    }

    pub(crate) async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.inner.reconciler().uninstall_service().await
    }

    pub(crate) async fn update_service(&self) -> anyhow::Result<()> {
        self.inner.reconciler().update_service().await
    }

    pub(crate) async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.inner.reconciler().start_service(ready_timeout).await
    }

    pub(crate) async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.inner.reconciler().restart_service(ready_timeout).await
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

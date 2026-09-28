//! Profile mutation phases coordinated by the tracked application workflow.

use std::sync::Arc;
use std::time::Duration;

use chimera_config::profile::Profiles;
use chimera_core::state::{AbortResourceState, StateDecision};
use chimera_core_manager::{CoreError, CoreErrorKind, OperationId};

use super::{
    impact::{ActivationIntent, RuntimeImpact, classify_profiles},
    mutation::{DomainChange, MutationRequest, TryAck},
    policy::{CommandClass, CommandPolicy, CoreRunIntent, policy_for},
    workflow::{ApplicationWorkflow, ProfileRuntime},
};
use crate::client::runtime::RuntimeCommitStatus;

pub(super) const MUTATION_DECISION_TIMEOUT: Duration = Duration::from_secs(90);

enum DecisionOutcome {
    Committed,
    Aborted,
    Unresolved(String),
}

impl ApplicationWorkflow {
    /// Runs prepare, awaits the transaction's authoritative decision, and
    /// confirms or restores the Profile runtime before releasing the workflow.
    pub(super) async fn run_mutation(&mut self, request: &mut MutationRequest) {
        let operation_id = request.operation_id.clone();
        let (previous, candidate) = match &request.change {
            DomainChange::Profiles {
                previous,
                candidate,
            } => (
                previous.as_ref().map(|state| state.as_ref().clone()),
                candidate.as_ref().clone(),
            ),
        };
        let previous = previous.unwrap_or_default();
        let mut attempted_runtime = false;
        let mut deferred_runtime = false;

        let ack = if request.decision.decision() != StateDecision::Undecided {
            TryAck::Rejected("Profile source transaction settled before runtime admission".into())
        } else if let Err(error) = self.recover_previous(&previous).await {
            self.mark_recovery_required(error.to_string()).await;
            TryAck::Failed(error.to_string())
        } else {
            self.prepare_candidate(
                &operation_id,
                &previous,
                &candidate,
                request,
                &mut attempted_runtime,
                &mut deferred_runtime,
            )
            .await
        };
        request.answer(ack);

        let decision =
            match tokio::time::timeout(MUTATION_DECISION_TIMEOUT, request.decision.wait()).await {
                Ok(StateDecision::Committed { .. }) => DecisionOutcome::Committed,
                Ok(StateDecision::Aborted {
                    resources: AbortResourceState::Restored,
                }) => DecisionOutcome::Aborted,
                Ok(StateDecision::Aborted {
                    resources: AbortResourceState::NeedsRecovery(incident),
                }) => DecisionOutcome::Unresolved(format!(
                    "source transaction requires local recovery: {}",
                    incident.message
                )),
                Err(_) => DecisionOutcome::Unresolved(
                    "source transaction did not publish a decision within the workflow budget"
                        .into(),
                ),
                Ok(StateDecision::Undecided) => unreachable!("decision wait returned undecided"),
            };

        let settlement = match decision {
            DecisionOutcome::Committed => {
                if deferred_runtime {
                    self.core.request_runtime_rebuild();
                }
                Ok(())
            }
            DecisionOutcome::Aborted => {
                self.cancel_mutation(&operation_id, attempted_runtime, &previous)
                    .await
            }
            DecisionOutcome::Unresolved(message) => {
                self.mark_recovery_required(message.clone()).await;
                Err(message)
            }
        };
        request.settle(settlement);
    }

    async fn prepare_candidate(
        &self,
        operation_id: &OperationId,
        previous: &Profiles,
        candidate: &Profiles,
        request: &MutationRequest,
        attempted_runtime: &mut bool,
        deferred_runtime: &mut bool,
    ) -> TryAck {
        let impact = classify_profiles(previous, candidate, &request.hints);
        let explicit_switch = request.class == CommandClass::ExplicitSwitch
            || request.hints.activation != ActivationIntent::None;

        if !explicit_switch && impact == RuntimeImpact::None {
            self.set_outcome(operation_id, RuntimeCommitStatus::Unchanged)
                .await;
            return TryAck::Ok;
        }
        if self.core.status().uncertain {
            let message = "Profile runtime mutation cannot proceed while the core lifecycle outcome is uncertain";
            self.mark_recovery_required(message.into()).await;
            return TryAck::Failed(message.into());
        }

        let status = match self.core.core_status().await {
            Ok(status) => status,
            Err(error) => return TryAck::Failed(error.to_string()),
        };
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            self.set_outcome(operation_id, RuntimeCommitStatus::SavedInactive)
                .await;
            return TryAck::Ok;
        }

        let policy = policy_for(
            request.class,
            impact,
            &Default::default(),
            CoreRunIntent::Running,
        );
        if matches!(
            policy,
            CommandPolicy::SaveOnly | CommandPolicy::SaveThenNotify
        ) {
            self.set_outcome(operation_id, RuntimeCommitStatus::Unchanged)
                .await;
            return TryAck::Ok;
        }

        *attempted_runtime = true;
        match self
            .core
            .reconcile_profiles(
                Arc::new(candidate.clone()),
                request.hints.staged_content.clone(),
            )
            .await
        {
            Ok(()) => {
                self.set_outcome(operation_id, RuntimeCommitStatus::Applied)
                    .await;
                TryAck::Ok
            }
            Err(error) if policy.allows_deferral() && safe_to_defer_runtime_apply(&error) => {
                // QueueFull proves this candidate was not admitted to the core
                // executor. Retry after the Profile source transaction commits.
                *attempted_runtime = false;
                *deferred_runtime = true;
                self.set_outcome(operation_id, RuntimeCommitStatus::Deferred)
                    .await;
                TryAck::Degraded(format!(
                    "Profile was saved with runtime apply deferred: {error}"
                ))
            }
            Err(error) => TryAck::Failed(error.to_string()),
        }
    }

    async fn cancel_mutation(
        &self,
        operation_id: &OperationId,
        attempted_runtime: bool,
        previous: &Profiles,
    ) -> Result<(), String> {
        if !attempted_runtime {
            self.outcomes.lock().await.remove(operation_id);
            return Ok(());
        }

        match self
            .core
            .reconcile_profiles(Arc::new(previous.clone()), Default::default())
            .await
        {
            Ok(()) => {
                self.outcomes.lock().await.remove(operation_id);
                Ok(())
            }
            Err(error) => {
                let message = format!("failed to restore the previous Profile runtime: {error}");
                self.mark_recovery_required(message.clone()).await;
                self.outcomes.lock().await.remove(operation_id);
                Err(message)
            }
        }
    }

    async fn recover_previous(&self, previous: &Profiles) -> anyhow::Result<()> {
        let Some(cause) = self.recovery_required.lock().await.clone() else {
            return Ok(());
        };
        anyhow::ensure!(
            !self.core.status().uncertain,
            "Profile runtime still needs recovery after rollback failure: {cause}"
        );
        let status = self.core.core_status().await?;
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            *self.recovery_required.lock().await = None;
            return Ok(());
        }
        self.core
            .reconcile_profiles(Arc::new(previous.clone()), Default::default())
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "Profile runtime recovery failed after rollback error ({cause}): {error}"
                )
            })?;
        *self.recovery_required.lock().await = None;
        Ok(())
    }

    async fn set_outcome(&self, operation_id: &OperationId, status: RuntimeCommitStatus) {
        self.outcomes
            .lock()
            .await
            .insert(operation_id.clone(), status);
    }
}

fn safe_to_defer_runtime_apply(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::QueueFull) && error.retryable)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use camino::Utf8PathBuf;
    use chimera_config::profile::Profiles;
    use chimera_core::state::{
        PersistentStateManagerSetup, ReplaceIfVersionResult, StateParticipant,
    };
    use chimera_core_manager::OperationId;
    use tempfile::{TempDir, tempdir};
    use tokio::sync::{Mutex, oneshot};

    use crate::{
        client::{
            application_workflow::{
                ApplicationWorkflowClient, impact::MutationHints,
                participant::ApplicationMutationParticipant, policy::CommandClass,
            },
            core_lifecycle::{CoreLifecycleStatus, ports::CoreStatusSnapshot},
            runtime::RuntimeCommitStatus,
        },
        core::clash::core::RunType,
    };

    struct TestProfileRuntime {
        applied: Mutex<Vec<Profiles>>,
        block_first: AtomicBool,
        queue_full_first: AtomicBool,
        fail_restore: AtomicBool,
        rebuilds: AtomicUsize,
        started: StdMutex<Option<oneshot::Sender<()>>>,
        release: StdMutex<Option<oneshot::Receiver<()>>>,
    }

    impl TestProfileRuntime {
        fn new() -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                block_first: AtomicBool::new(false),
                queue_full_first: AtomicBool::new(false),
                fail_restore: AtomicBool::new(false),
                rebuilds: AtomicUsize::new(0),
                started: StdMutex::new(None),
                release: StdMutex::new(None),
            }
        }

        fn blocking(started: oneshot::Sender<()>, release: oneshot::Receiver<()>) -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                block_first: AtomicBool::new(true),
                queue_full_first: AtomicBool::new(false),
                fail_restore: AtomicBool::new(false),
                rebuilds: AtomicUsize::new(0),
                started: StdMutex::new(Some(started)),
                release: StdMutex::new(Some(release)),
            }
        }

        fn queue_full_first() -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                block_first: AtomicBool::new(false),
                queue_full_first: AtomicBool::new(true),
                fail_restore: AtomicBool::new(false),
                rebuilds: AtomicUsize::new(0),
                started: StdMutex::new(None),
                release: StdMutex::new(None),
            }
        }

        fn fail_restore() -> Self {
            Self {
                applied: Mutex::new(Vec::new()),
                block_first: AtomicBool::new(false),
                queue_full_first: AtomicBool::new(false),
                fail_restore: AtomicBool::new(true),
                rebuilds: AtomicUsize::new(0),
                started: StdMutex::new(None),
                release: StdMutex::new(None),
            }
        }
    }

    #[async_trait::async_trait]
    impl ProfileRuntime for TestProfileRuntime {
        fn status(&self) -> CoreLifecycleStatus {
            CoreLifecycleStatus::default()
        }

        async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: chimera_ipc::api::status::CoreState::Running,
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        async fn reconcile_profiles(
            &self,
            profiles: Arc<Profiles>,
            _staged_content: std::collections::BTreeMap<String, String>,
        ) -> anyhow::Result<()> {
            let call = {
                let mut applied = self.applied.lock().await;
                applied.push(profiles.as_ref().clone());
                applied.len()
            };
            if self.block_first.swap(false, Ordering::AcqRel) {
                if let Some(started) = self
                    .started
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    let _ = started.send(());
                }
                let release = self
                    .release
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(release) = release {
                    release
                        .await
                        .map_err(|_| anyhow::anyhow!("test release dropped"))?;
                }
            }
            if call == 1 && self.queue_full_first.swap(false, Ordering::AcqRel) {
                return Err(anyhow::Error::new(CoreError::new(
                    CoreErrorKind::QueueFull,
                    "executor queue is full",
                    true,
                )));
            }
            if call > 1 && self.fail_restore.load(Ordering::Acquire) {
                anyhow::bail!("injected Profile runtime restore failure");
            }
            Ok(())
        }

        fn request_runtime_rebuild(&self) {
            self.rebuilds.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn profile_manager() -> (
        chimera_core::state::PersistentStateManager<
            Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >,
        TempDir,
    ) {
        let directory = tempdir().expect("temporary Profile directory");
        let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
            .expect("UTF-8 temporary Profile path");
        let manager = PersistentStateManagerSetup::<
            Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(Profiles::default())
        .await
        .expect("Profile state manager");
        (manager, directory)
    }

    async fn workflow(
        runtime: Arc<TestProfileRuntime>,
    ) -> (
        ApplicationWorkflowClient,
        Arc<Mutex<std::collections::HashMap<OperationId, RuntimeCommitStatus>>>,
        Arc<Mutex<Option<String>>>,
    ) {
        let outcomes = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let recovery_required = Arc::new(Mutex::new(None));
        let workflow = ApplicationWorkflowClient::spawn_with_runtime(
            runtime,
            outcomes.clone(),
            recovery_required.clone(),
        )
        .await
        .expect("application workflow actor");
        (workflow, outcomes, recovery_required)
    }

    fn candidate_profiles() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.valid.push("test-runtime-field".into());
        profiles
    }

    fn participant(
        operation_id: OperationId,
        decision: chimera_core::state::DecisionHandle,
        workflow: ApplicationWorkflowClient,
        ack_timeout: Duration,
    ) -> StateParticipant<Profiles> {
        ApplicationMutationParticipant::<Profiles>::with_ack_timeout(
            operation_id,
            MutationHints::default(),
            CommandClass::Save,
            decision,
            workflow,
            ack_timeout,
        )
    }

    #[test]
    fn runtime_deferral_requires_a_retryable_queue_full_error() {
        let queue_full = anyhow::Error::new(CoreError::new(
            CoreErrorKind::QueueFull,
            "executor queue is full",
            true,
        ));
        assert!(safe_to_defer_runtime_apply(&queue_full));

        let non_retryable_queue_full = anyhow::Error::new(CoreError::new(
            CoreErrorKind::QueueFull,
            "queue full without retryability",
            false,
        ));
        assert!(!safe_to_defer_runtime_apply(&non_retryable_queue_full));

        let other_retryable_error = anyhow::Error::new(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "backend unavailable",
            true,
        ));
        assert!(!safe_to_defer_runtime_apply(&other_retryable_error));

        let untyped = anyhow::anyhow!("executor queue is full");
        assert!(!safe_to_defer_runtime_apply(&untyped));
    }

    #[tokio::test]
    async fn committed_profile_mutation_settles_after_runtime_apply() {
        let (mut manager, _directory) = profile_manager().await;
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::new());
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("Profile source transaction");

        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            candidate.valid
        );
        let applied = runtime.applied.lock().await;
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].valid, candidate.valid);
        assert_eq!(
            outcomes.lock().await.get(&operation_id),
            Some(&RuntimeCommitStatus::Applied)
        );
    }

    #[tokio::test]
    async fn aborted_profile_mutation_restores_previous_runtime() {
        let (mut manager, _directory) = profile_manager().await;
        let previous = Profiles::default();
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::new());
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { anyhow::bail!("force source-side failure after prepare") },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert_eq!(manager.snapshot_handle().load().state.valid, previous.valid);
        let applied = runtime.applied.lock().await;
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].valid, candidate.valid);
        assert_eq!(applied[1].valid, previous.valid);
        assert!(!outcomes.lock().await.contains_key(&operation_id));
    }

    #[tokio::test]
    async fn safe_queue_full_commits_and_schedules_runtime_retry() {
        let (mut manager, _directory) = profile_manager().await;
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::queue_full_first());
        let (workflow, outcomes, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .expect("safe runtime deferral should commit Profile state");

        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.snapshot_handle().load().state.valid,
            candidate.valid
        );
        assert_eq!(
            outcomes.lock().await.get(&operation_id),
            Some(&RuntimeCommitStatus::Deferred)
        );
        assert_eq!(runtime.rebuilds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_profile_rollback_latches_recovery_outcome() {
        let (mut manager, _directory) = profile_manager().await;
        let previous = Profiles::default();
        let candidate = candidate_profiles();
        let runtime = Arc::new(TestProfileRuntime::fail_restore());
        let (workflow, outcomes, recovery_required) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let version = manager.snapshot_handle().load().version;

        let result = manager
            .replace_if_version_with_participant(
                version,
                candidate,
                {
                    let workflow = workflow.clone();
                    move |decision| {
                        participant(operation_id, decision, workflow, Duration::from_secs(1))
                    }
                },
                || async { anyhow::bail!("force source-side failure after prepare") },
                || async { Ok(()) },
            )
            .await;

        assert!(result.is_err());
        assert_eq!(manager.snapshot_handle().load().state.valid, previous.valid);
        assert!(!outcomes.lock().await.contains_key(&operation_id));
        assert!(recovery_required.lock().await.is_some());
    }

    #[tokio::test]
    async fn timed_out_prepare_waits_for_try_then_restores_baseline() {
        let (manager, _directory) = profile_manager().await;
        let previous = Profiles::default();
        let candidate = candidate_profiles();
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let runtime = Arc::new(TestProfileRuntime::blocking(started_tx, release_rx));
        let (workflow, _, _) = workflow(runtime.clone()).await;
        let operation_id = OperationId::generate();
        let expected_candidate = candidate.clone();
        let version = manager.snapshot_handle().load().version;

        let transaction = tokio::spawn({
            let workflow = workflow.clone();
            async move {
                let mut manager = manager;
                let result = manager
                    .replace_if_version_with_participant(
                        version,
                        candidate.clone(),
                        move |decision| {
                            ApplicationMutationParticipant::<Profiles>::with_ack_timeout(
                                operation_id,
                                MutationHints::default(),
                                CommandClass::Save,
                                decision,
                                workflow,
                                Duration::from_millis(20),
                            )
                        },
                        || async { Ok(()) },
                        || async { Ok(()) },
                    )
                    .await;
                (manager, result)
            }
        });

        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("candidate Try should start")
            .expect("candidate Try start notification");
        tokio::time::sleep(Duration::from_millis(60)).await;
        release_tx.send(()).expect("release runtime Try");
        let (manager, result) = tokio::time::timeout(Duration::from_secs(2), transaction)
            .await
            .expect("transaction should settle after cancellation")
            .expect("source transaction task");

        assert!(result.is_err());
        assert_eq!(manager.snapshot_handle().load().state.valid, previous.valid);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime.applied.lock().await.len() == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("tracked workflow should finish rollback after its waiter timed out");
        let applied = runtime.applied.lock().await;
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].valid, expected_candidate.valid);
        assert_eq!(applied[1].valid, previous.valid);
    }
}

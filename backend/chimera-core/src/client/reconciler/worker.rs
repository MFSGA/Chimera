//! Serialized worker execution for the single reconciler mailbox.
//! The host supplies lifecycle execution and observed evidence. Completion
//! still belongs to the mailbox and must pass its latest-source fence.
use super::mailbox::{OperationId, Request, Response};
use crate::{
    actor_v2::service_policy::ServiceRestartPolicySnapshot,
    client::core_lifecycle::{
        RECOVERY_INTERVAL,
        ports::{BinaryInstallProgress, ServiceLifecyclePort, ServiceTransitionLease},
    },
};
use ractor::{ActorRef, rpc::CallResult};
use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

pub const CALL_WAIT: Duration = Duration::from_secs(180);
const STARTUP_RECOVERY_LEASE_ATTEMPTS: usize = 3;

pub struct WorkerPlan {
    pub id: OperationId,
    pub shutdown: bool,
    pub deferred_retry: Option<String>,
    pub reconciled: bool,
    pub runtime_converged: Option<Arc<AtomicBool>>,
    pub service_mutation: bool,
    pub publish_legacy_service: bool,
    pub retry_reconcile_on_failure: bool,
    pub recover: bool,
    pub service_endpoint_recovery: bool,
    pub progress: Option<Arc<dyn BinaryInstallProgress>>,
}

pub struct WorkerEvidence {
    pub lower_outcome_uncertain: bool,
    pub startup_owner_unproven: bool,
    pub service_restart_policy: ServiceRestartPolicySnapshot,
}

pub struct WorkerResult {
    pub result: anyhow::Result<()>,
    pub workflow_panicked: bool,
    pub request_runtime_rebuild: bool,
}

pub struct WorkerCompletion<W> {
    pub id: OperationId,
    pub workflow: W,
    pub result: anyhow::Result<()>,
    pub workflow_panicked: bool,
    pub lower_outcome_uncertain: bool,
    pub startup_owner_unproven: bool,
    pub service_restart_policy: ServiceRestartPolicySnapshot,
    pub service_probe: Option<Result<chimera_ipc::types::StatusInfo<'static>, String>>,
    pub retry_reconcile_on_failure: bool,
    pub recover: bool,
    pub service_endpoint_recovery: bool,
    pub shutdown: bool,
    pub request_runtime_rebuild: bool,
    pub deferred_retry: Option<String>,
    pub reconciled: bool,
}

#[async_trait::async_trait]
pub trait WorkerWorkflow<C: Send + 'static>: Send + Sync + 'static {
    async fn execute_lifecycle_command(&mut self, command: C) -> anyhow::Result<bool>;
    async fn recover_worker_panic(&mut self, message: String);
    fn observe_worker(&self) -> WorkerEvidence;
    async fn probe_worker_service(&self)
    -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>>;

    async fn run_command(&mut self, command: C) -> WorkerResult {
        run_lifecycle(self, command).await
    }
}

pub async fn run_lifecycle<C: Send + 'static, W: WorkerWorkflow<C> + ?Sized>(
    workflow: &mut W,
    command: C,
) -> WorkerResult {
    // The recoverable panic flag covers each poll, not a whole Tokio await.
    let (result, workflow_panicked, request_runtime_rebuild) =
        match chimera_panics::catch_recoverable_future(workflow.execute_lifecycle_command(command))
            .await
        {
            Ok(Ok(rebuild)) => (Ok(()), false, rebuild),
            Ok(Err(error)) => (Err(error), false, false),
            Err(_) => {
                let message = "core lifecycle workflow panicked; execution state is uncertain";
                workflow.recover_worker_panic(message.into()).await;
                (Err(anyhow::anyhow!(message)), true, false)
            }
        };
    WorkerResult {
        result,
        workflow_panicked,
        request_runtime_rebuild,
    }
}

pub fn finish_binary_progress(progress: &dyn BinaryInstallProgress, error: Option<&str>) {
    // An observer panic must not change a verified lifecycle result.
    if chimera_panics::catch_recoverable(AssertUnwindSafe(|| progress.finished(error))).is_err() {
        tracing::error!("binary installation progress observer panicked");
    }
}

pub async fn execute_worker<C: Send + 'static, W: WorkerWorkflow<C>>(
    mut workflow: W,
    command: C,
    plan: WorkerPlan,
) -> WorkerCompletion<W> {
    let WorkerResult {
        result,
        workflow_panicked,
        request_runtime_rebuild,
    } = workflow.run_command(command).await;
    if let Some(progress) = plan.progress {
        let error = result.as_ref().err().map(ToString::to_string);
        finish_binary_progress(progress.as_ref(), error.as_deref());
    }
    let WorkerEvidence {
        lower_outcome_uncertain,
        startup_owner_unproven,
        service_restart_policy,
    } = workflow.observe_worker();
    // Ref ServiceActor publishes its own bounded probe. Only fixture ports
    // without that owner need this compatibility observation.
    let service_probe = if plan.service_mutation
        && !workflow_panicked
        && plan.publish_legacy_service
    {
        Some(
            match chimera_panics::catch_recoverable_future(workflow.probe_worker_service()).await {
                Ok(Ok(info)) => Ok(info),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("service status probe panicked".to_string()),
            },
        )
    } else {
        None
    };
    WorkerCompletion {
        id: plan.id,
        workflow,
        result,
        workflow_panicked,
        lower_outcome_uncertain,
        startup_owner_unproven,
        service_restart_policy,
        service_probe,
        retry_reconcile_on_failure: plan.retry_reconcile_on_failure,
        recover: plan.recover,
        service_endpoint_recovery: plan.service_endpoint_recovery,
        shutdown: plan.shutdown,
        request_runtime_rebuild,
        deferred_retry: plan.deferred_retry,
        reconciled: plan.reconciled
            && plan
                .runtime_converged
                .as_ref()
                .is_none_or(|converged| converged.load(Ordering::Acquire)),
    }
}

pub fn spawn_worker<C: Send + 'static, W: WorkerWorkflow<C>, M: ractor::Message>(
    actor: &ActorRef<M>,
    workflow: W,
    command: C,
    plan: WorkerPlan,
    completed: impl FnOnce(WorkerCompletion<W>) -> M + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    let actor = actor.clone();
    let id = plan.id;
    tokio::spawn(async move {
        let completion = execute_worker(workflow, command, plan).await;
        if actor.cast(completed(completion)).is_err() {
            tracing::error!(
                id,
                "core lifecycle actor stopped before operation completion"
            );
        }
    })
}

struct PendingRecovery(Arc<AtomicBool>);
impl Drop for PendingRecovery {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub fn schedule_startup_recovery<M: ractor::Message, C: Send + 'static>(
    actor_ref: ActorRef<M>,
    next_id: Arc<AtomicU64>,
    service: Arc<dyn ServiceLifecyclePort>,
    startup_recovery_pending: Arc<AtomicBool>,
    is_needed: impl Fn() -> bool + Send + Sync + 'static,
    command: impl FnOnce(u64, Box<dyn ServiceTransitionLease>) -> C + Send + 'static,
    request: impl FnOnce(Request<C>) -> M + Send + 'static,
    finished: impl FnOnce(bool) -> M + Send + 'static,
) {
    if startup_recovery_pending
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let pending_guard = PendingRecovery(startup_recovery_pending);
    tokio::spawn(async move {
        let result = async {
            for attempt in 0..STARTUP_RECOVERY_LEASE_ATTEMPTS {
                // Acquire the host lease before asking the serialized actor to
                // run recovery, preserving the global host-lock/mailbox order.
                let transition = match service.begin_transition().await {
                    Ok(transition) => transition,
                    Err(error) if attempt + 1 < STARTUP_RECOVERY_LEASE_ATTEMPTS => {
                        tracing::debug!(%error, "startup owner recovery lease busy; retrying");
                        tokio::time::sleep(RECOVERY_INTERVAL).await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if !is_needed() {
                    return Ok(());
                }
                let operation_id = next_id.fetch_add(1, Ordering::Relaxed);
                return match actor_ref
                    .call(
                        |reply| {
                            request(Request {
                                command: command(operation_id, transition),
                                response: Response {
                                    id: operation_id,
                                    reply: Some(reply),
                                },
                                deferred_retry: None,
                            })
                        },
                        Some(CALL_WAIT),
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::Timeout => Err(anyhow::anyhow!("startup owner recovery timed out")),
                    CallResult::SenderError => {
                        Err(anyhow::anyhow!("startup owner recovery reply was dropped"))
                    }
                };
            }
            unreachable!("startup recovery lease retry loop always returns or exhausts")
        }
        .await;
        drop(pending_guard);
        // A source commit may reach the mailbox after RecoverStartup has
        // settled but before the caller's source observation. Only a
        // successfully resolved Startup dependency can wake its retained
        // source: a lease failure or timed-out recovery grants no authority
        // to proceed, even if the shared owner flag changed meanwhile.
        let _ = actor_ref.cast(finished(result.is_ok()));
        if let Err(error) = result {
            tracing::warn!(%error, "automatic startup owner recovery did not complete");
        }
    });
}

/// Lease acquisition must run outside the mailbox: an already queued
/// lifecycle operation may still own the lease. Only the original mailbox
/// can admit the command after acquisition.
pub fn spawn_service_recovery_lease<P: Send + 'static, M: ractor::Message>(
    actor: ActorRef<M>,
    service: Arc<dyn ServiceLifecyclePort>,
    operation_id: OperationId,
    request_message: impl FnOnce(Request<crate::client::core_lifecycle::command::Command<P>>) -> M
    + Send
    + 'static,
    failed: impl FnOnce() -> M + Send + 'static,
) {
    use crate::client::core_lifecycle::command::Command;
    tokio::spawn(async move {
        match service.begin_transition().await {
            Ok(transition) => {
                let request = Request {
                    command: Command::ServiceEndpointDown(transition),
                    response: Response {
                        id: operation_id,
                        reply: None,
                    },
                    deferred_retry: None,
                };
                if actor.cast(request_message(request)).is_err() {
                    let _ = actor.cast(failed());
                }
            }
            Err(error) => {
                tracing::debug!(%error, "Service recovery retry deferred while acquiring its transition lease");
                let _ = actor.cast(failed());
            }
        }
    });
}

pub fn spawn_startup_retry_lease<P: Send + 'static, M: ractor::Message>(
    actor: ActorRef<M>,
    service: Arc<dyn ServiceLifecyclePort>,
    operation_id: OperationId,
    identity: String,
    request_message: impl FnOnce(Request<crate::client::core_lifecycle::command::Command<P>>) -> M
    + Send
    + 'static,
    failed: impl FnOnce(String, String) -> M + Send + 'static,
) {
    use crate::client::core_lifecycle::command::Command;
    tokio::spawn(async move {
        match service.begin_transition().await {
            Ok(transition) => {
                let request = Request {
                    command: Command::RetryStartup {
                        operation_id,
                        identity,
                        transition,
                    },
                    response: Response {
                        id: operation_id,
                        reply: None,
                    },
                    deferred_retry: None,
                };
                if actor.cast(request_message(request)).is_err() {
                    tracing::warn!(operation_id, "failed to enqueue startup retry");
                }
            }
            Err(error) => {
                let _ = actor.cast(failed(
                    identity,
                    format!("could not acquire the Service transition lease: {error:#}"),
                ));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_ipc::types::{ServiceStatus, StatusInfo};
    use std::{borrow::Cow, sync::atomic::AtomicUsize};

    enum Command {
        Success,
        Refused,
        Panic,
        PendingSource,
    }
    #[derive(Default)]
    struct Fixture {
        calls: usize,
        probes: AtomicUsize,
        panic_reason: Option<String>,
        probe_panics: bool,
    }

    #[async_trait::async_trait]
    impl WorkerWorkflow<Command> for Fixture {
        async fn execute_lifecycle_command(&mut self, command: Command) -> anyhow::Result<bool> {
            self.calls += 1;
            tokio::task::yield_now().await;
            match command {
                Command::Success | Command::PendingSource => Ok(true),
                Command::Refused => anyhow::bail!("known refusal"),
                Command::Panic => panic!("test lifecycle panic"),
            }
        }
        async fn recover_worker_panic(&mut self, reason: String) {
            self.panic_reason = Some(reason);
        }
        fn observe_worker(&self) -> WorkerEvidence {
            WorkerEvidence {
                lower_outcome_uncertain: self.panic_reason.is_some(),
                startup_owner_unproven: self.panic_reason.is_some(),
                service_restart_policy: ServiceRestartPolicySnapshot {
                    attempts: 2,
                    exhausted: false,
                },
            }
        }
        async fn probe_worker_service(&self) -> anyhow::Result<StatusInfo<'static>> {
            self.probes.fetch_add(1, Ordering::Relaxed);
            assert!(!self.probe_panics, "test probe panic");
            Ok(StatusInfo {
                name: Cow::Borrowed("test"),
                version: Cow::Borrowed("1"),
                status: ServiceStatus::Stopped,
                server: None,
            })
        }
    }

    struct PanicProgress;
    impl BinaryInstallProgress for PanicProgress {
        fn restarting(&self) {}
        fn finished(&self, _: Option<&str>) {
            panic!("test observer panic");
        }
    }

    fn plan() -> WorkerPlan {
        WorkerPlan {
            id: 42,
            shutdown: false,
            deferred_retry: None,
            reconciled: true,
            runtime_converged: None,
            service_mutation: true,
            publish_legacy_service: false,
            retry_reconcile_on_failure: false,
            recover: false,
            service_endpoint_recovery: false,
            progress: None,
        }
    }

    #[tokio::test]
    async fn native_service_owner_is_not_probed_again_and_observer_panic_does_not_change_result() {
        let mut p = plan();
        p.progress = Some(Arc::new(PanicProgress));
        let completed = execute_worker(Fixture::default(), Command::Success, p).await;
        assert_eq!(completed.id, 42);
        assert!(completed.result.is_ok());
        assert!(completed.reconciled && completed.request_runtime_rebuild);
        assert!(!completed.workflow_panicked && !completed.lower_outcome_uncertain);
        assert_eq!(completed.workflow.calls, 1);
        assert_eq!(completed.workflow.probes.load(Ordering::Relaxed), 0);
        assert!(completed.service_probe.is_none());
        assert_eq!(completed.service_restart_policy.attempts, 2);
    }

    #[tokio::test]
    async fn known_refusal_remains_known_and_legacy_probe_panic_is_only_an_observation_error() {
        let mut p = plan();
        p.publish_legacy_service = true;
        let completed = execute_worker(
            Fixture {
                probe_panics: true,
                ..Default::default()
            },
            Command::Refused,
            p,
        )
        .await;
        assert_eq!(completed.result.unwrap_err().to_string(), "known refusal");
        assert!(!completed.workflow_panicked && !completed.lower_outcome_uncertain);
        assert!(!completed.request_runtime_rebuild);
        assert_eq!(completed.workflow.probes.load(Ordering::Relaxed), 1);
        assert_eq!(
            completed.service_probe.unwrap().unwrap_err(),
            "service status probe panicked"
        );
    }

    #[tokio::test]
    async fn lifecycle_panic_preserves_recovery_evidence_and_skips_service_probe() {
        let mut p = plan();
        p.publish_legacy_service = true;
        let completed = execute_worker(Fixture::default(), Command::Panic, p).await;
        assert!(completed.result.is_err());
        assert!(
            completed.workflow_panicked
                && completed.lower_outcome_uncertain
                && completed.startup_owner_unproven
        );
        assert!(!completed.request_runtime_rebuild);
        assert!(
            completed
                .workflow
                .panic_reason
                .unwrap()
                .contains("uncertain")
        );
        assert_eq!(completed.workflow.probes.load(Ordering::Relaxed), 0);
        assert!(completed.service_probe.is_none());
    }

    #[tokio::test]
    async fn source_only_success_cannot_claim_runtime_convergence() {
        let mut p = plan();
        p.runtime_converged = Some(Arc::new(AtomicBool::new(false)));
        p.publish_legacy_service = true;
        let completed = execute_worker(Fixture::default(), Command::PendingSource, p).await;
        assert!(completed.result.is_ok());
        assert!(!completed.reconciled);
        assert_eq!(
            completed.service_probe.unwrap().unwrap().status,
            ServiceStatus::Stopped
        );
        assert_eq!(completed.workflow.probes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn recovery_admission_guard_clears_when_abandoned() {
        let pending = Arc::new(AtomicBool::new(true));
        drop(PendingRecovery(pending.clone()));
        assert!(!pending.load(Ordering::Acquire));
    }
}

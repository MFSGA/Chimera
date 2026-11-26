//! Shared mailbox protocol and actor client state for the single reconciler.
#[cfg(any(test, feature = "test-support"))]
use super::committed::SourceVersions;
use super::{
    checked::CheckedRuntimeCandidate,
    committed::SourceDomain,
    mailbox::{CoreLifecycleStatus, Request, Response},
    startup_policy::StartupReport,
    status::ReconcilerStatus,
    worker::{CALL_WAIT, WorkerCompletion},
};
use crate::{
    actor_v2::service_status::ServiceHostStatus,
    client::{
        app_lifecycle::CompletedLifecycleOperation,
        core_lifecycle::{
            command::Command,
            ports::{CoreLifecyclePort, PreparedRuntime, ServiceLifecyclePort},
            status::CoreStatusSnapshot,
        },
    },
};
use chimera_contracts::runtime_error::RuntimeError;
use ractor::{ActorRef, RpcReplyPort, rpc::CallResult};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

pub enum Message<W, P> {
    /// Pinned ref RuntimeReconciler::Message::Checked, kept in the existing
    /// single CoreLifecycle mailbox until its prepared-apply path is ported.
    Checked {
        checked: CheckedRuntimeCandidate,
        reply: RpcReplyPort<Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>>>,
    },
    /// A checked candidate was never persisted; revoke its domain before
    /// the next durable SourceCommitted can consume stale uncommitted bytes.
    AbortChecked {
        domain: SourceDomain,
        request: Option<String>,
    },
    /// Install the single production ref CandidateCheck on the existing
    /// lifecycle workflow before any Application source mutation can run.
    InstallCandidateGate {
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ConnectApplicationWorkflow {
        workflow: W,
        reply: RpcReplyPort<anyhow::Result<()>>,
        registered: Option<tokio::sync::oneshot::Sender<()>>,
    },
    Request(Request<Command<P>>),
    Completed {
        completion: WorkerCompletion<W>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ProbeService {
        reply: RpcReplyPort<anyhow::Result<ServiceHostStatus>>,
    },
    ObserveService {
        info: chimera_ipc::types::StatusInfo<'static>,
    },
    ServiceReadiness,
    ServiceProbeFailed,
    ServiceRecoveryTick,
    ServiceRecoveryLeaseFailed,
    RecoverCore,
    /// Ref RuntimeGate::committed: observe the durable source version.
    /// Profiles requests run a latest-target reconciliation here, while
    /// already-applied legacy domains may only record their source version.
    SourceCommitted {
        domain: SourceDomain,
        version: u64,
        reconcile_requested: bool,
        /// The caller already observed a ref Service generation / owner that
        /// requires Startup recovery. Retain the durable runtime intent while
        /// that single owner is being re-established.
        startup_recovery_required: bool,
    },
    /// Ref RuntimeGate::committed for an already Checked product. Unlike the
    /// old SourceCommitted route, this must NEVER schedule an independent
    /// legacy build for a candidate it successfully consumes.
    CheckedSourceCommitted {
        domain: SourceDomain,
        version: u64,
        request: Option<String>,
    },
    RuntimeDirty,
    DirtyTick,
    /// A committed checked value can only be dispatched from the sole Owner
    /// mailbox after observing the matching source version. Not connected to
    /// the production Source Gate until the authoritative latest-target read
    /// is also integrated.
    ApplyCheckedCommitted {
        domain: SourceDomain,
        version: u64,
        target: String,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ReadSourceVersionsForTest {
        reply: RpcReplyPort<SourceVersions>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ReadCheckedForTest {
        reply: RpcReplyPort<Option<(Option<String>, String, Vec<u8>)>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ReadAppliedSelectionForTest {
        reply: RpcReplyPort<Option<crate::client::reconciler::committed::Selection>>,
    },
    StartupRecoveryFinished {
        succeeded: bool,
    },
    StartupRetryDue {
        identity: String,
    },
    StartupRetryLeaseFailed {
        identity: String,
        reason: String,
        observer: Option<tokio::sync::oneshot::Sender<()>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ArmStartupRetryLeaseFailureObserver {
        observer: tokio::sync::oneshot::Sender<()>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ReadStartupRetryScheduleForTest {
        reply: RpcReplyPort<Option<(String, tokio::time::Instant)>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    SeedUnresolvedApplicationAttemptForTest {
        reply: RpcReplyPort<anyhow::Result<chimera_core_manager::OperationId>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    SeedUnresolvedSourceDecisionForTest {
        decision: crate::state::DecisionHandle,
        reply: RpcReplyPort<anyhow::Result<chimera_core_manager::OperationId>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    SeedUnresolvedAttemptWithDeferredRetryForTest {
        decision: crate::state::DecisionHandle,
        reply: RpcReplyPort<anyhow::Result<(chimera_core_manager::OperationId, String)>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    SeedUnresolvedCommittedTargetForTest {
        decision: crate::state::DecisionHandle,
        reply: RpcReplyPort<anyhow::Result<chimera_core_manager::OperationId>>,
    },
    #[cfg(any(test, feature = "test-support"))]
    ReadApplicationRecoveryStateForTest {
        reply: RpcReplyPort<(
            Option<(chimera_core_manager::OperationId, bool)>,
            Option<(String, tokio::time::Instant)>,
            Option<(u8, u32)>,
        )>,
    },
    DeferredRetry {
        digest: String,
    },
    ReadStartupReport {
        reply: RpcReplyPort<Option<StartupReport>>,
    },
}

pub struct ActorClient<W, P> {
    pub actor_ref: ActorRef<Message<W, P>>,
    pub next_id: Arc<AtomicU64>,
    pub status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    pub reconciler: tokio::sync::watch::Receiver<ReconcilerStatus>,
    pub service_status: tokio::sync::watch::Receiver<ServiceHostStatus>,
    pub service: Arc<dyn ServiceLifecyclePort>,
    pub core: Arc<dyn CoreLifecyclePort>,
    pub startup_owner_unproven: Arc<AtomicBool>,
    pub service_failed_generation: Arc<AtomicU64>,
    pub startup_recovery_pending: Arc<AtomicBool>,
    /// The source verifier comes only from the composition root's
    /// committed Application, Clash and Profiles snapshots plus files.
    pub source_authority:
        Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
}

fn completed_from_startup_report(report: StartupReport) -> CompletedLifecycleOperation {
    let result = report
        .outcome
        .failure_reason()
        .map_or(Ok(()), |reason| Err(anyhow::anyhow!(reason.to_owned())));
    CompletedLifecycleOperation { result, report }
}

impl<W: Send + 'static, P: Send + 'static> ActorClient<W, P> {
    pub async fn execute(&self, command: Command<P>) -> anyhow::Result<()> {
        self.execute_with_timeout(command, CALL_WAIT).await
    }
    pub async fn install_candidate_gate(
        &self,
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
    ) -> anyhow::Result<()> {
        let Self { actor_ref, .. } = self;

        match actor_ref
            .call(
                |reply| Message::InstallCandidateGate { gate, reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("candidate gate installation timed out"),
            CallResult::SenderError => {
                anyhow::bail!("candidate gate installation reply dropped")
            }
        }
    }

    pub async fn execute_with_timeout(
        &self,
        command: Command<P>,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        let Self {
            actor_ref, next_id, ..
        } = self;

        let id = next_id.fetch_add(1, Ordering::Relaxed);
        match actor_ref
            .call(
                |reply| {
                    Message::Request(Request {
                        command,
                        response: Response {
                            id,
                            reply: Some(reply),
                        },
                        deferred_retry: None,
                    })
                },
                Some(timeout),
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::Timeout) => anyhow::bail!(
                "core lifecycle operation {id} timed out; it may still be queued or running; inspect get_core_lifecycle_status before retrying"
            ),
            Ok(CallResult::SenderError) => {
                anyhow::bail!(
                    "core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                )
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn cached_startup_report(&self) -> anyhow::Result<Option<StartupReport>> {
        let Self { actor_ref, .. } = self;

        match actor_ref
            .call(
                |reply| Message::ReadStartupReport { reply },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(report) => Ok(report),
            CallResult::Timeout => anyhow::bail!("startup report query timed out"),
            CallResult::SenderError => {
                anyhow::bail!("startup report query reply was dropped")
            }
        }
    }

    pub async fn startup_reconcile_with_timeout(
        &self,
        timeout: Option<Duration>,
    ) -> anyhow::Result<CompletedLifecycleOperation> {
        let Self {
            actor_ref,
            next_id,
            service,
            ..
        } = self;

        if let Some(report) = self.cached_startup_report().await? {
            return Ok(completed_from_startup_report(report));
        }
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        let transition = match service.begin_transition().await {
            Ok(transition) => transition,
            Err(error) => {
                let reason = format!("could not acquire the Service transition lease: {error:#}");
                let cache_id = next_id.fetch_add(1, Ordering::Relaxed);
                let _ = actor_ref
                    .call(
                        |reply| {
                            Message::Request(Request {
                                command: Command::CacheStartupUnsettled {
                                    operation_id: id,
                                    reason,
                                },
                                response: Response {
                                    id: cache_id,
                                    reply: Some(reply),
                                },
                                deferred_retry: None,
                            })
                        },
                        timeout,
                    )
                    .await?;
                return self
                    .cached_startup_report()
                    .await?
                    .map(completed_from_startup_report)
                    .ok_or_else(|| anyhow::anyhow!("startup failure report was not cached"));
            }
        };
        if let Some(report) = self.cached_startup_report().await? {
            return Ok(completed_from_startup_report(report));
        }
        let result = match actor_ref
            .call(
                |reply| {
                    Message::Request(Request {
                        command: Command::StartupReconcile {
                            operation_id: id,
                            transition,
                        },
                        response: Response {
                            id,
                            reply: Some(reply),
                        },
                        deferred_retry: None,
                    })
                },
                timeout,
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::Timeout) => anyhow::bail!(
                "startup core lifecycle operation {id} timed out; it may still be queued or running"
            ),
            Ok(CallResult::SenderError) => anyhow::bail!(
                "startup core lifecycle actor reply dropped for operation {id}; outcome is unknown"
            ),
            Err(error) => Err(error.into()),
        };
        let report = match self.cached_startup_report().await? {
            Some(report) => report,
            None => {
                let reason = result
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "startup lifecycle completed without a report".into());
                let cache_id = next_id.fetch_add(1, Ordering::Relaxed);
                let _ = actor_ref
                    .call(
                        |reply| {
                            Message::Request(Request {
                                command: Command::CacheStartupUnsettled {
                                    operation_id: id,
                                    reason,
                                },
                                response: Response {
                                    id: cache_id,
                                    reply: Some(reply),
                                },
                                deferred_retry: None,
                            })
                        },
                        Some(CALL_WAIT),
                    )
                    .await?;
                self.cached_startup_report()
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("startup failure report was not cached"))?
            }
        };
        Ok(CompletedLifecycleOperation { result, report })
    }

    pub fn startup_recovery_needed(&self) -> bool {
        let Self {
            startup_owner_unproven,
            service_failed_generation,
            service_status,
            service,
            core,
            ..
        } = self;
        super::service_observer::startup_recovery_needed(
            core.as_ref(),
            service.as_ref(),
            startup_owner_unproven,
            service_failed_generation,
            service_status,
        )
    }

    pub fn runtime_preparation_session(
        &self,
    ) -> Option<crate::client::ports::RuntimePreparationSession> {
        let Self { core, .. } = self;
        core.runtime_preparation_session()
    }

    pub fn status(&self) -> CoreLifecycleStatus {
        let Self { status, .. } = self;
        status.lock().clone()
    }

    pub fn reconciler_status(&self) -> ReconcilerStatus {
        let Self { reconciler, .. } = self;
        reconciler.borrow().clone()
    }

    pub async fn checked_candidate_inner(
        &self,
        domain: SourceDomain,
        prepared: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: Option<String>,
        interrupt_mode: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        let Self { actor_ref, .. } = self;

        let operation_id = chimera_core_manager::OperationId::generate();
        match actor_ref
            .call(
                |reply| Message::Checked {
                    checked: CheckedRuntimeCandidate {
                        domain,
                        request,
                        interrupt_mode,
                        prepared,
                        core_spec,
                    },
                    reply,
                },
                None,
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            // The message never entered the Actor, so no candidate
            // could have been accepted.
            Err(ractor::MessagingErr::SendErr(Message::Checked { .. })) => {
                Err(Arc::new(RuntimeError::ShuttingDown))
            }
            // A delivered request without an ACK may have been
            // accepted. Do not turn an uncertain ownership outcome
            // into a definitely closed owner.
            _ => Err(Arc::new(RuntimeError::OwnerUnresponsive {
                operation_id: operation_id.to_string(),
            })),
        }
    }

    pub fn abort_checked_inner(&self, domain: SourceDomain, request: Option<String>) {
        let Self { actor_ref, .. } = self;

        if actor_ref
            .cast(Message::AbortChecked { domain, request })
            .is_err()
        {
            tracing::debug!(?domain, "checked abort skipped: Core owner is unavailable");
        }
    }

    pub fn checked_source_committed_inner(
        &self,
        domain: SourceDomain,
        version: u64,
        request: Option<String>,
    ) {
        let Self { actor_ref, .. } = self;

        if actor_ref
            .cast(Message::CheckedSourceCommitted {
                domain,
                version,
                request,
            })
            .is_err()
        {
            tracing::warn!(
                ?domain,
                version,
                "checked source notice could not reach Core owner"
            );
        }
    }

    pub fn service_status(&self) -> ServiceHostStatus {
        let Self { service_status, .. } = self;

        service_status.borrow().clone()
    }

    pub async fn recover_startup(&self) -> anyhow::Result<()> {
        let Self {
            next_id, service, ..
        } = self;

        // Keep host-lock-before-mailbox ordering, matching every
        // Service transition and the initial StartupReconcile path.
        let transition = service.begin_transition().await?;
        let operation_id = next_id.fetch_add(1, Ordering::Relaxed);
        self.execute(Command::RecoverStartup {
            operation_id,
            transition,
        })
        .await
    }

    pub async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let Self { core, .. } = self;
        core.status().await
    }

    pub async fn submit_checked_with_authority(
        &self,
        domain: SourceDomain,
        version: u64,
        target: String,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
    ) -> anyhow::Result<()> {
        let actor_ref = &self.actor_ref;
        match actor_ref
            .call(
                |reply| Message::ApplyCheckedCommitted {
                    domain,
                    version,
                    target,
                    authority,
                    reply,
                },
                Some(CALL_WAIT),
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::Timeout => anyhow::bail!("checked runtime apply outcome is unknown"),
            CallResult::SenderError => {
                anyhow::bail!("checked runtime apply reply was dropped; outcome unknown")
            }
        }
    }

    pub fn subscribe_reconciler(&self) -> tokio::sync::watch::Receiver<ReconcilerStatus> {
        self.reconciler.clone()
    }

    pub async fn settled(&self, domain: SourceDomain, version: u64) -> ReconcilerStatus {
        super::status::settled(self.subscribe_reconciler(), domain, version).await
    }

    pub async fn apply_checked_committed(
        &self,
        domain: SourceDomain,
        version: u64,
        target: String,
    ) -> anyhow::Result<()> {
        let authority = self
            .source_authority
            .clone()
            .ok_or_else(|| anyhow::anyhow!("authoritative source verification is not connected"))?;
        self.submit_checked_with_authority(domain, version, target, authority)
            .await
    }

    /// Preserve the Service-generation fence before notifying durable source
    /// commits. Source publication does not prove an established Core owner.
    pub fn source_committed(&self, domain: SourceDomain, version: u64, reconcile_requested: bool) {
        let recover_startup = reconcile_requested && self.startup_recovery_needed();
        if recover_startup {
            self.request_startup_recovery();
        }
        if self
            .actor_ref
            .cast(Message::SourceCommitted {
                domain,
                version,
                reconcile_requested,
                startup_recovery_required: recover_startup,
            })
            .is_err()
        {
            tracing::warn!(
                ?domain,
                version,
                "failed to notify lifecycle of source commit"
            );
        }
    }

    pub fn request_runtime_rebuild(&self) {
        if self.startup_recovery_needed() {
            self.request_startup_recovery();
            return;
        }
        if self.actor_ref.cast(Message::RuntimeDirty).is_err() {
            tracing::warn!("failed to enqueue background runtime rebuild");
        }
    }

    pub fn request_startup_recovery(&self) {
        let client = self.clone();
        super::worker::schedule_startup_recovery(
            self.actor_ref.clone(),
            self.next_id.clone(),
            self.service.clone(),
            self.startup_recovery_pending.clone(),
            move || client.startup_recovery_needed(),
            |operation_id, transition| Command::RecoverStartup {
                operation_id,
                transition,
            },
            Message::Request,
            |succeeded| Message::StartupRecoveryFinished { succeeded },
        );
    }

    pub async fn request_service_endpoint_down_if(
        &self,
        still_current: impl Fn() -> bool + Send + 'static,
    ) {
        super::worker::request_service_endpoint_down_if(
            self.actor_ref.clone(),
            self.next_id.clone(),
            self.service.clone(),
            still_current,
            Command::ServiceEndpointDown,
            Message::Request,
        )
        .await;
    }

    /// The native ServiceActor is still the sole production status publisher.
    pub fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        if self.service.actor_status().is_none()
            && self
                .actor_ref
                .cast(Message::ObserveService { info })
                .is_err()
        {
            tracing::debug!("failed to publish legacy service status observation");
        }
    }

    pub fn observe_service_probe_failure(&self) {
        if self.service.actor_status().is_none()
            && self.actor_ref.cast(Message::ServiceProbeFailed).is_err()
        {
            tracing::debug!("failed to publish legacy service probe failure");
        }
    }

    pub async fn execute_service_command(
        &self,
        command: impl FnOnce(
            Box<dyn crate::client::core_lifecycle::ports::ServiceTransitionLease>,
        ) -> Command<P>,
    ) -> anyhow::Result<()> {
        let transition = self.service.begin_transition().await?;
        self.execute(command(transition)).await
    }

    pub async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<crate::client::core_lifecycle::command::ApplicationPatchResult> {
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

impl<W: Send + 'static, P: Send + 'static> Clone for ActorClient<W, P> {
    fn clone(&self) -> Self {
        Self {
            actor_ref: self.actor_ref.clone(),
            next_id: self.next_id.clone(),
            status: self.status.clone(),
            reconciler: self.reconciler.clone(),
            service_status: self.service_status.clone(),
            service: self.service.clone(),
            core: self.core.clone(),
            startup_owner_unproven: self.startup_owner_unproven.clone(),
            service_failed_generation: self.service_failed_generation.clone(),
            startup_recovery_pending: self.startup_recovery_pending.clone(),
            source_authority: self.source_authority.clone(),
        }
    }
}

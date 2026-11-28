//! The shared RuntimeReconciler actor. One mailbox, one workflow owner.
use super::{
    actor_state::{MailboxWorkflow, RuntimeReconcilerMailbox},
    client::Message,
    completion::{
        HandoffContinuation, RebuildAction, checked_clash_notice, rebuild_after_completion,
    },
    dispatch::{
        RequestAdmission, apply_pending_startup_retry_lease_failures,
        record_startup_retry_lease_failure,
    },
    mailbox::{CoreLifecycleStatus, DirtyTick, Mailbox, MailboxTimers},
    plan::{RunIntent, Trigger},
    startup_executor::StartupHost,
    status::ReconcilerStatus,
    worker::{WorkerCompletion, spawn_service_recovery_lease, spawn_startup_retry_lease},
    workflow::RuntimeReconcilerState,
};
use crate::{
    actor_v2::service_status::ServiceHostWatch,
    client::core_lifecycle::{RECOVERY_INTERVAL, command::Command, ports::ServiceLifecyclePort},
};
use ractor::{Actor, ActorProcessingErr, ActorRef};
use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

pub struct RuntimeReconcilerActor<W, P>(PhantomData<fn() -> (W, P)>);
impl<W, P> RuntimeReconcilerActor<W, P> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
impl<W, P> Default for RuntimeReconcilerActor<W, P> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RuntimeReconcilerArgs<W> {
    pub reconciler: tokio::sync::watch::Sender<ReconcilerStatus>,
    pub workflow: W,
    pub next_id: Arc<AtomicU64>,
    pub status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    pub service_status: ServiceHostWatch,
    pub service: Arc<dyn ServiceLifecyclePort>,
    pub source_authority:
        Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    pub startup_owner_unproven_shared: Arc<AtomicBool>,
    pub startup_recovery_pending: Arc<AtomicBool>,
}

// The Windows desktop regression needs to distinguish a slow Core operation
// from a synchronous Actor mailbox handler blocking a Checked ACK. This probe
// is compiled only into the isolated E2E executable; it never changes admission.
#[cfg(feature = "e2e")]
struct E2eCoreMailboxProbe {
    kind: &'static str,
    started: std::time::Instant,
}

#[cfg(feature = "e2e")]
impl E2eCoreMailboxProbe {
    fn begin(kind: &'static str) -> Self {
        if matches!(
            kind,
            "checked" | "completed" | "service_readiness" | "service_recovery_tick"
        ) {
            tracing::warn!(target: "chimera::e2e_core_mailbox", kind, stage = "entered", "CoreLifecycle mailbox stage");
        }
        Self {
            kind,
            started: std::time::Instant::now(),
        }
    }
}

#[cfg(feature = "e2e")]
impl Drop for E2eCoreMailboxProbe {
    fn drop(&mut self) {
        let elapsed_ms = self.started.elapsed().as_millis();
        if elapsed_ms >= 100 || matches!(self.kind, "checked" | "completed") {
            tracing::warn!(target: "chimera::e2e_core_mailbox", kind = self.kind, stage = "finished", elapsed_ms = elapsed_ms as u64, "CoreLifecycle mailbox stage");
        }
    }
}

impl<W: MailboxWorkflow<P>, P: Send + 'static> Actor for RuntimeReconcilerActor<W, P> {
    type Msg = Message<W, P>;
    type State = RuntimeReconcilerMailbox<W, P>;
    type Arguments = RuntimeReconcilerArgs<W>;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(RuntimeReconcilerMailbox {
            mailbox: Mailbox::new(
                args.next_id,
                args.status,
                args.startup_owner_unproven_shared,
            ),
            timers: MailboxTimers::new(&myself, RECOVERY_INTERVAL, || Message::ServiceRecoveryTick),
            reconciler: args.reconciler,
            workflow: Some(args.workflow),
            pending_mutation_connection: None,
            pending_startup_retry_lease_failures: VecDeque::new(),
            startup_retry_lease_failure_observer: None,
            service_status: args.service_status,
            service: args.service,
            runtime: RuntimeReconcilerState::default(),
            source_authority: args.source_authority,
            service_recovery_inflight: false,
            startup_recovery_pending: args.startup_recovery_pending,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        #[cfg(feature = "e2e")]
        let _mailbox_probe = E2eCoreMailboxProbe::begin(match &message {
            Message::Checked { .. } => "checked",
            Message::Completed { .. } => "completed",
            Message::ObserveService { .. } => "observe_service",
            Message::ServiceReadiness => "service_readiness",
            Message::ServiceProbeFailed => "service_probe_failed",
            Message::ServiceRecoveryTick => "service_recovery_tick",
            Message::Request(_) => "request",
            _ => "other",
        });
        match message {
            Message::AbortChecked { domain, request } => {
                let count = state
                    .runtime
                    .abort_checked_candidate(domain, request.as_deref());
                tracing::debug!(
                    ?domain,
                    ?request,
                    count,
                    "discarded uncommitted checked candidates"
                );
            }
            Message::InstallCandidateGate { gate, reply } => {
                let result = if state.mailbox.shutting_down {
                    Err(anyhow::anyhow!("core lifecycle is shutting down"))
                } else {
                    state
                        .workflow
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("core lifecycle workflow is busy"))
                        .and_then(|workflow| workflow.install_candidate_gate(gate))
                };
                let _ = reply.send(result);
            }
            Message::Checked { checked, reply } => {
                // Unlike a bare cast, this acknowledgement proves that the
                // owner actually retained the exact document and digest.
                // Do not let a closed or uncertain owner authorize a commit.
                let facts = state.source_dispatch_facts();
                let result = state.runtime.admit_checked_candidate(checked, facts);
                let _ = reply.send(result);
            }
            Message::Request(request) => {
                if matches!(&request.command, Command::ServiceEndpointDown(_)) {
                    state.service_recovery_inflight = true;
                }
                let service_connected = W::legacy_service_connected();
                match state.mailbox.receive_request(request, service_connected) {
                    RequestAdmission::Accepted => {}
                    RequestAdmission::Close => state.close(),
                    RequestAdmission::Reject(request, reason) => {
                        state.reject(request, anyhow::anyhow!(reason.message()));
                    }
                }
            }

            Message::Completed { completion } => {
                let WorkerCompletion {
                    id,
                    workflow,
                    result,
                    workflow_panicked,
                    lower_outcome_uncertain,
                    startup_owner_unproven,
                    service_restart_policy,
                    service_probe,
                    retry_reconcile_on_failure,
                    recover,
                    service_endpoint_recovery,
                    shutdown,
                    request_runtime_rebuild,
                    deferred_retry,
                    reconciled,
                } = completion;
                if state
                    .mailbox
                    .active
                    .as_ref()
                    .is_some_and(|active| active.response.id == id)
                {
                    let lifecycle = workflow.lifecycle();
                    let super::completion::CompletedRound {
                        active,
                        result,
                        source_advanced,
                        round,
                        runtime_binding,
                    } = super::completion::observe_completed(
                        &mut state.mailbox,
                        &state.runtime,
                        lifecycle,
                        id,
                        result,
                        workflow_panicked,
                        lower_outcome_uncertain,
                        reconciled,
                        service_probe.is_some(),
                        shutdown,
                        || workflow.take_prepared_outcome(),
                        || workflow.take_checked_connection_effect(),
                        || workflow.effect_port_binding(),
                    )
                    .await
                    .expect("matched active lifecycle operation");
                    let verified_running = round.verified_running;
                    state.runtime.observe_round_connection(&round);
                    // Legacy SetConfig UI messages describe the outcome of
                    // Core refresh, not source persistence. Checked Clash
                    // follows the same semantics once the Actor settles it.
                    if let Some(notice) =
                        checked_clash_notice(active.checked_clash, &result, verified_running)
                    {
                        W::checked_clash_notice(notice);
                    }
                    // This belongs to the existing LocalRuntimeHost. The
                    // status projection only records outstanding publication
                    // debt and cannot mutate or duplicate the runtime product.
                    let pending_product =
                        workflow.lifecycle().core().pending_runtime_product_target();
                    state.reconciler.send_modify(|status| {
                        status.observe_product_publication(pending_product.as_deref());
                    });
                    state.publish_maintenance();
                    state.workflow = Some(workflow);
                    if service_endpoint_recovery {
                        state.service_recovery_inflight = false;
                    }
                    if state.runtime.observe_host_completion(
                        active.trigger,
                        service_endpoint_recovery,
                        &result,
                    ) {
                        state.publish_maintenance();
                    }
                    state.mailbox.set_owner_unproven(startup_owner_unproven);
                    W::restore_pending_connection(state);
                    if let Some(ports) = runtime_binding
                        && let Some(application_workflow) = state.workflow.as_ref()
                    {
                        application_workflow.runtime_bound(ports);
                    }
                    apply_pending_startup_retry_lease_failures(
                        state.workflow.as_mut().map(W::progress_mut),
                        &mut state.pending_startup_retry_lease_failures,
                    );
                    if let Some((identity, next_attempt)) = state
                        .workflow
                        .as_ref()
                        .and_then(|workflow| workflow.progress().startup_retry_schedule())
                    {
                        state.schedule_startup_retry(&myself, identity, next_attempt);
                    }
                    state.mailbox.uncertain |= workflow_panicked || lower_outcome_uncertain;

                    match service_probe {
                        Some(Ok(info)) => {
                            state.publish_service_status_with_policy(info, service_restart_policy)
                        }
                        Some(Err(error)) => {
                            tracing::debug!(%error, "failed to refresh service status after mutation");
                            state.publish_service_probe_failure();
                        }
                        None => {}
                    }

                    let (failed, retry) = super::completion::settle_completed(
                        &mut state.mailbox,
                        &mut state.runtime,
                        state
                            .workflow
                            .as_ref()
                            .expect("completed workflow was restored")
                            .lifecycle()
                            .progress(),
                        &state.reconciler,
                        round,
                        active.response,
                        active.trigger,
                        &active.observed_sources,
                        reconciled,
                        shutdown,
                        result,
                    );
                    if let Some((digest, next_attempt)) = super::completion::settle_deferred_retry(
                        state.workflow.as_mut().map(W::progress_mut),
                        deferred_retry,
                        reconciled,
                        failed,
                        state.mailbox.uncertain,
                        retry,
                    )
                    .await
                    {
                        state.schedule_deferred_retry(&myself, digest, next_attempt);
                    }

                    #[cfg(feature = "e2e")]
                    tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_retry_settled", "CoreLifecycle completion stage");
                    if state.runtime.retry_completed_source(
                        retry_reconcile_on_failure,
                        failed,
                        source_advanced,
                        state.mailbox.uncertain,
                        state.mailbox.shutting_down,
                    ) {
                        state.mark_runtime_dirty(&myself);
                    }
                    match rebuild_after_completion(
                        request_runtime_rebuild,
                        state.mailbox.shutting_down,
                        state.mailbox.startup_owner_unproven
                            || state
                                .mailbox
                                .startup_owner_unproven_shared
                                .load(Ordering::Acquire),
                        state.mailbox.uncertain,
                    ) {
                        RebuildAction::None => {}
                        RebuildAction::RecoverStartup => state.request_startup_recovery(&myself),
                        RebuildAction::ScheduleRuntime => {
                            let deferred = state
                                .workflow
                                .as_ref()
                                .and_then(|workflow| workflow.progress().deferred_retry());
                            if let Some((digest, next_attempt)) = deferred {
                                state.schedule_deferred_retry(&myself, digest, next_attempt);
                            } else {
                                state.mark_runtime_dirty(&myself);
                            }
                        }
                    }
                    if recover && failed && !state.mailbox.uncertain && !state.mailbox.shutting_down
                    {
                        tracing::error!("failed to recover core; scheduling retry");
                        state
                            .timers
                            .after(&myself, Duration::from_secs(5), || Message::RecoverCore);
                    }
                    state.follow_service().await;
                    state.rearm_deferred_dirty_after_owner(&myself);
                    match state.runtime.complete_handoff(
                        active.trigger,
                        reconciled,
                        failed,
                        verified_running,
                        state.mailbox.uncertain,
                        state.mailbox.startup_owner_unproven
                            || state
                                .mailbox
                                .startup_owner_unproven_shared
                                .load(Ordering::Acquire),
                        state.mailbox.shutting_down,
                    ) {
                        HandoffContinuation::None => {}
                        HandoffContinuation::ClearMaintenance => state.publish_maintenance(),
                        HandoffContinuation::MarkDirty => state.mark_runtime_dirty(&myself),
                    }
                    state
                        .schedule_service_release(active.trigger == Trigger::Retry)
                        .await;
                } else {
                    tracing::warn!(id, "ignoring stale core lifecycle completion");
                }
            }
            Message::ReadStartupReport { reply } => {
                let report = state
                    .workflow
                    .as_ref()
                    .and_then(|workflow| workflow.progress().startup_report());
                let _ = reply.send(report);
            }
            Message::ObserveService { info } => {
                state.publish_service_status(info);
                state.follow_service().await;
                state.replan_retained_source_on_service_change(&myself);
                state.rearm_deferred_dirty_after_owner(&myself);
                state.schedule_service_release(false).await;
            }
            Message::ServiceReadiness => {
                state.follow_service().await;
                state.replan_retained_source_on_service_change(&myself);
                state.rearm_deferred_dirty_after_owner(&myself);
                state.schedule_service_release(false).await;
            }
            Message::ServiceProbeFailed => {
                state.publish_service_probe_failure();
                state.follow_service().await;
                state.schedule_service_release(false).await;
            }
            Message::ServiceRecoveryTick => {
                let can_recover = !state.mailbox.uncertain
                    && !state.mailbox.shutting_down
                    && state.mailbox.idle()
                    && !state.service_recovery_inflight;
                let recovery_due = if can_recover {
                    match state.workflow.as_ref() {
                        Some(workflow) => workflow.service_endpoint_recovery_due().await,
                        None => false,
                    }
                } else {
                    false
                };
                if recovery_due {
                    state.service_recovery_inflight = true;
                    state.runtime.service_recovery_maintenance =
                        Some("Service endpoint recovery is in progress".into());
                    state.publish_maintenance();
                    let service = state.service.clone();
                    let actor = myself.clone();
                    let operation_id = state.mailbox.allocate_operation_id();
                    spawn_service_recovery_lease(
                        actor,
                        service,
                        operation_id,
                        Message::Request,
                        || Message::ServiceRecoveryLeaseFailed,
                    );
                }
            }
            Message::ServiceRecoveryLeaseFailed => {
                state.service_recovery_inflight = false;
                state.runtime.service_recovery_maintenance =
                    Some("Service endpoint recovery is waiting for a transition lease".into());
                state.publish_maintenance();
            }
            Message::RecoverCore => {
                if state.mailbox.uncertain || state.mailbox.shutting_down {
                    tracing::warn!(
                        uncertain = state.mailbox.uncertain,
                        shutting_down = state.mailbox.shutting_down,
                        "ignoring crash recovery because lifecycle is unavailable"
                    );
                } else {
                    tracing::info!("queueing core recovery through lifecycle actor");
                    state.push_background(Command::RecoverCore);
                }
            }
            Message::SourceCommitted {
                domain,
                version,
                reconcile_requested,
                startup_recovery_required,
            } => {
                let facts = state.source_dispatch_facts();
                let dispatch = state.runtime.dispatch_source_committed(
                    domain,
                    version,
                    reconcile_requested,
                    startup_recovery_required,
                    facts,
                );
                state.apply_source_dispatch(&myself, domain, version, dispatch);
            }
            Message::CheckedSourceCommitted {
                domain,
                version,
                request,
            } => {
                let facts = state.source_dispatch_facts();
                let dispatch = state.runtime.dispatch_checked_source(
                    domain,
                    version,
                    request.as_deref(),
                    facts,
                );
                state.apply_source_dispatch(&myself, domain, version, dispatch);
            }
            Message::ApplyCheckedCommitted {
                domain,
                version,
                target,
                authority,
                reply,
            } => {
                let facts = state.source_dispatch_facts();
                if let Some(checked) = state
                    .runtime
                    .take_explicit_checked(domain, version, &target, facts)
                {
                    state.mailbox.enqueue_checked_source(
                        checked,
                        domain,
                        version,
                        authority,
                        Some(reply),
                    );
                } else {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "checked candidate has no matching committed source or Core owner"
                    )));
                }
            }
            Message::RuntimeDirty => {
                if state.runtime.run_intent == RunIntent::Stopped
                    || state.mailbox.uncertain
                    || state.mailbox.shutting_down
                {
                    tracing::debug!(
                        ?state.runtime.run_intent,
                        uncertain = state.mailbox.uncertain,
                        shutting_down = state.mailbox.shutting_down,
                        "ignoring runtime dirty signal while stopped or unavailable"
                    );
                } else {
                    state.mark_runtime_dirty(&myself);
                }
            }
            Message::DirtyTick => {
                let owner_transition = state.owner_transition_pending();
                match state.runtime.dirty_tick(
                    state.mailbox.uncertain,
                    state.mailbox.shutting_down,
                    owner_transition,
                    state.mailbox.queue_full(),
                ) {
                    DirtyTick::None => {}
                    DirtyTick::Reconcile => state.push_background(Command::Reconcile),
                    DirtyTick::SourceCommitted => state.push_background(Command::ReconcileSource),
                }
            }
            Message::StartupRecoveryFinished { succeeded } => {
                if succeeded {
                    state.replan_retained_source_on_service_change(&myself);
                    state.rearm_deferred_dirty_after_owner(&myself);
                }
            }
            Message::StartupRetryDue { identity } => {
                use super::dispatch::{StartupRetryAdmission, admit_startup_retry};
                match admit_startup_retry(
                    &state.mailbox,
                    &state.runtime,
                    state.workflow.as_mut().map(W::progress_mut),
                    &identity,
                ) {
                    StartupRetryAdmission::Ignore => {}
                    StartupRetryAdmission::Reschedule(deadline) => {
                        state.schedule_startup_retry(&myself, identity, deadline)
                    }
                    StartupRetryAdmission::AcquireLease => {
                        let service = state.service.clone();
                        let actor = myself.clone();
                        let operation_id = state.mailbox.allocate_operation_id();
                        let observer = state.startup_retry_lease_failure_observer.take();
                        spawn_startup_retry_lease(
                            actor,
                            service,
                            operation_id,
                            identity,
                            Message::Request,
                            move |identity, reason| Message::StartupRetryLeaseFailed {
                                identity,
                                reason,
                                observer,
                            },
                        );
                    }
                }
            }
            Message::StartupRetryLeaseFailed {
                identity,
                reason,
                observer,
            } => {
                record_startup_retry_lease_failure(
                    state.workflow.as_mut().map(W::progress_mut),
                    &mut state.pending_startup_retry_lease_failures,
                    identity,
                    reason,
                );
                if let Some((identity, next_attempt)) = state
                    .workflow
                    .as_ref()
                    .and_then(|workflow| workflow.progress().startup_retry_schedule())
                {
                    state.schedule_startup_retry(&myself, identity, next_attempt);
                }
                if let Some(observer) = observer {
                    let _ = observer.send(());
                }
            }
            Message::DeferredRetry { digest } => {
                // Take the same lifecycle Arc before borrowing its progress
                // mutably. The worker owns it whenever workflow is absent.
                let lifecycle = state.workflow.as_ref().map(W::lifecycle_handle);
                let workflow = state
                    .workflow
                    .as_mut()
                    .zip(lifecycle.as_ref())
                    .map(|(workflow, lifecycle)| (workflow.progress_mut(), lifecycle.core()));
                if let Some(next_attempt) =
                    super::dispatch::dispatch_deferred_retry(&mut state.mailbox, workflow, &digest)
                        .await
                {
                    state.schedule_deferred_retry(&myself, digest, next_attempt);
                }
            }
            #[allow(unreachable_patterns)]
            other => {
                if !W::handle_test_message(other, state).await? {
                    return Ok(());
                }
            }
        }
        state.drive(&myself);
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.timers.cancel();
        // The workflow (and its transaction/Service lease) is moved into the
        // active task. An unexpected Actor stop cannot drop that owner while
        // it might still be mutating Core or committing a profile decision.
        // The outcome is unproven because its Completed message was not
        // handled; do not publish a successful runtime conclusion.
        state.mailbox.join_lost_worker().await;
        state.runtime.checked.clear();
        state.reconciler.send_modify(ReconcilerStatus::close);
        Ok(())
    }
}

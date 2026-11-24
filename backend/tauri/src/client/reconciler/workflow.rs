//! Serialized committed convergence dispatch over the sole lifecycle owner.
use chimera_contracts::execution_host::ExecutionHost;
use chimera_core::client::reconciler::{
    plan::{RunIntent, Trigger},
    workflow::effective_host,
};

use super::{
    apply::finish_connection_effect,
    attempt::OwnershipTransitionPending,
    checked::CheckedRuntimeInbox,
    committed::{SourceDomain, SourceVersions},
    proof::host_for,
    release::ServiceReleaseDebt,
    startup::{CommittedRound, uses_committed_convergence},
};
use crate::client::{
    application_workflow::startup::ServiceEvidence,
    core_lifecycle::{Command, CoreLifecycleWorkflow},
};

/// Runtime-owned fields of the single production mailbox. Lower-host and
/// source actors own neither its desired run intent nor its checked cache.
pub(in crate::client) struct RuntimeReconcilerState {
    pub(in crate::client) dirty: bool,
    pub(in crate::client) dirty_tick_scheduled: bool,
    pub(in crate::client) source_dirty: bool,
    pub(in crate::client) source_after_handoff: bool,
    pub(in crate::client) run_intent: RunIntent,
    pub(in crate::client) checked: CheckedRuntimeInbox,
    pub(in crate::client) seen_sources: SourceVersions,
    pub(in crate::client) service_handoff_maintenance: Option<String>,
    pub(in crate::client) service_release_debt: ServiceReleaseDebt,
    pub(in crate::client) service_recovery_maintenance: Option<String>,
    pub(in crate::client) connection_interruption_maintenance: Option<String>,
}

impl Default for RuntimeReconcilerState {
    fn default() -> Self {
        Self {
            dirty: false,
            dirty_tick_scheduled: false,
            source_dirty: false,
            source_after_handoff: false,
            run_intent: RunIntent::Running,
            checked: Default::default(),
            seen_sources: Default::default(),
            service_handoff_maintenance: None,
            service_release_debt: Default::default(),
            service_recovery_maintenance: None,
            connection_interruption_maintenance: None,
        }
    }
}

pub(in crate::client) enum RuntimeAdmission {
    Run,
    RejectStoppedChecked,
    RejectStaleChecked,
    SkipInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum CheckedSourceNoticeKind {
    Current,
    Duplicate,
    Stale,
    Superseded,
}

pub(in crate::client) struct CheckedSourceNotice {
    pub previous: Option<u64>,
    pub mode_requested: bool,
    pub kind: CheckedSourceNoticeKind,
}

impl RuntimeReconcilerState {
    pub(in crate::client) fn admit(&mut self, command: &Command) -> RuntimeAdmission {
        match command {
            Command::StopCore => {
                self.run_intent = RunIntent::Stopped;
                self.clear_dirty();
            }
            Command::StartCore => self.run_intent = RunIntent::Running,
            _ => {}
        }
        if self.run_intent == RunIntent::Stopped {
            match command {
                Command::ApplyChecked { .. } => return RuntimeAdmission::RejectStoppedChecked,
                Command::Reconcile
                | Command::ReconcileSource
                | Command::ReconcileProfiles { .. }
                | Command::RetryDeferredProfiles { .. }
                | Command::RecoverCore
                | Command::RecoverStartup { .. }
                | Command::RetryStartup { .. }
                | Command::FollowService { .. } => return RuntimeAdmission::SkipInactive,
                _ => {}
            }
        }
        // Recheck at execution, not only queue admission. A source notice
        // may advance while another command owns the worker.
        if let Command::ApplyChecked {
            domain,
            version,
            prepared,
            ..
        } = command
            && (self.seen_sources.version(*domain) != Some(*version) || prepared.target.is_none())
        {
            return RuntimeAdmission::RejectStaleChecked;
        }
        RuntimeAdmission::Run
    }

    /// Observe the monotonic source publication and revoke any receipt that
    /// can no longer authorize an apply. Scheduler decisions stay with the
    /// same mailbox: stale runtime intent can request latest-source recovery.
    pub(in crate::client) fn observe_checked_source(
        &mut self,
        domain: SourceDomain,
        version: u64,
        request: Option<&str>,
    ) -> CheckedSourceNotice {
        let previous = self.seen_sources.version(domain);
        let mode_requested =
            request.is_some_and(|receipt| self.checked.requests_mode_interruption(domain, receipt));
        self.seen_sources.observe(domain, version);
        let kind = if previous == Some(version) {
            if let Some(request) = request {
                self.checked.discard_request(domain, request);
            }
            CheckedSourceNoticeKind::Duplicate
        } else if previous.is_some_and(|seen| seen > version) {
            if let Some(request) = request {
                self.checked.discard_request(domain, request);
            } else {
                self.checked.discard_domain(domain);
            }
            CheckedSourceNoticeKind::Stale
        } else if self.seen_sources.version(domain) != Some(version) {
            CheckedSourceNoticeKind::Superseded
        } else {
            CheckedSourceNoticeKind::Current
        };
        CheckedSourceNotice {
            previous,
            mode_requested,
            kind,
        }
    }

    pub(in crate::client) fn clear_dirty(&mut self) {
        self.dirty = false;
        self.source_dirty = false;
        self.source_after_handoff = false;
        self.connection_interruption_maintenance = None;
    }

    pub(in crate::client) fn maintenance(&self, product: Option<&str>) -> Option<String> {
        let service_release = self.service_release_debt.maintenance();
        let details: Vec<_> = [
            self.service_handoff_maintenance.as_deref(),
            service_release.as_deref(),
            self.service_recovery_maintenance.as_deref(),
            product,
            self.connection_interruption_maintenance.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!details.is_empty()).then(|| details.join("; "))
    }
}

/// A late authoritative source change is a replan request, not evidence
/// that the preceding Core operation converged the latest durable target.
/// Source notices can arrive in the CoreLifecycle mailbox while a source
/// round owns the single Core executor. The worker's own post-Core source
/// read is not sufficient if a new *message* is received immediately after
/// that read, but before Completed is handled. A verified old Core receipt
/// cannot settle the newer version or release an S2 continuation.
pub(in crate::client) fn source_committed_during_round(
    trigger: &str,
    started: &SourceVersions,
    latest: &SourceVersions,
) -> bool {
    trigger == "SourceCommitted" && started != latest
}

pub(in crate::client) fn committed_target_advanced(result: &anyhow::Result<()>) -> bool {
    result.as_ref().err().is_some_and(|error| {
        error.downcast_ref::<ProfilesSourceAdvanced>().is_some()
            || error
                .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
                .is_some()
    })
}

/// On a source fallback, never discard an acknowledged Mode effect silently:
/// only a *fresh* committed receipt with a mode intent and no matching Core
/// dispatch can create outstanding effect debt. Duplicate/out-of-order notices
/// do not create a second effect or overwrite the first operation's status.
pub(in crate::client) fn checked_mode_effect_deferred(
    previous: Option<u64>,
    version: u64,
    mode_requested: bool,
    checked_dispatched: bool,
    running: bool,
) -> bool {
    mode_requested && !checked_dispatched && running && previous.is_none_or(|seen| version > seen)
}

pub(in crate::client) fn deferred_mode_effect_degradation(
    version: u64,
) -> chimera_contracts::runtime::Degradation {
    use chimera_contracts::runtime::{
        Degradation, DegradationPhase, DegradationReason, InterruptFailure,
    };
    Degradation {
        phase: DegradationPhase::SystemEffect,
        reason: DegradationReason::ModeInterruptionFailed {
            cause: InterruptFailure::Unavailable,
        },
        message: format!(
            "Checked Clash source version {version} was deferred to latest-source recovery; its Mode interruption is not confirmed"
        ),
        retryable: false,
    }
}

pub(in crate::client) fn runtime_trigger(command: &Command) -> &'static str {
    match command {
        Command::StartCore => "Start",
        Command::StopCore => "Stop",
        Command::Shutdown => "Shutdown",
        Command::Reconcile => "Retry",
        Command::ReconcileSource | Command::ApplyChecked { .. } => "SourceCommitted",
        Command::MaintainServiceRelease => "ServiceRelease",
        Command::StartupReconcile { .. } => "Startup",
        Command::RetryStartup { .. } => "Retry",
        Command::FollowService { .. } => "ServiceReadiness",
        Command::ReplaceCoreBinary(_) => "BinaryReplaced",
        Command::ProfileMutation(_) => "Mutation",
        Command::RecoverCore => "CoreChanged",
        Command::RetryDeferredProfiles { .. } => "Tick",
        _ => "ServiceOperation",
    }
}

/// An older source generation became stale while the single runtime actor
/// was building or applying it. The actor must schedule another latest-source
/// round, but must not blindly retry arbitrary Core apply failures.
#[derive(Debug, thiserror::Error)]
#[error("Profiles source advanced while reconciling runtime: {before} -> {after}")]
pub(crate) struct ProfilesSourceAdvanced {
    before: u64,
    after: u64,
}

/// Apply ref's effective host preference to the same typed Service evidence
/// used by the owner planner. Chimera's foreign-owner and generation fence
/// must remain outside the generic upstream policy.
pub(crate) fn desired_host(
    enabled: bool,
    service: &ServiceEvidence,
    generation: u64,
    failed_generation: u64,
    owner: ExecutionHost,
) -> ExecutionHost {
    if matches!(service, ServiceEvidence::ForeignCore { .. }) {
        return ExecutionHost::Local;
    }
    let readiness = chimera_core::client::core_lifecycle::workflow::service_usable(
        service.phase(),
        generation,
        (failed_generation != 0).then_some(failed_generation),
    );
    effective_host(enabled, readiness, owner)
}

impl CoreLifecycleWorkflow {
    /// Treat Service failure for one readiness generation as a Local
    /// preference only after the previous Service Core was safely released.
    async fn committed_run_type(
        &self,
        enable_service_mode: bool,
        trigger: Trigger,
    ) -> anyhow::Result<crate::core::RunType> {
        use crate::core::RunType;
        if self.canonical_convergence_session() && uses_committed_convergence(trigger, true) {
            let observed = self.convergence_core().status().await.map_err(|error| {
                OwnershipTransitionPending::waiting(format!(
                    "cannot observe selected owner: {error:#}"
                ))
            })?;
            let (service, generation) = self.service_evidence().await;
            let failed = self.startup_failed_generation();
            let host = desired_host(
                enable_service_mode,
                &service,
                generation,
                failed,
                host_for(observed.run_type),
            );
            return Ok(match host {
                ExecutionHost::Local => RunType::Normal,
                ExecutionHost::Service => RunType::Service,
            });
        }

        let preferred = RunType::classify(
            enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        if preferred != RunType::Service {
            return Ok(preferred);
        }
        let failed_generation = self.startup_failed_generation();
        if failed_generation == 0 {
            return Ok(preferred);
        }
        let (_, current_generation) = self.service_evidence().await;
        Ok(if failed_generation == current_generation {
            RunType::Normal
        } else {
            preferred
        })
    }

    pub(in crate::client) async fn reconcile_with_trigger(
        &self,
        trigger: Trigger,
    ) -> anyhow::Result<()> {
        // Ref maintain(Retry) lives with S4 apply outcome maintenance.
        // Only repair the accepted receipt's derived file, and never replay
        // old Core bytes or publish a product after its source has advanced.
        if crate::client::reconciler::apply::retry_pending_product(
            self.convergence_core(),
            trigger,
            || self.startup_target_identity(),
        )
        .await?
        {
            return Ok(());
        }
        let clash = self.convergence_clash()?;
        let app = self.startup_application_config();
        let target_core = app.core;
        if let Some(before) = self.convergence_profiles_version() {
            // Capture both the authoritative committed document and its managed
            // file bytes under the same publication fence, as in ref.
            let (target, app, clash, current, content) =
                self.capture_reconcile_committed_target(app, clash).await?;
            let captured = self
                .convergence_profiles_version()
                .ok_or_else(|| anyhow::anyhow!("Profiles owner disappeared during convergence"))?;
            if captured != before {
                return Err(ProfilesSourceAdvanced {
                    before,
                    after: captured,
                }
                .into());
            }
            let run_type = self
                .committed_run_type(app.enable_service_mode, trigger)
                .await?;
            let host = match run_type {
                crate::core::RunType::Service => ExecutionHost::Service,
                crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                    ExecutionHost::Local
                }
            };
            let planned = self.planned_committed_round(&target, host, trigger).await?;
            let ref_apply = matches!(&planned, CommittedRound::Apply);
            let handled_by_plan = matches!(&planned, CommittedRound::Settled(_));
            let applied = match planned {
                CommittedRound::Settled(result) => {
                    if result.is_err() {
                        self.convergence_progress().lock().invalidate_selection();
                    }
                    result
                }
                CommittedRound::Apply | CommittedRound::LegacyRecovery => {
                    let admitted = self
                        .convergence_progress()
                        .lock()
                        .allow(&target, host, trigger);
                    match admitted {
                        Ok(()) => {
                            // The actual Core attempt is staged, never recorded
                            // as Applied/Refused before Completed's final fence.
                            self.convergence_progress()
                                .lock()
                                .stage(target.clone(), host);
                            let prepared = match run_type {
                                crate::core::RunType::Normal => {
                                    self.try_reconcile_prepared_local(
                                        &target, &app, &clash, run_type,
                                    )
                                    .await
                                }
                                crate::core::RunType::Service => {
                                    self.try_reconcile_prepared_service(&target, &app, &clash)
                                        .await
                                }
                                crate::core::RunType::Elevated => None,
                            };
                            use crate::client::reconciler::apply::{
                                CommittedPreparedDispatch, route_committed_prepared,
                            };
                            match route_committed_prepared(ref_apply, prepared) {
                                CommittedPreparedDispatch::Observed(result) => result,
                                CommittedPreparedDispatch::LegacyRecovery => {
                                    // Only a missing/unproven legacy S3/S4
                                    // plan may use inline Core reconciliation.
                                    self.convergence_progress().lock().invalidate_selection();
                                    self.convergence_core()
                                        .reconcile_profiles(
                                            clash,
                                            app.core,
                                            run_type,
                                            current,
                                            app,
                                            content,
                                            Some(target.clone()),
                                        )
                                        .await
                                }
                            }
                        }
                        Err(previous_refusal) => Err(previous_refusal.into()),
                    }
                }
            };
            let applied = if host == ExecutionHost::Service && !handled_by_plan {
                match applied {
                    Ok(()) => Ok(()),
                    Err(refusal) => self.fallback_after_service_refusal(&target, refusal).await,
                }
            } else {
                applied
            };
            let after = self
                .convergence_profiles_version()
                .ok_or_else(|| anyhow::anyhow!("Profiles owner disappeared during convergence"))?;
            if after != captured {
                return Err(ProfilesSourceAdvanced {
                    before: captured,
                    after,
                }
                .into());
            }
            applied
        } else {
            let run_type = self
                .committed_run_type(app.enable_service_mode, trigger)
                .await?;
            let target = self.startup_target_identity().await?;
            let host = match run_type {
                crate::core::RunType::Service => ExecutionHost::Service,
                crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                    ExecutionHost::Local
                }
            };
            let planned = self.planned_committed_round(&target, host, trigger).await?;
            let handled_by_plan = matches!(&planned, CommittedRound::Settled(_));
            let result = match planned {
                CommittedRound::Settled(result) => {
                    self.convergence_progress().lock().invalidate_selection();
                    result
                }
                CommittedRound::Apply | CommittedRound::LegacyRecovery => {
                    let admitted = self
                        .convergence_progress()
                        .lock()
                        .allow(&target, host, trigger);
                    match admitted {
                        Ok(()) => {
                            self.convergence_progress().lock().invalidate_selection();
                            self.convergence_progress()
                                .lock()
                                .stage(target.clone(), host);
                            self.convergence_core()
                                .reconcile(clash, target_core, run_type)
                                .await
                        }
                        Err(previous_refusal) => Err(previous_refusal.into()),
                    }
                }
            };
            if host == ExecutionHost::Service && !handled_by_plan {
                match result {
                    Ok(()) => Ok(()),
                    Err(refusal) => self.fallback_after_service_refusal(&target, refusal).await,
                }
            } else {
                result
            }
        }
    }
}

/// One completed runtime round, observed only by the existing mailbox after
/// the worker has joined. This object does not own or mutate a Core session.
pub(in crate::client) struct Round {
    pub verified_running: bool,
    verified_stopped: bool,
    pub connection_effect_observed: bool,
    pub connection_degradation: Option<chimera_contracts::runtime::Degradation>,
    verdict: super::attempt::LegacyApplyVerdict,
}

impl Round {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::client) async fn observe(
        lifecycle: &CoreLifecycleWorkflow,
        result: &anyhow::Result<()>,
        trigger: &str,
        reconciled: bool,
        checked_apply: bool,
        source_advanced: bool,
        final_uncertain: bool,
        prepared_outcome: Option<&super::mutation::ApplyOutcome>,
    ) -> Self {
        // A successful command alone is not proof that the Core
        // reached Running or Stopped. Publish Converged/Inactive
        // only after sampling the runtime owner.
        let observed_core = if result.is_ok() && (reconciled || trigger == "Stop") {
            lifecycle.startup_core_status().await.ok()
        } else {
            None
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_core_status_observed", "CoreLifecycle completion stage");
        let verified_running = if reconciled && result.is_ok() {
            if let Some(observed) = observed_core.as_ref() {
                lifecycle.verified_applied_runtime(observed).await
            } else {
                false
            }
        } else {
            false
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_running_verified", "CoreLifecycle completion stage");
        let verified_stopped = trigger == "Stop"
            && observed_core.as_ref().is_some_and(|snapshot| {
                matches!(
                    snapshot.state,
                    chimera_ipc::api::status::CoreState::Stopped(_)
                )
            });
        // The upstream reconciler records selection after a
        // confirmed runtime apply, not when a source saves.
        // Legacy startup, readiness and Retry use a Result<()>
        // Core port; recover only if both Core and source target
        // still match. Checked Apply has already done its own
        // exact-version selection check and must not be reset.
        if reconciled && !checked_apply {
            if result.is_ok() && verified_running && !source_advanced && !final_uncertain {
                lifecycle.recover_applied_selection_from_receipt().await;
            } else {
                lifecycle.invalidate_applied_selection();
            }
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_selection_recovery_finished", "CoreLifecycle completion stage");
        // Unlike the worker, Completed owns the final observed
        // source-version fence and Running evidence. The previous
        // Core's connections may only be interrupted here; a
        // superseded checked product is never an effect success.
        let mut round_connection_degradation = None;
        let mut connection_effect_observed = false;
        if let Some(effect) = lifecycle.take_checked_connection_effect() {
            let issue = finish_connection_effect(
                effect,
                result,
                verified_running && !final_uncertain,
                source_advanced,
            )
            .await;
            if let Some(ref degradation) = issue {
                tracing::warn!(reason = ?degradation.reason, message = %degradation.message, "checked connection effect degraded");
            }
            connection_effect_observed = true;
            round_connection_degradation = issue;
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_connection_effect_finished", "CoreLifecycle completion stage");

        let verdict = super::attempt::observed_apply_verdict(
            result,
            final_uncertain,
            source_advanced,
            prepared_outcome,
        );
        Self {
            verified_running,
            verified_stopped,
            connection_effect_observed,
            connection_degradation: round_connection_degradation,
            verdict,
        }
    }

    /// One atomic status publication and one attempt settlement, from the
    /// exact same terminal. Never derive Applied from a successful reply.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::client) fn settle(
        self,
        lifecycle: &CoreLifecycleWorkflow,
        status: &tokio::sync::watch::Sender<super::status::ReconcilerStatus>,
        operation_id: u64,
        trigger: &str,
        observed_sources: &super::committed::SourceVersions,
        intent: chimera_core::client::reconciler::plan::RunIntent,
        reconciled: bool,
    ) {
        lifecycle.settle_observed_attempt(&self.verdict, self.verified_running);
        status.send_modify(|status| {
            status.finish_with_verdict(
                operation_id,
                trigger,
                observed_sources,
                intent,
                reconciled,
                self.verified_running,
                self.verified_stopped,
                &self.verdict,
            );
            if let Some(degradation) = self.connection_degradation {
                status.record_system_degradation(degradation);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::actor_v2::service_policy::ServicePhase;

    #[test]
    fn stop_retains_source_versions_and_only_explicit_start_resumes_runtime() {
        let mut state = RuntimeReconcilerState::default();
        state.seen_sources.observe(SourceDomain::Profiles, 4);
        state.dirty = true;
        state.source_dirty = true;
        state.source_after_handoff = true;
        assert!(matches!(
            state.admit(&Command::StopCore),
            RuntimeAdmission::Run
        ));
        assert_eq!(state.run_intent, RunIntent::Stopped);
        assert!(!state.dirty && !state.source_dirty && !state.source_after_handoff);
        assert!(matches!(
            state.admit(&Command::ReconcileSource),
            RuntimeAdmission::SkipInactive
        ));
        assert!(matches!(
            state.admit(&Command::Reconcile),
            RuntimeAdmission::SkipInactive
        ));
        assert!(matches!(
            state.admit(&Command::RecoverCore),
            RuntimeAdmission::SkipInactive
        ));
        assert_eq!(state.seen_sources.version(SourceDomain::Profiles), Some(4));
        assert!(matches!(
            state.admit(&Command::StartCore),
            RuntimeAdmission::Run
        ));
        assert_eq!(state.run_intent, RunIntent::Running);
    }

    #[test]
    fn checked_notices_never_move_a_source_version_backward() {
        let mut state = RuntimeReconcilerState::default();
        assert_eq!(
            state
                .observe_checked_source(SourceDomain::Clash, 5, None)
                .kind,
            CheckedSourceNoticeKind::Current
        );
        assert_eq!(
            state
                .observe_checked_source(SourceDomain::Clash, 5, None)
                .kind,
            CheckedSourceNoticeKind::Duplicate
        );
        assert_eq!(
            state
                .observe_checked_source(SourceDomain::Clash, 4, None)
                .kind,
            CheckedSourceNoticeKind::Stale
        );
        assert_eq!(state.seen_sources.version(SourceDomain::Clash), Some(5));
        assert_eq!(state.seen_sources.version(SourceDomain::Profiles), None);
    }

    #[test]
    fn ref_effective_host_keeps_unknown_owner_and_respects_generation_fence() {
        let unknown = ServiceEvidence::Unreadable {
            phase: ServicePhase::Unknown,
        };
        let ready = ServiceEvidence::CoreStopped {
            ready: true,
            phase: ServicePhase::Ready,
            runtime_owned: true,
        };
        let foreign = ServiceEvidence::ForeignCore {
            ready: true,
            phase: ServicePhase::Ready,
        };
        assert_eq!(
            desired_host(true, &unknown, 7, 0, ExecutionHost::Service),
            ExecutionHost::Service
        );
        assert_eq!(
            desired_host(true, &unknown, 7, 0, ExecutionHost::Local),
            ExecutionHost::Local
        );
        assert_eq!(
            desired_host(true, &ready, 7, 0, ExecutionHost::Local),
            ExecutionHost::Service
        );
        assert_eq!(
            desired_host(true, &ready, 7, 7, ExecutionHost::Service),
            ExecutionHost::Local
        );
        assert_eq!(
            desired_host(true, &ready, 8, 7, ExecutionHost::Local),
            ExecutionHost::Service
        );
        assert_eq!(
            desired_host(true, &foreign, 7, 0, ExecutionHost::Service),
            ExecutionHost::Local
        );
        assert_eq!(
            desired_host(false, &ready, 7, 0, ExecutionHost::Service),
            ExecutionHost::Local
        );
    }
}

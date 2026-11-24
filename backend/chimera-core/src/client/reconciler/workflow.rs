//! Shared serialized runtime intent, source admission and host preference.
use crate::actor_v2::endpoint::ExecutionHost;

pub fn effective_host(
    enable_service_mode: bool,
    service: Option<bool>,
    current: ExecutionHost,
) -> ExecutionHost {
    match (enable_service_mode, service) {
        (true, Some(true)) => ExecutionHost::Service,
        (true, None) => current,
        _ => ExecutionHost::Local,
    }
}

use super::{
    attempt::CommittedTargetAdvanced,
    checked::CheckedRuntimeInbox,
    committed::{SourceDomain, SourceVersions},
    plan::RunIntent,
    release::ServiceReleaseDebt,
    service_evidence::ServiceEvidence,
};
#[derive(Debug, Clone, Copy)]
pub enum AdmissionCommand {
    Start,
    Stop,
    Automatic,
    Checked {
        domain: SourceDomain,
        version: u64,
        has_target: bool,
    },
    Other,
}
/// Runtime-owned fields of the single production mailbox. Lower-host and
/// source actors own neither its desired run intent nor its checked cache.
pub struct RuntimeReconcilerState {
    pub dirty: bool,
    pub dirty_tick_scheduled: bool,
    pub source_dirty: bool,
    pub source_after_handoff: bool,
    pub run_intent: RunIntent,
    pub checked: CheckedRuntimeInbox,
    pub seen_sources: SourceVersions,
    pub service_handoff_maintenance: Option<String>,
    pub service_release_debt: ServiceReleaseDebt,
    pub service_recovery_maintenance: Option<String>,
    pub connection_interruption_maintenance: Option<String>,
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

pub enum RuntimeAdmission {
    Run,
    RejectStoppedChecked,
    RejectStaleChecked,
    SkipInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckedSourceNoticeKind {
    Current,
    Duplicate,
    Stale,
    Superseded,
}

pub struct CheckedSourceNotice {
    pub previous: Option<u64>,
    pub mode_requested: bool,
    pub kind: CheckedSourceNoticeKind,
}

impl RuntimeReconcilerState {
    /// Admission uses the same immutable intent/checked-version facts as
    /// the native command dispatcher, without depending on host commands.
    pub fn admit(&mut self, command: AdmissionCommand) -> RuntimeAdmission {
        match command {
            AdmissionCommand::Stop => {
                self.run_intent = RunIntent::Stopped;
                self.clear_dirty();
            }
            AdmissionCommand::Start => self.run_intent = RunIntent::Running,
            _ => {}
        }
        if self.run_intent == RunIntent::Stopped {
            match command {
                AdmissionCommand::Checked { .. } => return RuntimeAdmission::RejectStoppedChecked,
                AdmissionCommand::Automatic => return RuntimeAdmission::SkipInactive,
                _ => {}
            }
        }
        if let AdmissionCommand::Checked {
            domain,
            version,
            has_target,
        } = command
            && (self.seen_sources.version(domain) != Some(version) || !has_target)
        {
            return RuntimeAdmission::RejectStaleChecked;
        }
        RuntimeAdmission::Run
    }

    /// Observe the monotonic source publication and revoke any receipt that
    /// can no longer authorize an apply. Scheduler decisions stay with the
    /// same mailbox: stale runtime intent can request latest-source recovery.
    pub fn observe_checked_source(
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

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        self.source_dirty = false;
        self.source_after_handoff = false;
        self.connection_interruption_maintenance = None;
    }

    pub fn maintenance(&self, product: Option<&str>) -> Option<String> {
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
pub fn source_committed_during_round(
    trigger: &str,
    started: &SourceVersions,
    latest: &SourceVersions,
) -> bool {
    trigger == "SourceCommitted" && started != latest
}

pub fn committed_target_advanced(result: &anyhow::Result<()>) -> bool {
    result.as_ref().err().is_some_and(|error| {
        error.downcast_ref::<ProfilesSourceAdvanced>().is_some()
            || error.downcast_ref::<CommittedTargetAdvanced>().is_some()
    })
}

/// On a source fallback, never discard an acknowledged Mode effect silently:
/// only a *fresh* committed receipt with a mode intent and no matching Core
/// dispatch can create outstanding effect debt. Duplicate/out-of-order notices
/// do not create a second effect or overwrite the first operation's status.
pub fn checked_mode_effect_deferred(
    previous: Option<u64>,
    version: u64,
    mode_requested: bool,
    checked_dispatched: bool,
    running: bool,
) -> bool {
    mode_requested && !checked_dispatched && running && previous.is_none_or(|seen| version > seen)
}

pub fn deferred_mode_effect_degradation(version: u64) -> chimera_contracts::runtime::Degradation {
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

/// An older source generation became stale while the single runtime actor
/// was building or applying it. The actor must schedule another latest-source
/// round, but must not blindly retry arbitrary Core apply failures.
#[derive(Debug, thiserror::Error)]
#[error("Profiles source advanced while reconciling runtime: {before} -> {after}")]
pub struct ProfilesSourceAdvanced {
    pub before: u64,
    pub after: u64,
}

/// Apply ref's effective host preference to the same typed Service evidence
/// used by the owner planner. Chimera's foreign-owner and generation fence
/// must remain outside the generic upstream policy.
pub fn desired_host(
    enabled: bool,
    service: &ServiceEvidence,
    generation: u64,
    failed_generation: u64,
    owner: ExecutionHost,
) -> ExecutionHost {
    if matches!(service, ServiceEvidence::ForeignCore { .. }) {
        return ExecutionHost::Local;
    }
    let readiness = crate::client::core_lifecycle::workflow::service_usable(
        service.phase(),
        generation,
        (failed_generation != 0).then_some(failed_generation),
    );
    effective_host(enabled, readiness, owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_v2::service_policy::ServicePhase;

    #[test]
    fn stop_retains_source_versions_and_only_explicit_start_resumes_runtime() {
        let mut state = RuntimeReconcilerState::default();
        state.seen_sources.observe(SourceDomain::Profiles, 4);
        state.dirty = true;
        state.source_dirty = true;
        state.source_after_handoff = true;
        assert!(matches!(
            state.admit(AdmissionCommand::Stop),
            RuntimeAdmission::Run
        ));
        assert_eq!(state.run_intent, RunIntent::Stopped);
        assert!(!state.dirty && !state.source_dirty && !state.source_after_handoff);
        assert!(matches!(
            state.admit(AdmissionCommand::Automatic),
            RuntimeAdmission::SkipInactive
        ));
        assert!(matches!(
            state.admit(AdmissionCommand::Automatic),
            RuntimeAdmission::SkipInactive
        ));
        assert!(matches!(
            state.admit(AdmissionCommand::Automatic),
            RuntimeAdmission::SkipInactive
        ));
        assert_eq!(state.seen_sources.version(SourceDomain::Profiles), Some(4));
        assert!(matches!(
            state.admit(AdmissionCommand::Start),
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

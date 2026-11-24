//! Ref lifecycle decisions shared by the desktop owner and Core adapters.
use crate::actor_v2::{endpoint::ExecutionHost, service_policy::ServicePhase};
use chimera_core_manager::CoreKind;

/// A connection retry budget, separate from the OS daemon restart budget the
/// `ServiceActor` keeps: this one bounds IPC reconnections, that one bounds
/// elevated daemon starts, and a caller can exhaust either without touching
/// the other. Success does not re-arm it: repeated disconnects cannot create
/// a restart loop. Explicit Service selection/start re-arms recovery.
#[derive(Default)]
pub struct ServiceRecovery {
    attempts: u8,
    suppressed: bool,
    /// Ticks queue up behind a long attempt, so the first one after it would
    /// otherwise fire the next one immediately. This is what makes the
    /// interval a floor between attempts rather than between their starts.
    next_attempt: Option<tokio::time::Instant>,
    // Chimera observes host changes through a compatibility port.
    owner: Option<ExecutionHost>,
}

/// The connection attempts allowed before explicit service selection re-arms recovery.
const RECOVERY_BUDGET: u8 = 3;

impl ServiceRecovery {
    /// Explicit service selection or start re-arms connection recovery.
    pub fn rearm(&mut self) {
        self.attempts = 0;
        self.suppressed = false;
        self.next_attempt = None;
    }

    /// A terminal connection failure pauses automatic reconnection.
    fn pause(&mut self) {
        self.suppressed = true;
    }

    /// Leaving the service host or explicitly stopping its daemon suppresses reconnection.
    pub fn suppress(&mut self) {
        self.suppressed = true;
    }

    pub fn armed(&self) -> bool {
        !self.suppressed && self.attempts < RECOVERY_BUDGET
    }

    pub fn follow_host(&mut self, host: ExecutionHost) {
        if self.owner == Some(host) {
            return;
        }
        self.owner = Some(host);
        match host {
            ExecutionHost::Service => self.rearm(),
            ExecutionHost::Local => self.suppress(),
        }
    }

    pub fn blocks_handoff(&self, endpoint_degraded: bool) -> bool {
        self.owner == Some(ExecutionHost::Service) && self.armed() && endpoint_degraded
    }

    pub fn due(&self, endpoint_degraded: bool, closing: bool) -> bool {
        self.blocks_handoff(endpoint_degraded)
            && !closing
            && self
                .next_attempt
                .is_none_or(|at| tokio::time::Instant::now() >= at)
    }

    /// Atomically consume one budget slot only when the selected endpoint
    /// still needs recovery. The caller holds its existing owner lock.
    pub fn begin(&mut self, endpoint_degraded: bool, closing: bool) -> bool {
        if !self.due(endpoint_degraded, closing) {
            return false;
        }
        self.attempts += 1;
        true
    }

    pub fn finish_attempt(&mut self, failed: bool, terminal_error: bool, endpoint_degraded: bool) {
        self.next_attempt = Some(tokio::time::Instant::now() + super::RECOVERY_INTERVAL);
        if terminal_error {
            self.pause();
        }
        if self.attempts >= RECOVERY_BUDGET && (failed || endpoint_degraded) {
            tracing::warn!(
                "service endpoint recovery budget spent; explicitly select or start Service to re-arm"
            );
        }
    }
}

/// Whether the service host can run the core, given the daemon's `phase`
/// and the ready generation read with it: `None` while that cannot be
/// told.
pub fn service_usable(phase: ServicePhase, generation: u64, failed: Option<u64>) -> Option<bool> {
    phase
        .readiness()
        .map(|ready| ready && failed != Some(generation))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartIntent {
    Owed,
    NotOwed,
}

/// The selected executable is desired state, not a proof of what is running.
/// This is ref replace_binary's stop/restart decision, with the caller
/// supplying its authoritative stopped/active-kind evidence.
pub fn binary_replacement(
    not_proven_stopped: bool,
    applied_kind: Option<CoreKind>,
    target_kind: CoreKind,
    desired_matches_target: bool,
) -> (bool, RestartIntent) {
    let needs_stop =
        not_proven_stopped && (applied_kind.is_none() || applied_kind == Some(target_kind));
    let restart = desired_matches_target || needs_stop;
    (
        needs_stop,
        if restart {
            RestartIntent::Owed
        } else {
            RestartIntent::NotOwed
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_generation_does_not_convert_unknown_readiness_to_false() {
        assert_eq!(service_usable(ServicePhase::Unknown, 8, Some(8)), None);
        assert_eq!(service_usable(ServicePhase::Ready, 8, Some(8)), Some(false));
        assert_eq!(service_usable(ServicePhase::Ready, 9, Some(8)), Some(true));
    }

    #[test]
    fn ref_recovery_budget_does_not_rearm_on_same_host_or_success() {
        let mut recovery = ServiceRecovery::default();
        recovery.follow_host(ExecutionHost::Service);
        for _ in 0..RECOVERY_BUDGET {
            assert!(recovery.begin(true, false));
        }
        recovery.finish_attempt(false, false, false);
        recovery.follow_host(ExecutionHost::Service);
        assert!(!recovery.armed());
        recovery.rearm();
        assert!(recovery.armed());
        assert!(recovery.begin(true, false));
        recovery.finish_attempt(false, true, true);
        assert!(!recovery.armed());
    }

    #[test]
    fn replacement_stops_an_unknown_running_binary_but_not_an_unrelated_proven_one() {
        assert_eq!(
            binary_replacement(true, None, CoreKind::Mihomo, false),
            (true, RestartIntent::Owed)
        );
        assert_eq!(
            binary_replacement(true, Some(CoreKind::ClashRust), CoreKind::Mihomo, false),
            (false, RestartIntent::NotOwed)
        );
        assert_eq!(
            binary_replacement(false, None, CoreKind::Mihomo, true),
            (false, RestartIntent::Owed)
        );
    }
}

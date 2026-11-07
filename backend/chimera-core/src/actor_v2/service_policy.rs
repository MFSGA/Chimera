//! Pure service lifecycle phase and restart policy extracted from
//! ref/backend/nyanpasu-core/src/actor_v2/service_actor.rs.
//! The ractor mailbox and elevated OS process control remain host adapters
//! until Chimera's service contracts are fully migrated.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Probing,
    NotInstalled,
    DaemonStopped,
    Installing,
    StartingDaemon,
    Ready,
    /// Version gate failed closed: upgrade required, never downgraded to.
    Incompatible,
    Restarting,
    /// Auto-restart budget spent; waits for `EnsureReady` or a successful explicit start.
    Exhausted,
    Uninstalling,
    /// The probe itself failed or timed out (F5): what the daemon actually
    /// is cannot be determined. Never treated as `DaemonStopped` -- an
    /// unreachable daemon might still be holding a core open, so callers
    /// that gate on "no core held" (the uninstall guard, `RecoverEndpoint`'s
    /// restart) must refuse rather than proceed.
    Unknown,
}

impl ServicePhase {
    /// Whether this phase settles if the daemon can serve: `None` for a
    /// transition still under way and for a probe that could not tell.
    pub fn readiness(self) -> Option<bool> {
        match self {
            Self::Ready => Some(true),
            Self::NotInstalled | Self::DaemonStopped | Self::Incompatible | Self::Exhausted => {
                Some(false)
            }
            Self::Probing
            | Self::Installing
            | Self::StartingDaemon
            | Self::Restarting
            | Self::Uninstalling
            | Self::Unknown => None,
        }
    }
}

/// A probe is Ready only when the daemon is compatible AND the service
/// actually owns the runtime (a Chimera-specific authority proof).
/// V1 status or failed probes must never be a stop or version proof.
pub fn phase_for_probe(
    status: chimera_ipc::types::ServiceStatus,
    compatible: bool,
    runtime_owned: bool,
) -> ServicePhase {
    match status {
        chimera_ipc::types::ServiceStatus::Running if compatible && runtime_owned => {
            ServicePhase::Ready
        }
        chimera_ipc::types::ServiceStatus::Running => ServicePhase::Incompatible,
        chimera_ipc::types::ServiceStatus::Stopped => ServicePhase::DaemonStopped,
        chimera_ipc::types::ServiceStatus::NotInstalled => ServicePhase::NotInstalled,
    }
}

/// Upstream's generation rule: a transitional or unknowable probe cannot
/// reset readiness, and only a settled false -> true starts a generation.
pub fn settled_readiness(was_ready: bool, generation: u64, phase: ServicePhase) -> (bool, u64) {
    match phase.readiness() {
        Some(true) if !was_ready => (true, generation.wrapping_add(1)),
        Some(ready) => (ready, generation),
        None => (was_ready, generation),
    }
}

/// Only an explicitly stopped daemon can be restarted. An Unknown probe
/// or an already exhausted service cannot trigger an elevated command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRestartDecision {
    Ignore,
    Restart { next_attempt: u8 },
    Exhaust,
}

pub fn service_restart_decision(
    status: chimera_ipc::types::ServiceStatus,
    attempts: u8,
    exhausted: bool,
    budget: u8,
) -> ServiceRestartDecision {
    if exhausted || status != chimera_ipc::types::ServiceStatus::Stopped {
        return ServiceRestartDecision::Ignore;
    }
    if attempts >= budget {
        ServiceRestartDecision::Exhaust
    } else {
        ServiceRestartDecision::Restart {
            next_attempt: attempts.saturating_add(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_ipc::types::ServiceStatus;

    #[test]
    fn transitional_probes_never_invent_a_settled_state() {
        for phase in [
            ServicePhase::Probing,
            ServicePhase::Installing,
            ServicePhase::StartingDaemon,
            ServicePhase::Restarting,
            ServicePhase::Uninstalling,
            ServicePhase::Unknown,
        ] {
            assert_eq!(phase.readiness(), None, "{phase:?}");
            assert_eq!(settled_readiness(true, 7, phase), (true, 7));
            assert_eq!(settled_readiness(false, 7, phase), (false, 7));
        }
    }

    #[test]
    fn true_generation_only_advances_after_a_proven_not_ready_state() {
        assert_eq!(settled_readiness(false, 4, ServicePhase::Ready), (true, 5));
        assert_eq!(settled_readiness(true, 5, ServicePhase::Ready), (true, 5));
        assert_eq!(settled_readiness(true, 5, ServicePhase::Unknown), (true, 5));
        assert_eq!(
            settled_readiness(true, 5, ServicePhase::Exhausted),
            (false, 5)
        );
        assert_eq!(settled_readiness(false, 5, ServicePhase::Ready), (true, 6));
    }

    #[test]
    fn probe_requires_runtime_ownership_as_well_as_version_compatibility() {
        assert_eq!(
            phase_for_probe(ServiceStatus::Running, true, true),
            ServicePhase::Ready
        );
        assert_eq!(
            phase_for_probe(ServiceStatus::Running, true, false),
            ServicePhase::Incompatible
        );
        assert_eq!(
            phase_for_probe(ServiceStatus::Running, false, true),
            ServicePhase::Incompatible
        );
        assert_eq!(
            phase_for_probe(ServiceStatus::Stopped, true, false),
            ServicePhase::DaemonStopped
        );
        assert_eq!(
            phase_for_probe(ServiceStatus::NotInstalled, true, false),
            ServicePhase::NotInstalled
        );
    }

    #[test]
    fn exhausted_or_unconfirmed_daemon_never_triggers_an_automatic_restart() {
        assert_eq!(
            service_restart_decision(ServiceStatus::Running, 0, false, 3),
            ServiceRestartDecision::Ignore
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::NotInstalled, 0, false, 3),
            ServiceRestartDecision::Ignore
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 0, false, 3),
            ServiceRestartDecision::Restart { next_attempt: 1 }
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 2, false, 3),
            ServiceRestartDecision::Restart { next_attempt: 3 }
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 3, false, 3),
            ServiceRestartDecision::Exhaust
        );
        assert_eq!(
            service_restart_decision(ServiceStatus::Stopped, 0, true, 3),
            ServiceRestartDecision::Ignore
        );
    }
}

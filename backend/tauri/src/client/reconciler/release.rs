//! Reference S2/S5 Service daemon release maintenance policy.
//!
//! This tracks a *separate* daemon-release debt after Local has already
//! converged. It never grants authority to stop a Service Core: that belongs
//! to the existing instance-bound owner handoff.
use crate::client::application_workflow::startup::ServiceEvidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReleaseObservation {
    Owed,
    Cleared,
    Unproven,
}

pub(crate) fn observation(
    service_preferred: bool,
    verified_local: bool,
    evidence: &ServiceEvidence,
) -> ReleaseObservation {
    if service_preferred {
        return ReleaseObservation::Cleared;
    }
    if !verified_local {
        return ReleaseObservation::Unproven;
    }
    match evidence {
        ServiceEvidence::CoreStopped {
            ready: true,
            runtime_owned: true,
            ..
        } => ReleaseObservation::Owed,
        ServiceEvidence::Absent { .. }
        | ServiceEvidence::ForeignCore { .. }
        | ServiceEvidence::CoreStopped {
            runtime_owned: false,
            ..
        } => ReleaseObservation::Cleared,
        ServiceEvidence::CoreStopped { ready: false, .. }
        | ServiceEvidence::CoreRunning { .. }
        | ServiceEvidence::CoreStoppedWithReason { .. }
        | ServiceEvidence::Unreadable { .. } => ReleaseObservation::Unproven,
    }
}

/// The daemon-stop command is queued after the Local Core converges, but
/// acquiring the Service transition lease may race a daemon restart, a new
/// Ready generation, or another instance's ownership. A generic "stopped"
/// answer is not sufficient: only the *same* proven daemon generation may
/// consume the queued release intent. The Service transition lease then
/// excludes further operations while the stop is performed.
pub(crate) fn same_release_owner(
    before: &ServiceEvidence,
    before_generation: u64,
    after: &ServiceEvidence,
    after_generation: u64,
) -> bool {
    before_generation == after_generation
        && before == after
        && observation(false, true, before) == ReleaseObservation::Owed
        && observation(false, true, after) == ReleaseObservation::Owed
}

#[derive(Debug, Default)]
pub(crate) struct ServiceReleaseDebt {
    owed: bool,
    pending_failure: Option<String>,
    in_flight: bool,
}

impl ServiceReleaseDebt {
    pub(crate) fn observe(&mut self, evidence: ReleaseObservation) {
        match evidence {
            ReleaseObservation::Owed => self.owed = true,
            ReleaseObservation::Cleared => {
                self.owed = false;
                self.pending_failure = None;
            }
            ReleaseObservation::Unproven => {}
        }
    }

    pub(crate) fn begin(&mut self, explicit_retry: bool) -> bool {
        if !self.owed || self.in_flight || (self.pending_failure.is_some() && !explicit_retry) {
            return false;
        }
        self.in_flight = true;
        self.pending_failure = None;
        true
    }

    pub(crate) fn finish(&mut self, outcome: Result<(), String>) {
        self.in_flight = false;
        match outcome {
            Ok(()) => {
                self.owed = false;
                self.pending_failure = None;
            }
            Err(error) => {
                self.owed = true;
                self.pending_failure = Some(error);
            }
        }
    }

    pub(crate) fn deferred(&mut self, error: String) {
        self.finish(Err(error));
    }

    pub(crate) fn maintenance(&self) -> Option<String> {
        if !self.owed {
            return None;
        }
        if let Some(error) = &self.pending_failure {
            Some(format!("Service daemon release pending: {error}"))
        } else if self.in_flight {
            Some("Service daemon release in progress".into())
        } else {
            Some("Service daemon release owed".into())
        }
    }

    #[cfg(test)]
    pub(crate) fn owed(&self) -> bool {
        self.owed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::actor_v2::service_policy::ServicePhase;

    #[test]
    fn ownership_evidence_must_prove_an_owned_stopped_daemon_core() {
        let owned = ServiceEvidence::CoreStopped {
            ready: true,
            phase: ServicePhase::Ready,
            runtime_owned: true,
        };
        assert_eq!(observation(false, true, &owned), ReleaseObservation::Owed);
        assert_eq!(observation(true, true, &owned), ReleaseObservation::Cleared);
        assert_eq!(
            observation(false, false, &owned),
            ReleaseObservation::Unproven
        );
        let foreign = ServiceEvidence::ForeignCore {
            ready: true,
            phase: ServicePhase::Ready,
        };
        assert_eq!(
            observation(false, true, &foreign),
            ReleaseObservation::Cleared
        );
        let unsafe_core = ServiceEvidence::CoreRunning {
            ready: true,
            phase: ServicePhase::Ready,
            instance: None,
            revision: None,
        };
        assert_eq!(
            observation(false, true, &unsafe_core),
            ReleaseObservation::Unproven
        );
        assert_eq!(
            observation(
                false,
                true,
                &ServiceEvidence::Unreadable {
                    phase: ServicePhase::Unknown,
                },
            ),
            ReleaseObservation::Unproven
        );
    }

    #[test]
    fn release_requires_same_proven_daemon_generation_after_lease() {
        let original = ServiceEvidence::CoreStopped {
            ready: true,
            phase: ServicePhase::Ready,
            runtime_owned: true,
        };
        assert!(same_release_owner(&original, 12, &original, 12));
        assert!(!same_release_owner(&original, 12, &original, 13));

        for changed in [
            ServiceEvidence::CoreStopped {
                ready: true,
                phase: ServicePhase::Ready,
                runtime_owned: false,
            },
            ServiceEvidence::CoreRunning {
                ready: true,
                phase: ServicePhase::Ready,
                instance: None,
                revision: None,
            },
            ServiceEvidence::ForeignCore {
                ready: true,
                phase: ServicePhase::Ready,
            },
            ServiceEvidence::Unreadable {
                phase: ServicePhase::Unknown,
            },
        ] {
            assert!(!same_release_owner(&original, 12, &changed, 12));
        }
    }

    #[test]
    fn failed_release_remains_owed_without_unbounded_automatic_retries() {
        let mut debt = ServiceReleaseDebt::default();
        debt.observe(ReleaseObservation::Owed);
        assert!(debt.begin(false));
        assert!(!debt.begin(false));
        debt.finish(Err("lease unavailable".into()));
        assert!(debt.maintenance().unwrap().contains("lease unavailable"));
        debt.observe(ReleaseObservation::Unproven);
        assert!(debt.owed());
        assert!(!debt.begin(false));
        assert!(debt.begin(true));
        debt.finish(Ok(()));
        assert!(!debt.owed());
        assert!(debt.maintenance().is_none());
    }

    #[test]
    fn changed_service_preference_cancels_debt_and_foreign_never_stopped() {
        let mut debt = ServiceReleaseDebt::default();
        debt.observe(ReleaseObservation::Owed);
        assert!(debt.begin(false));
        debt.deferred("service host changed".into());
        debt.observe(ReleaseObservation::Cleared);
        assert!(!debt.begin(true));
        assert!(debt.maintenance().is_none());
    }
}

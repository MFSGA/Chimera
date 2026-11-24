//! Reference S2/S5 Service daemon release maintenance policy.
//!
//! This tracks a *separate* daemon-release debt after Local has already
//! converged. It never grants authority to stop a Service Core: that belongs
//! to the existing instance-bound owner handoff.
use crate::client::reconciler::service_evidence::ServiceEvidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseObservation {
    Owed,
    Cleared,
    Unproven,
}

pub fn observation(
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
pub fn same_release_owner(
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
pub struct ServiceReleaseDebt {
    owed: bool,
    pending_failure: Option<String>,
    in_flight: bool,
}

impl ServiceReleaseDebt {
    pub fn observe(&mut self, evidence: ReleaseObservation) {
        match evidence {
            ReleaseObservation::Owed => self.owed = true,
            ReleaseObservation::Cleared => {
                self.owed = false;
                self.pending_failure = None;
            }
            ReleaseObservation::Unproven => {}
        }
    }

    pub fn begin(&mut self, explicit_retry: bool) -> bool {
        if !self.owed || self.in_flight || (self.pending_failure.is_some() && !explicit_retry) {
            return false;
        }
        self.in_flight = true;
        self.pending_failure = None;
        true
    }

    pub fn finish(&mut self, outcome: Result<(), String>) {
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

    pub fn deferred(&mut self, error: String) {
        self.finish(Err(error));
    }

    pub fn maintenance(&self) -> Option<String> {
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
    pub fn owed(&self) -> bool {
        self.owed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_v2::service_policy::ServicePhase;

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

/// Execute a release only while a verified Local instance and the stopped
/// Service owner remain stable through the nonblocking transition lease.
pub async fn maintain_service_release<R, S, L>(
    core: &(impl crate::client::core_lifecycle::ports::RuntimeObservationPort + ?Sized),
    recovery: &parking_lot::Mutex<crate::client::core_lifecycle::workflow::ServiceRecovery>,
    observe_release: impl Fn() -> R,
    observe_service: impl Fn() -> S,
    begin_transition: impl FnOnce() -> L,
) -> anyhow::Result<()>
where
    R: std::future::Future<Output = ReleaseObservation>,
    S: std::future::Future<Output = (ServiceEvidence, u64)>,
    L: std::future::Future<
            Output = anyhow::Result<
                Box<dyn crate::client::core_lifecycle::ports::ServiceTransitionLease>,
            >,
        >,
{
    use super::decision::same_core_status;
    use chimera_ipc::api::status::CoreState;
    let before = core.observe_core_status().await?;
    anyhow::ensure!(
        observe_release().await == ReleaseObservation::Owed,
        "Service daemon release is not authorized by a verified Local owner"
    );
    let (service_before, generation_before) = observe_service().await;
    anyhow::ensure!(
        observation(false, true, &service_before) == ReleaseObservation::Owed,
        "Service daemon stopped-owner evidence was lost before acquiring the transition lease"
    );
    // Nonblocking lock acquisition preserves ServiceActor -> CoreLifecycle
    // ordering: a UI command can hold this lock while awaiting our mailbox.
    let mut lease = begin_transition().await?;
    let local_after_lease = core.observe_core_status().await?;
    anyhow::ensure!(
        same_core_status(&before, &local_after_lease)
            && matches!(local_after_lease.state, CoreState::Running),
        "Local Core instance changed before the queued Service release was admitted"
    );
    anyhow::ensure!(
        observe_release().await == ReleaseObservation::Owed,
        "Service owner evidence changed after acquiring the transition lease"
    );
    let (service_after_lease, generation_after_lease) = observe_service().await;
    anyhow::ensure!(
        same_release_owner(
            &service_before,
            generation_before,
            &service_after_lease,
            generation_after_lease,
        ),
        "Service daemon owner or readiness generation changed before the queued release"
    );
    // The lease proves the stop still targets the same stopped, locally
    // owned daemon. A newer readiness generation must use a new release
    // decision rather than reusing an older convergence observation.
    recovery.lock().suppress();
    lease.stop_daemon().await?;
    lease.confirm_stopped().await?;
    let (after, _) = observe_service().await;
    anyhow::ensure!(
        matches!(after, ServiceEvidence::Absent { .. }),
        "Service stop was acknowledged but a fresh probe did not prove daemon shutdown: {after:?}"
    );
    let current = core.observe_core_status().await?;
    anyhow::ensure!(
        same_core_status(&before, &current) && matches!(current.state, CoreState::Running),
        "Local Core changed while retiring the Service daemon"
    );

    Ok(())
}

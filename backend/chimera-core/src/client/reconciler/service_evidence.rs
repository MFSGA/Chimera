use super::plan::ServiceEvidence as ReconcilerServiceEvidence;
use crate::actor_v2::{service_policy::ServicePhase, service_status::ServiceHostStatus};
use chimera_core_manager::{CoreOwner, Epoch, RevisionId};
use chimera_ipc::{api::status::ConfigRevisionInfo, types::ServiceStatus};
/// What one Service probe says about a core the daemon may hold.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceEvidence {
    /// Not installed, or the daemon is stopped: it holds no core.
    Absent { phase: ServicePhase },
    /// The daemon answers, and its core is stopped.
    CoreStopped {
        ready: bool,
        phase: ServicePhase,
        runtime_owned: bool,
    },
    /// The daemon reports a stopped core with a reason. For this installation,
    /// error stops may hide a quarantined process, so the manager must recover
    /// before another host is allowed to start. A foreign manager stays
    /// unproven and must never receive this installation's recovery command.
    CoreStoppedWithReason {
        ready: bool,
        phase: ServicePhase,
        reason: String,
        runtime_owned: bool,
    },
    /// The daemon answers, and its core is not stopped.
    CoreRunning {
        ready: bool,
        phase: ServicePhase,
        instance: Option<String>,
        revision: Option<ConfigRevisionInfo>,
    },
    /// The daemon answers for another app instance; this instance must leave
    /// that core alone.
    ForeignCore { ready: bool, phase: ServicePhase },
    /// The probe failed, or the answer cannot establish whether a core is held.
    Unreadable { phase: ServicePhase },
}

impl ServiceEvidence {
    pub fn from_status(status: &ServiceHostStatus) -> Self {
        if status.phase == ServicePhase::Unknown {
            return Self::Unreadable {
                phase: status.phase,
            };
        }

        let ready = status.phase == ServicePhase::Ready;
        match (status.status, status.server.as_ref()) {
            (ServiceStatus::NotInstalled | ServiceStatus::Stopped, _) => Self::Absent {
                phase: status.phase,
            },
            (ServiceStatus::Running, None) => Self::Unreadable {
                phase: status.phase,
            },
            (ServiceStatus::Running, Some(server)) => match &server.core_infos.detail {
                Some(chimera_ipc::api::status::CoreStateDetail::Stopped {
                    reason: Some(reason),
                }) => Self::CoreStoppedWithReason {
                    ready,
                    phase: status.phase,
                    reason: reason.clone(),
                    runtime_owned: status.runtime_owned,
                },
                Some(chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }) => {
                    Self::CoreStopped {
                        ready,
                        phase: status.phase,
                        runtime_owned: status.runtime_owned,
                    }
                }
                Some(_) if !status.runtime_owned => Self::ForeignCore {
                    ready,
                    phase: status.phase,
                },
                Some(_) => Self::CoreRunning {
                    ready,
                    phase: status.phase,
                    instance: server.core_infos.instance_id.clone(),
                    revision: server.core_infos.revision.clone(),
                },
                None => Self::Unreadable {
                    phase: status.phase,
                },
            },
        }
    }

    pub fn phase(&self) -> ServicePhase {
        match self {
            Self::Absent { phase }
            | Self::CoreStopped { phase, .. }
            | Self::CoreStoppedWithReason { phase, .. }
            | Self::CoreRunning { phase, .. }
            | Self::ForeignCore { phase, .. }
            | Self::Unreadable { phase } => *phase,
        }
    }

    pub fn proves_no_owned_core(&self) -> bool {
        matches!(
            self,
            Self::Absent { .. } | Self::CoreStopped { .. } | Self::ForeignCore { .. }
        )
    }

    pub fn proves_runtime_owner(&self) -> bool {
        match self {
            Self::CoreStopped { runtime_owned, .. }
            | Self::CoreStoppedWithReason { runtime_owned, .. } => *runtime_owned,
            Self::CoreRunning { .. } => true,
            Self::Absent { .. } | Self::ForeignCore { .. } | Self::Unreadable { .. } => false,
        }
    }

    /// Build the server-side conditional-stop token from complete startup
    /// evidence. Missing or malformed fields fail closed.
    pub fn expected_owner(&self) -> Result<CoreOwner, String> {
        let Self::CoreRunning {
            instance: Some(instance),
            revision: Some(revision),
            ..
        } = self
        else {
            return Err("the Service probe did not provide a complete Core owner token".into());
        };
        let epoch = Epoch::try_from(revision.epoch)
            .map_err(|_| "the Service probe returned a zero Core revision epoch")?;
        CoreOwner::from_parts(
            instance,
            RevisionId {
                epoch,
                generation: revision.generation,
                effective_hash: revision.effective_hash.clone(),
            },
        )
    }
}

/// Adapt Chimera's quarantine evidence into ref's runtime ownership planner.
/// A stopped Core with an error reason may still own quarantined processes:
/// it must never be presented to the planner as an ordinary stopped Core.
pub fn reconciler_service_evidence(service: &ServiceEvidence) -> ReconcilerServiceEvidence {
    match service {
        ServiceEvidence::Absent { phase } => ReconcilerServiceEvidence::Absent { phase: *phase },
        ServiceEvidence::CoreStopped { ready, phase, .. } => {
            ReconcilerServiceEvidence::CoreStopped {
                ready: *ready,
                phase: *phase,
            }
        }
        ServiceEvidence::CoreStoppedWithReason { phase, .. }
        | ServiceEvidence::Unreadable { phase } => {
            ReconcilerServiceEvidence::Unreadable { phase: *phase }
        }
        ServiceEvidence::CoreRunning {
            ready,
            phase,
            instance,
            revision,
        } => ReconcilerServiceEvidence::CoreRunning {
            ready: *ready,
            phase: *phase,
            instance: instance.clone(),
            revision: revision.clone(),
        },
        ServiceEvidence::ForeignCore { ready, phase } => ReconcilerServiceEvidence::ForeignCore {
            ready: *ready,
            phase: *phase,
        },
    }
}

pub fn startup_recovery_evidence_changed(
    previous: &ServiceHostStatus,
    current: &ServiceHostStatus,
    owner_unproven: bool,
) -> bool {
    let readiness_changed = previous.phase.readiness() != current.phase.readiness()
        || current.ready_generation != previous.ready_generation;
    let owner_evidence_changed = owner_unproven
        && ServiceEvidence::from_status(previous) != ServiceEvidence::from_status(current);
    readiness_changed || owner_evidence_changed
}

/// A service watch can republish an unchanged daemon observation on each poll.
/// Every redundant message otherwise enters the sole CoreLifecycle mailbox,
/// where service follow-up performs async Core/source reads and can starve
/// Checked admission. Match ref's evidence-driven Service ownership planning:
/// only changes that can affect a host handoff, generation, or retry are events.
///
/// Deliberately compare projected *semantic evidence*, not the raw server DTO
/// (which can contain volatile reporting fields unrelated to Core ownership).
pub fn service_followup_evidence_changed(
    previous: &ServiceHostStatus,
    current: &ServiceHostStatus,
) -> bool {
    previous.phase != current.phase
        || previous.ready_generation != current.ready_generation
        || previous.settled_ready != current.settled_ready
        || previous.status != current.status
        || previous.runtime_owned != current.runtime_owned
        || previous.compat != current.compat
        || previous.restart_attempts != current.restart_attempts
        || ServiceEvidence::from_status(previous) != ServiceEvidence::from_status(current)
}

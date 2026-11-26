//! Read-only projection of ref ServiceActor into Chimera's legacy Service
//! status DTO. This is an adapter, never a second lifecycle status publisher.
//! The typed ServiceClient owns the daemon phase, retry budget and readiness
//! generation; Chimera only adds its installation-directory ownership proof.

#[cfg(test)]
use chimera_core::actor_v2::service_router::ServicePhase as ActorServicePhase;
use chimera_core::actor_v2::{
    service_policy::ServicePhase, service_router::ServiceHostStatus as ActorServiceStatus,
    service_status::ServiceHostStatus,
};
use chimera_ipc::types::{ServiceStatus, StatusInfo};

pub(super) fn project_actor_service_status(actor: &ActorServiceStatus) -> ServiceHostStatus {
    let info = StatusInfo {
        name: actor.name.clone(),
        version: actor.version.clone(),
        status: actor.status,
        server: actor.server.clone(),
    };
    // A compatible daemon owned by a different Chimera installation is NOT
    // authorized to host this application's runtime. The ownership proof
    // never comes from ServiceActor's readiness alone.
    let runtime_owned = actor.status == ServiceStatus::Running
        && info.server.is_some()
        && crate::core::service::is_service_runtime_owned(&info);
    project_actor_service_status_with_ownership(actor, runtime_owned)
}

use chimera_core::client::reconciler::service_observer::project_actor_service_status_with_ownership;

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::service::compat::ServiceCompat;

    fn actor_status(phase: ActorServicePhase) -> ActorServiceStatus {
        ActorServiceStatus {
            name: "chimera-service".into(),
            version: "1.10.1".into(),
            status: ServiceStatus::Running,
            server: None,
            phase,
            compat: ServiceCompat::Compatible {
                server_version: "1.10.1".into(),
            },
            restart_attempts: 2,
            settled_ready: true,
            ready_generation: 7,
        }
    }

    #[test]
    fn ref_service_actor_is_the_source_of_phase_attempts_and_generation() {
        let ready = actor_status(ActorServicePhase::Ready);
        let projected = project_actor_service_status_with_ownership(&ready, true);
        assert_eq!(projected.phase, ServicePhase::Ready);
        assert_eq!(projected.restart_attempts, 2);
        assert_eq!(projected.ready_generation, 7);
        assert!(projected.runtime_owned);
        assert!(projected.settled_ready);

        let restarting = actor_status(ActorServicePhase::Restarting);
        let projected = project_actor_service_status_with_ownership(&restarting, true);
        assert_eq!(projected.phase, ServicePhase::Restarting);
        assert_eq!(projected.restart_attempts, 2);
        assert_eq!(projected.ready_generation, 7);

        let exhausted = actor_status(ActorServicePhase::Exhausted);
        assert_eq!(
            project_actor_service_status_with_ownership(&exhausted, true).phase,
            ServicePhase::Exhausted
        );
    }

    #[test]
    fn foreign_runtime_cannot_claim_ready_even_if_ref_actor_is_ready() {
        let ready = actor_status(ActorServicePhase::Ready);
        let projected = project_actor_service_status_with_ownership(&ready, false);
        assert_eq!(projected.phase, ServicePhase::Incompatible);
        assert!(!projected.runtime_owned);
        assert!(!projected.settled_ready);
        assert_eq!(projected.ready_generation, 7);
        assert_eq!(projected.restart_attempts, 2);

        let unknown = actor_status(ActorServicePhase::Unknown);
        let projected = project_actor_service_status_with_ownership(&unknown, false);
        assert_eq!(projected.phase, ServicePhase::Unknown);
        assert!(!projected.settled_ready);
        assert!(!projected.runtime_owned);
    }

    #[test]
    fn service_projection_without_daemon_evidence_is_not_ready() {
        let mut ready = actor_status(ActorServicePhase::Ready);
        ready.server = None;
        let projected = project_actor_service_status(&ready);
        assert_eq!(projected.phase, ServicePhase::Incompatible);
        assert!(!projected.runtime_owned);
    }
}

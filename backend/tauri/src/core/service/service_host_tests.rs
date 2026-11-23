//! Windows TUN Service host proof: the ref CoreActor, never the legacy
//! two-state Facade, decides whether a privileged runtime is actually active.

use super::{actor_core_running, actor_service_core_running};
use chimera_core::actor_v2::{
    endpoint::{CoreStatusSnapshot, ExecutionHost},
    router::{CoreStatusProjection, EndpointConnectivity},
};
use chimera_ipc::api::status::CoreStateDetail;

fn running_local() -> CoreStatusProjection {
    CoreStatusProjection {
        host: ExecutionHost::Local,
        generation: 9,
        connectivity: EndpointConnectivity::Connected,
        snapshot: Some(CoreStatusSnapshot {
            instance_id: Some("core-before-tun-handoff".into()),
            controller: None,
            state: Some(CoreStateDetail::Running {
                epoch: 5,
                pid: 1234,
            }),
            state_changed_at: 3,
            revision: None,
            source_hash: None,
            healthy: Some(true),
            applied_kind: None,
        }),
    }
}

#[test]
fn preflight_recognizes_running_local_but_requires_service_after_handoff() {
    let local = running_local();
    assert!(actor_core_running(&local));
    assert!(!actor_service_core_running(&local));

    let mut service = local.clone();
    service.host = ExecutionHost::Service;
    service.generation += 1;
    service.snapshot.as_mut().unwrap().instance_id = Some("service-core".into());
    assert!(actor_core_running(&service));
    assert!(actor_service_core_running(&service));
}

#[test]
fn preflight_rejects_handoff_degraded_unknown_and_unhealthy_runtime() {
    let running = running_local();

    let mut handoff = running.clone();
    handoff.connectivity = EndpointConnectivity::HandingOff {
        from: ExecutionHost::Local,
        to: ExecutionHost::Service,
    };
    assert!(!actor_core_running(&handoff));

    let mut degraded = running.clone();
    degraded.connectivity = EndpointConnectivity::Degraded {
        desired: ExecutionHost::Service,
        reason: "daemon went away".into(),
    };
    assert!(!actor_core_running(&degraded));

    let mut disconnected = running.clone();
    disconnected.connectivity = EndpointConnectivity::ShutDown;
    assert!(!actor_core_running(&disconnected));

    let mut starting = running.clone();
    starting.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Starting { epoch: 6 });
    assert!(!actor_core_running(&starting));

    let mut stopped = running.clone();
    stopped.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Stopped { reason: None });
    assert!(!actor_core_running(&stopped));

    let mut unknown_instance = running.clone();
    unknown_instance.snapshot.as_mut().unwrap().instance_id = None;
    assert!(!actor_core_running(&unknown_instance));

    let mut unhealthy = running.clone();
    unhealthy.snapshot.as_mut().unwrap().healthy = Some(false);
    assert!(!actor_core_running(&unhealthy));

    let mut empty_snapshot = running;
    empty_snapshot.snapshot = None;
    assert!(!actor_core_running(&empty_snapshot));
}

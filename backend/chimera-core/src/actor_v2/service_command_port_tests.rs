use super::*;
use crate::actor_v2::{
    endpoint::EndpointHandle, service_endpoint_factory::IpcServiceEndpointFactory,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct RecordingService {
    probes: AtomicUsize,
    starts: AtomicUsize,
    stops: AtomicUsize,
    restarts: AtomicUsize,
    fail_probe: AtomicBool,
    fail_restart: AtomicBool,
    stopped: AtomicBool,
}

#[async_trait::async_trait]
impl chimera_contracts::service::ServiceProcessControl for RecordingService {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        if self.fail_probe.load(Ordering::SeqCst) {
            return Err(ServiceCommandError::RunServiceStatus {
                source: std::io::Error::other("the OS status helper failed"),
            });
        }
        serde_json::to_string(&StatusInfo {
            name: "chimera-service".into(),
            version: "1.10.1".into(),
            status: if self.stopped.load(Ordering::SeqCst) {
                chimera_ipc::types::ServiceStatus::Stopped
            } else {
                chimera_ipc::types::ServiceStatus::NotInstalled
            },
            server: None,
        })
        .map_err(|source| ServiceCommandError::RunServiceStatus {
            source: std::io::Error::other(source),
        })
    }
    async fn install(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn update(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn restart_daemon(&self) -> Result<(), ServiceCommandError> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        if self.fail_restart.load(Ordering::SeqCst) {
            return Err(ServiceCommandError::RunElevated {
                command: ServiceCommand::Restart,
                source: std::io::Error::other("native restart failed"),
            });
        }
        Ok(())
    }
}

async fn spawn(process: Arc<RecordingService>) -> ServiceClient {
    ServiceClient::spawn(
        process,
        Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
            panic!("not-installed daemon may not construct a Service core endpoint");
        })),
        3,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn native_restart_is_one_ref_actor_turn_not_stop_then_start() {
    let process = Arc::new(RecordingService::default());
    let port = ServiceClientCommandPort::new(spawn(process.clone()).await);
    port.command(ServiceCommand::Restart).await.unwrap();

    assert_eq!(process.restarts.load(Ordering::SeqCst), 1);
    assert_eq!(process.starts.load(Ordering::SeqCst), 0);
    assert_eq!(process.stops.load(Ordering::SeqCst), 0);
    assert!(!port.has_unsettled_outcome());
    assert!(port.command_settled().await);
}

#[tokio::test]
async fn ref_actor_probe_failure_does_not_synthesize_stopped() {
    let process = Arc::new(RecordingService::default());
    process.fail_probe.store(true, Ordering::SeqCst);
    let port = ServiceClientCommandPort::new(spawn(process).await);

    assert!(port.probe().await.is_err());
    assert!(!port.daemon_not_running().await);
}

#[tokio::test]
async fn exhausted_projection_does_not_turn_cached_stopped_status_into_probe_proof() {
    let process = Arc::new(RecordingService::default());
    process.stopped.store(true, Ordering::SeqCst);
    let actor = ServiceClient::spawn(
        process,
        Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
            panic!("an exhausted service must not create a core endpoint")
        })),
        0,
    )
    .await
    .unwrap();
    assert!(actor.recover_endpoint().await.is_err());
    assert_eq!(actor.status().phase, ServicePhase::Exhausted);

    let port = ServiceClientCommandPort::new(actor);
    assert!(
        port.probe().await.is_err(),
        "cached coarse stopped is not a fresh proof"
    );
    assert!(
        port.daemon_not_running().await,
        "a separate fresh OS probe can still prove the daemon is stopped"
    );
}

#[tokio::test]
async fn failed_ref_command_latches_until_actor_confirms_settlement() {
    let process = Arc::new(RecordingService::default());
    process.fail_restart.store(true, Ordering::SeqCst);
    let port = ServiceClientCommandPort::new(spawn(process).await);

    assert!(port.command(ServiceCommand::Restart).await.is_err());
    assert!(port.has_unsettled_outcome());
    assert!(port.has_outstanding());
    assert!(!port.try_reap_finished().await);
    assert!(port.command_settled().await);
    assert!(!port.has_unsettled_outcome());
}

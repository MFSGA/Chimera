//! Ref-style partial startup: exercise actual actor/mailbox owners without
//! starting the desktop's existing LegacyCoreBridge in parallel.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use chimera_contracts::service::{ServiceCommandError, ServiceProcessControl};
use chimera_core::actor_v2::{
    endpoint::{
        ControlEndpoint, CoreStatusSnapshot, CoreSubmission, EndpointHandle, ExecutionHost,
    },
    service_endpoint_factory::IpcServiceEndpointFactory,
};
use chimera_core_manager::{CoreError, OperationId};
use chimera_ipc::{
    api::{core::v2::OperationInfo, status::CoreStateDetail},
    types::{ServiceStatus, StatusInfo},
};

struct StoppedLocal;

#[async_trait::async_trait]
impl ControlEndpoint for StoppedLocal {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    async fn submit(&self, _: CoreSubmission) -> Result<OperationInfo, CoreError> {
        panic!("an already stopped core must not receive a mutating command")
    }

    async fn wait_operation(&self, _: OperationId, _: Duration) -> Option<OperationInfo> {
        panic!("an already stopped core cannot have an outstanding stop")
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            controller: None,
            state: Some(CoreStateDetail::Stopped { reason: None }),
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: Some(true),
            applied_kind: None,
        })
    }
}

#[derive(Default)]
struct OfflineService {
    probes: AtomicUsize,
}

#[async_trait::async_trait]
impl ServiceProcessControl for OfflineService {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::to_string(&StatusInfo {
            name: "chimera-service".into(),
            version: "1.10.1".into(),
            status: ServiceStatus::NotInstalled,
            server: None,
        })
        .unwrap())
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        panic!("actor startup must not install an absent daemon")
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        panic!("actor startup must not update an absent daemon")
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        panic!("actor startup must not uninstall an absent daemon")
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        panic!("actor startup must not start an absent daemon")
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        panic!("actor startup must not stop an absent daemon")
    }

    async fn restart_daemon(&self) -> Result<(), ServiceCommandError> {
        panic!("actor startup must not restart an absent daemon")
    }
}

#[tokio::test]
async fn actor_owners_follow_ref_spawn_order_and_drain_on_failed_setup() {
    let mut owners = super::StartupOwners::default();
    let offline = Arc::new(OfflineService::default());
    let factory = Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
        panic!("a not-installed service must never supply a core endpoint")
    }));
    let (core, service) = super::start_runtime_owners(
        &mut owners,
        Arc::new(StoppedLocal),
        offline.clone(),
        factory.clone(),
    )
    .await
    .expect("spawn both ref actor owners");

    assert!(
        owners.core.is_some(),
        "partial Core ownership must be retained"
    );
    assert!(
        owners.legacy_core.is_none(),
        "must not duplicate the core owner"
    );
    assert_eq!(core.status().host, ExecutionHost::Local);
    assert_eq!(service.status().status, ServiceStatus::NotInstalled);
    assert!(offline.probes.load(Ordering::SeqCst) >= 1);

    // A second startup would fork two routing authorities. Reject before
    // starting any second Service process adapter or endpoint.
    let duplicate =
        super::start_runtime_owners(&mut owners, Arc::new(StoppedLocal), offline, factory).await;
    assert!(
        duplicate.is_err_and(|error| error.to_string().contains("another Core owner")),
        "duplicate actor composition must be refused"
    );

    owners.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), owners.wait_shutdown())
        .await
        .expect("partial owners must drain promptly");
    assert!(
        core.connected_endpoint().await.is_err(),
        "the Core Actor must be drained"
    );
    assert!(
        service.probe().await.is_err(),
        "the Service Actor must be drained"
    );
}

#[tokio::test]
async fn routed_bootstrap_refuses_a_legacy_core_owner_without_probing_service() {
    use chimera_core_manager::{ControlOptions, CoreControl, CoreManager, ManagerOptions};

    let directory = tempfile::tempdir().unwrap();
    let runtime_dir = camino::Utf8PathBuf::from_path_buf(directory.path().join("runtime"))
        .expect("runtime dir should be UTF-8");
    let manager = CoreManager::builder(ManagerOptions {
        runtime_dir: Some(runtime_dir.clone()),
        log_sink_enabled: false,
        ..ManagerOptions::default()
    })
    .build()
    .await
    .unwrap();
    let control = CoreControl::spawn(
        manager,
        ControlOptions::new(runtime_dir.join("staging"), runtime_dir.join("working")),
    );
    let mut owners = super::StartupOwners {
        legacy_core: Some(control.clone()),
        ..super::StartupOwners::default()
    };
    let offline = Arc::new(OfflineService::default());
    let result = super::start_runtime_owners(
        &mut owners,
        Arc::new(StoppedLocal),
        offline.clone(),
        Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
            panic!("a duplicate owner must never create a Service endpoint")
        })),
    )
    .await;
    assert!(
        result
            .err()
            .is_some_and(|error| error.to_string().contains("another Core owner")),
        "the old and ref Core owners must be mutually exclusive"
    );
    assert_eq!(offline.probes.load(Ordering::SeqCst), 0);
    assert!(owners.core.is_none());

    owners.request_shutdown();
    owners.wait_shutdown().await;
    assert_eq!(
        control.until_closed().await,
        chimera_core_manager::ExecutorExit::Clean
    );
}

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

#[test]
fn synchronous_tauri_setup_registers_service_drain_on_the_runtime() {
    // E2E regression: the real Tauri setup callback runs on the main thread.
    // Calling ServiceClient::drain_on_shutdown here without entering Tokio
    // previously panicked and exited the desktop app with code 101.
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "this test must start outside a Tokio reactor"
    );

    let offline = Arc::new(OfflineService::default());
    let factory = Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
        panic!("an uninstalled daemon must not create a Core endpoint")
    }));
    let service = tauri::async_runtime::block_on(async {
        chimera_core::actor_v2::service_actor::ServiceClient::spawn(offline, factory, 3)
            .await
            .expect("service actor must start on Tauri's runtime")
    });
    let tasks = tokio_util::task::TaskTracker::new();
    let shutdown = tokio_util::sync::CancellationToken::new();

    // This is exactly how the synchronous production setup registers the
    // shutdown watcher; registration itself must never require a reactor on
    // the calling thread.
    super::register_service_drain_on_tauri_runtime(&service, &tasks, shutdown.clone());
    shutdown.cancel();
    tasks.close();
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), tasks.wait())
            .await
            .expect("registered ServiceActor shutdown watcher must drain");
        assert!(
            service.probe().await.is_err(),
            "ServiceActor must be closed"
        );
    });
}

#[tokio::test]
async fn lifecycle_service_probe_and_settlement_use_ref_actor_not_facade() {
    use crate::client::core_lifecycle::{LegacyServiceBridge, ports::ServiceLifecyclePort};

    let daemon = Arc::new(OfflineService::default());
    let service = chimera_core::actor_v2::service_actor::ServiceClient::spawn(
        daemon.clone(),
        Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
            panic!("an offline daemon must never create a Core endpoint")
        })),
        3,
    )
    .await
    .expect("spawn production ServiceActor");
    // A test-only Facade has no production OS process executor or Service
    // ownership. Any successful probe here must have come from ServiceActor.
    let adapter = LegacyServiceBridge::new(Arc::new(
        crate::core::actor_v2::facade::CoreFacade::new_local(),
    ))
    .with_service_actor(service.clone());

    let mut status_updates = ServiceLifecyclePort::actor_status_updates(&adapter)
        .expect("production Service bridge must subscribe to ref ServiceActor");
    let observed = ServiceLifecyclePort::probe(&adapter)
        .await
        .expect("ServiceActor probe must remain authoritative");
    assert_eq!(observed.name, "chimera-service");
    assert_eq!(observed.version, "1.10.1");
    assert_eq!(observed.status, ServiceStatus::NotInstalled);
    assert!(daemon.probes.load(Ordering::SeqCst) > 0);
    let actor_status = ServiceLifecyclePort::actor_status(&adapter)
        .expect("production bridge must read ref ServiceActor generation");
    assert_eq!(
        actor_status.phase,
        chimera_core::actor_v2::service_policy::ServicePhase::NotInstalled
    );
    assert_eq!(
        actor_status.ready_generation,
        service.status().ready_generation
    );
    // A direct ref Actor probe must reach the same receiver used by the
    // production startup-recovery observer. Do not create a second watch.
    tokio::time::timeout(Duration::from_secs(5), status_updates.changed())
        .await
        .expect("the ref ServiceActor watch must notify subscribers")
        .expect("the ServiceActor status channel must remain open");
    assert_eq!(
        status_updates.borrow_and_update().ready_generation,
        actor_status.ready_generation
    );
    assert!(
        ServiceLifecyclePort::command_settled(&adapter).await,
        "the ref Actor should confirm no outstanding OS command"
    );

    let tasks = tokio_util::task::TaskTracker::new();
    let shutdown = tokio_util::sync::CancellationToken::new();
    service.drain_on_shutdown(&tasks, shutdown.clone());
    shutdown.cancel();
    tasks.close();
    tokio::time::timeout(Duration::from_secs(5), tasks.wait())
        .await
        .expect("ServiceActor must drain on shutdown");
    assert!(
        ServiceLifecyclePort::probe(&adapter).await.is_err(),
        "a drained actor must not fall back to Facade probe"
    );
    assert!(
        !ServiceLifecyclePort::command_settled(&adapter).await,
        "an unreachable actor cannot prove the last OS helper settled"
    );
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

#[tokio::test]
async fn partial_production_core_drains_router_then_local_executor() {
    use chimera_core_manager::{ControlOptions, CoreControl, CoreManager, ManagerOptions};

    let directory = tempfile::tempdir().unwrap();
    let runtime_dir = camino::Utf8PathBuf::from_path_buf(directory.path().join("runtime"))
        .expect("runtime directory is UTF-8");
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
    let router = chimera_core::actor_v2::CoreClient::spawn(Arc::new(
        crate::core::actor_v2::control_endpoint::LocalEndpoint::new(control.clone()),
    ))
    .await
    .unwrap();
    let owners = super::StartupOwners {
        core: Some(router.clone()),
        core_executor: Some(control.clone()),
        ..super::StartupOwners::default()
    };

    owners.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), owners.wait_shutdown())
        .await
        .expect("both ref Core Actor and local executor must drain");

    assert!(
        router.connected_endpoint().await.is_err(),
        "the Core Actor mailbox must close after partial startup cleanup"
    );
    assert_eq!(
        control.until_closed().await,
        chimera_core_manager::ExecutorExit::Clean,
        "CoreControl executor must not leak after router termination"
    );
}

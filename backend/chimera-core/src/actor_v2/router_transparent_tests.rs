//! Chimera-only transparent-capture Effects on ref's single-owner Core router.
//! These fakes cannot touch host networking; every OS-facing mutation is counted.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use chimera_core_manager::{CoreError, CoreErrorKind, OperationId};
use chimera_ipc::api::{
    core::v2::{OperationInfo, OperationOutputInfo, OperationPhase},
    network::transparent_proxy::{
        NetworkTransparentProxyMode, NetworkTransparentProxyRequest, NetworkTransparentProxyStatus,
    },
    status::{CoreStateDetail, RevisionIdInfo},
};

use super::{ControlEndpoint, CoreClient, CoreStatusSnapshot, CoreSubmission, ExecutionHost};

struct FakeNetworkEndpoint {
    host: ExecutionHost,
    snapshot: Mutex<CoreStatusSnapshot>,
    requests: Mutex<Vec<NetworkTransparentProxyRequest>>,
    reads: AtomicUsize,
    fail_mutation: AtomicBool,
}

impl FakeNetworkEndpoint {
    fn new(host: ExecutionHost, with_revision: bool) -> Arc<Self> {
        Arc::new(Self {
            host,
            snapshot: Mutex::new(CoreStatusSnapshot {
                instance_id: Some("network-core".into()),
                controller: None,
                state: Some(CoreStateDetail::Running { epoch: 9, pid: 123 }),
                state_changed_at: 1,
                revision: with_revision.then_some(RevisionIdInfo {
                    epoch: 9,
                    generation: 7,
                    effective_hash: "running-revision".into(),
                }),
                source_hash: Some("source".into()),
                healthy: Some(true),
                applied_kind: None,
            }),
            requests: Mutex::new(Vec::new()),
            reads: AtomicUsize::new(0),
            fail_mutation: AtomicBool::new(false),
        })
    }

    fn calls(&self) -> Vec<NetworkTransparentProxyRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for FakeNetworkEndpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        Ok(OperationInfo {
            id: submission.envelope.operation_id.to_string(),
            phase: OperationPhase::Queued,
            output: None,
            error: None,
        })
    }

    async fn wait_operation(&self, id: OperationId, _: Duration) -> Option<OperationInfo> {
        Some(OperationInfo {
            id: id.to_string(),
            phase: OperationPhase::Succeeded,
            output: Some(OperationOutputInfo::Stopped),
            error: None,
        })
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.snapshot.lock().unwrap().clone())
    }

    async fn reconcile_transparent_proxy(
        &self,
        request: &NetworkTransparentProxyRequest,
    ) -> Result<NetworkTransparentProxyStatus, CoreError> {
        self.requests.lock().unwrap().push(request.clone());
        if self.fail_mutation.load(Ordering::SeqCst) {
            return Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "transport lost after OS request was accepted",
                true,
            ));
        }
        Ok(NetworkTransparentProxyStatus {
            supported: true,
            active: request.mode != NetworkTransparentProxyMode::Disabled,
            mode: Some(request.mode),
            revision: Some(request.expected_revision.clone()),
            error: None,
        })
    }

    async fn transparent_proxy_status(&self) -> Result<NetworkTransparentProxyStatus, CoreError> {
        Ok(NetworkTransparentProxyStatus {
            supported: true,
            active: true,
            mode: Some(NetworkTransparentProxyMode::Redir),
            revision: self.snapshot.lock().unwrap().revision.clone(),
            error: None,
        })
    }
}

fn network_request(mode: NetworkTransparentProxyMode) -> NetworkTransparentProxyRequest {
    NetworkTransparentProxyRequest {
        mode,
        port: 7890,
        local: true,
        interfaces: vec!["eth0".into()],
        ipv6: false,
        // The actor must never trust this placeholder from the Effects plan.
        expected_revision: RevisionIdInfo {
            epoch: 0,
            generation: 0,
            effective_hash: String::new(),
        },
    }
}

#[tokio::test]
async fn local_host_rejects_enabled_capture_and_disabled_is_a_noop() {
    let local = FakeNetworkEndpoint::new(ExecutionHost::Local, true);
    let core = CoreClient::spawn(local.clone()).await.unwrap();

    let result = core
        .reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Redir))
        .await;
    assert!(result.is_err());
    assert!(!result.unwrap_err().retryable);
    let disabled = core
        .reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Disabled))
        .await
        .unwrap();
    assert!(!disabled.supported);
    assert!(!disabled.active);
    let observed = core.transparent_proxy_status().await.unwrap();
    assert!(!observed.supported);
    assert!(local.calls().is_empty());
    core.finish_shutdown().await;
}

#[tokio::test]
async fn service_host_uses_current_runtime_revision_not_the_effect_placeholder() {
    let service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    let core = CoreClient::spawn(service.clone()).await.unwrap();

    let applied = core
        .reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Tproxy))
        .await
        .unwrap();
    assert!(applied.active);
    assert_eq!(applied.mode, Some(NetworkTransparentProxyMode::Tproxy));
    let calls = service.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].expected_revision.epoch, 9);
    assert_eq!(calls[0].expected_revision.generation, 7);
    assert_eq!(
        calls[0].expected_revision.effective_hash,
        "running-revision"
    );
    assert!(service.reads.load(Ordering::SeqCst) >= 1);
    assert!(core.transparent_proxy_status().await.unwrap().supported);
    core.finish_shutdown().await;
}

#[tokio::test]
async fn service_without_confirmed_revision_cannot_enable_capture() {
    let service = FakeNetworkEndpoint::new(ExecutionHost::Service, false);
    let core = CoreClient::spawn(service.clone()).await.unwrap();
    assert!(
        core.reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Redir,))
            .await
            .is_err()
    );
    assert!(service.calls().is_empty());
    // Disabling an old capture is still possible without an applied core.
    assert!(
        !core
            .reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Disabled,))
            .await
            .unwrap()
            .active
    );
    core.finish_shutdown().await;
}

#[tokio::test]
async fn host_handoff_fences_old_network_effect_and_routes_next_to_new_service() {
    let first = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    let second = FakeNetworkEndpoint::new(ExecutionHost::Local, true);
    let core = CoreClient::spawn(first.clone()).await.unwrap();
    core.reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Redir))
        .await
        .unwrap();
    core.change_host_from(second, false).await.unwrap();
    assert!(
        core.reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Tproxy,))
            .await
            .is_err()
    );
    assert_eq!(first.calls().len(), 1);
    let new_service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    core.change_host_from(new_service.clone(), false)
        .await
        .unwrap();
    core.reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Tproxy))
        .await
        .unwrap();
    assert_eq!(first.calls().len(), 1);
    assert_eq!(new_service.calls().len(), 1);
    core.finish_shutdown().await;
}

#[tokio::test]
async fn stale_generation_request_does_not_modify_the_new_service() {
    use ractor::rpc::CallResult;

    let old_service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    let local = FakeNetworkEndpoint::new(ExecutionHost::Local, true);
    let new_service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    let core = CoreClient::spawn(old_service).await.unwrap();

    core.change_host_from(local, false).await.unwrap();
    core.change_host_from(new_service.clone(), false)
        .await
        .unwrap();
    assert_eq!(core.status().generation, 2);

    let reply = core
        .actor
        .call(
            |reply| super::CoreActorMessage::ReconcileTransparentProxy {
                request: network_request(NetworkTransparentProxyMode::Tproxy),
                expected_owner: (ExecutionHost::Service, 0),
                reply,
            },
            Some(Duration::from_secs(3)),
        )
        .await
        .unwrap();
    let CallResult::Success(Err(error)) = reply else {
        panic!("the stale owner must be rejected by the CoreActor mailbox")
    };
    assert_eq!(error.kind, Some(CoreErrorKind::RevisionConflict));
    assert!(new_service.calls().is_empty());
    core.finish_shutdown().await;
}

#[tokio::test]
async fn uncertain_network_write_is_isolated_to_its_original_host_generation() {
    let first_service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    first_service.fail_mutation.store(true, Ordering::SeqCst);
    let local = FakeNetworkEndpoint::new(ExecutionHost::Local, true);
    let new_service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    let core = CoreClient::spawn(first_service).await.unwrap();

    assert!(
        core.reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Tproxy,))
            .await
            .is_err()
    );
    core.change_host_from(local, false).await.unwrap();
    core.change_host_from(new_service.clone(), false)
        .await
        .unwrap();
    assert!(
        core.reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Tproxy,))
            .await
            .unwrap()
            .active
    );
    assert_eq!(new_service.calls().len(), 1);
    core.finish_shutdown().await;
}

#[tokio::test]
async fn uncertain_network_mutation_blocks_second_write_in_same_generation() {
    let service = FakeNetworkEndpoint::new(ExecutionHost::Service, true);
    service.fail_mutation.store(true, Ordering::SeqCst);
    let core = CoreClient::spawn(service.clone()).await.unwrap();
    let error = core
        .reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Redir))
        .await
        .unwrap_err();
    assert!(error.retryable);
    let second = core
        .reconcile_transparent_proxy(network_request(NetworkTransparentProxyMode::Redir))
        .await
        .unwrap_err();
    assert!(
        !second.retryable,
        "an uncertain effect must not retry blindly"
    );
    assert_eq!(service.calls().len(), 1);
    // Diagnosis stays possible even when another side-effect is fenced off.
    assert!(core.transparent_proxy_status().await.unwrap().supported);
    core.finish_shutdown().await;
}

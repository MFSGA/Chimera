use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use chimera_core_manager::{
    ConfigInput, CoreCommand, CoreCommandEnvelope, CoreError, CoreKind, CoreSpec, InstanceOptions,
    OperationId, ReconcileRequest,
};
use chimera_ipc::api::{
    core::v2::{OperationInfo, OperationOutputInfo, OperationPhase},
    status::CoreStateDetail,
};

use super::*;
use crate::actor_v2::router::CoreClient;

struct FakeLocal {
    host: ExecutionHost,
    submissions: AtomicUsize,
    waits: AtomicUsize,
}

impl FakeLocal {
    fn new() -> Arc<Self> {
        Self::at(ExecutionHost::Local)
    }

    fn at(host: ExecutionHost) -> Arc<Self> {
        Arc::new(Self {
            host,
            submissions: AtomicUsize::new(0),
            waits: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for FakeLocal {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.submissions.fetch_add(1, Ordering::SeqCst);
        Ok(OperationInfo {
            id: submission.envelope.operation_id.to_string(),
            phase: OperationPhase::Running,
            output: None,
            error: None,
        })
    }

    async fn wait_operation(&self, id: OperationId, _: Duration) -> Option<OperationInfo> {
        self.waits.fetch_add(1, Ordering::SeqCst);
        Some(OperationInfo {
            id: id.to_string(),
            phase: OperationPhase::Succeeded,
            output: Some(OperationOutputInfo::Stopped),
            error: None,
        })
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

fn request() -> CoreSubmission {
    CoreSubmission {
        expected_owner: None,
        envelope: CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                core: CoreSpec {
                    kind: CoreKind::Mihomo,
                    binary_path: camino::Utf8PathBuf::from("mihomo"),
                    version: None,
                    features: vec![],
                },
                config: ConfigInput::inline(b"proxies: []".to_vec()),
                options: InstanceOptions::default(),
                expected_applied: None,
            })),
        },
        core_type: None,
    }
}

#[tokio::test]
async fn submits_through_ref_mailbox_and_pins_the_admitting_endpoint_for_wait() {
    let local = FakeLocal::new();
    let router = CoreClient::spawn(local.clone()).await.unwrap();
    let port = RoutedEndpoint::new(router.clone(), ExecutionHost::Local);
    let submission = request();
    let id = submission.envelope.operation_id;
    let admitted = port.submit(submission).await.unwrap();
    assert_eq!(admitted.id, id.to_string());
    assert_eq!(admitted.phase, OperationPhase::Running);
    assert_eq!(local.submissions.load(Ordering::SeqCst), 1);
    assert_eq!(
        port.wait_operation(id, Duration::from_millis(10))
            .await
            .unwrap()
            .phase,
        OperationPhase::Succeeded
    );
    assert_eq!(local.waits.load(Ordering::SeqCst), 1);

    // A terminal ticket cannot be accidentally reused for a later host.
    assert!(
        port.wait_operation(id, Duration::from_millis(10))
            .await
            .is_none()
    );
    router.finish_shutdown().await;
}

#[tokio::test]
async fn waiting_after_a_host_handoff_uses_the_original_admission_ticket() {
    let local = FakeLocal::new();
    let service = FakeLocal::at(ExecutionHost::Service);
    let router = CoreClient::spawn(local.clone()).await.unwrap();
    let local_port = RoutedEndpoint::new(router.clone(), ExecutionHost::Local);
    let submission = request();
    let id = submission.envelope.operation_id;

    let admitted = local_port.submit(submission).await.unwrap();
    assert_eq!(admitted.phase, OperationPhase::Running);
    router
        .change_host_from(service.clone(), false)
        .await
        .unwrap();
    assert_eq!(router.status().host, ExecutionHost::Service);

    // The handoff itself may wait for a distinct local Stop operation.
    // Count only the wait caused by the original submission ticket.
    let local_waits_before_ticket = local.waits.load(Ordering::SeqCst);
    let service_waits_before_ticket = service.waits.load(Ordering::SeqCst);
    // Waiting on the old ticket cannot accidentally query the newly active
    // Service endpoint. That endpoint has never seen this operation id.
    let finished = local_port
        .wait_operation(id, Duration::from_millis(50))
        .await
        .unwrap();
    assert_eq!(finished.id, id.to_string());
    assert_eq!(finished.phase, OperationPhase::Succeeded);
    assert_eq!(
        local.waits.load(Ordering::SeqCst),
        local_waits_before_ticket + 1
    );
    assert_eq!(
        service.waits.load(Ordering::SeqCst),
        service_waits_before_ticket
    );

    // The old adapter is now fenced; the new host receives new commands.
    assert!(local_port.submit(request()).await.is_err());
    let service_port = RoutedEndpoint::new(router.clone(), ExecutionHost::Service);
    assert!(service_port.submit(request()).await.is_ok());
    assert_eq!(service.submissions.load(Ordering::SeqCst), 1);
    router.finish_shutdown().await;
}

#[tokio::test]
async fn a_nonselected_host_cannot_bypass_core_actor_admission() {
    let local = FakeLocal::new();
    let router = CoreClient::spawn(local.clone()).await.unwrap();
    let service_port = RoutedEndpoint::new(router.clone(), ExecutionHost::Service);
    assert!(service_port.submit(request()).await.is_err());
    assert_eq!(local.submissions.load(Ordering::SeqCst), 0);
    router.finish_shutdown().await;
}

#[tokio::test]
async fn rejects_a_stale_generation_token_before_submitting() {
    let local = FakeLocal::new();
    let router = CoreClient::spawn(local.clone()).await.unwrap();
    let port = RoutedEndpoint::new(router.clone(), ExecutionHost::Local);
    let mut submission = request();
    submission.expected_owner = Some((ExecutionHost::Local, u64::MAX));
    assert!(port.submit(submission).await.is_err());
    assert_eq!(local.submissions.load(Ordering::SeqCst), 0);
    router.finish_shutdown().await;
}

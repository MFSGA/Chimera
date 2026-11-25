//! Exercise the actual audited execution seam without a transport or application owner.
use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use chimera_rpc_protocol::RpcValue;
use chimera_runtime_values::runtime::{CommitReceipt, RuntimeCommitStatus};
use serde_json::json;

use super::*;
use crate::audit::{ObservationEnd, RpcAuditRecord, RpcAuditSink};

const SECRET: &str = "private-audit-sentinel";

#[derive(Default)]
struct Capture(Mutex<Vec<RpcAuditRecord>>);

impl RpcAuditSink for Capture {
    fn record(&self, record: &RpcAuditRecord) {
        self.0.lock().unwrap().push(record.clone());
    }
}

fn error() -> RpcError {
    RpcError {
        kind: "application_error",
        message: SECRET.into(),
        code: Some("original_code".into()),
        retryable: Some(true),
        operation_id: Some("operation".into()),
        domain_error: Some(Box::new(RpcValue(json!({"detail": SECRET})))),
    }
}

#[tokio::test]
async fn rejection_and_handler_failure_preserve_errors_without_auditing_payloads() {
    let capture = Arc::new(Capture::default());
    let audit = RpcAudit::new(capture.clone());
    let expected = serde_json::to_value(error()).unwrap();
    let rejected = dispatch_audited::<(), std::future::Ready<RpcCompletion<()>>>(
        &audit,
        RpcTransport::Http,
        RpcOwner::new(SECRET.into()),
        None,
        || "known_command",
        None,
        || Err(error()),
    )
    .await
    .unwrap_err();
    assert_eq!(serde_json::to_value(rejected).unwrap(), expected);
    let failed = dispatch_audited(
        &audit,
        RpcTransport::Http,
        RpcOwner::new(SECRET.into()),
        None,
        || "known_command",
        None,
        || {
            Ok(std::future::ready(RpcCompletion::<()>::unreported(Err(
                error(),
            ))))
        },
    )
    .await
    .unwrap_err();
    assert_eq!(serde_json::to_value(failed).unwrap(), expected);
    let records = capture.0.lock().unwrap();
    assert_eq!(records.len(), 3);
    assert!(matches!(records[0].phase, AuditPhase::Rejected { .. }));
    assert_eq!(records[1].phase, AuditPhase::Admitted);
    assert!(matches!(
        records[2].phase,
        AuditPhase::Finished {
            outcome: RpcAuditOutcome::Failure { .. },
            ..
        }
    ));
    assert_ne!(
        records[0].header.invocation_id,
        records[1].header.invocation_id
    );
    assert_eq!(
        records[1].header.invocation_id,
        records[2].header.invocation_id
    );
    assert_eq!(records[0].header.owner, records[1].header.owner);
    assert!(!format!("{records:?}").contains(SECRET));
}

#[tokio::test]
async fn dropping_audited_waiter_does_not_cancel_owner_started_work() {
    let capture = Arc::new(Capture::default());
    let audit = RpcAudit::new(capture.clone());
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let (owner_task, task) = tokio::sync::oneshot::channel();
    let (finished_work, work_result) = tokio::sync::oneshot::channel::<()>();
    let mut call = Box::pin(dispatch_audited(
        &audit,
        RpcTransport::Http,
        RpcOwner::new(SECRET.into()),
        None,
        || "owner_work",
        None,
        || {
            Ok(unreported(async {
                owner_task
                    .send(tokio::spawn(async move {
                        wait.await.unwrap();
                        let _ = finished_work.send(());
                        7
                    }))
                    .unwrap();
                work_result.await.unwrap();
                Ok(())
            }))
        },
    ));
    assert!(matches!(
        call.as_mut().poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    let owner = task.await.unwrap();
    drop(call);
    {
        let records = capture.0.lock().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records[1].phase,
            AuditPhase::Finished {
                outcome: RpcAuditOutcome::NotObserved,
                completion: CompletionSummary::NotReported,
            }
        );
        assert_eq!(
            records[2].phase,
            AuditPhase::ObservationEnded {
                reason: ObservationEnd::FutureDropped
            }
        );
    }
    release.send(()).unwrap();
    assert_eq!(
        owner.await.unwrap(),
        7,
        "owner commits after the waiter leaves"
    );
}

#[tokio::test]
async fn response_encoding_failure_keeps_committed_receipts() {
    let capture = Arc::new(Capture::default());
    let audit = RpcAudit::new(capture.clone());
    let completion = CompletionSummary::Mutation {
        commits: vec![CommitReceipt {
            operation_id: Some("audit-operation".into()),
            domain: "profiles".into(),
            source_version: 7,
            runtime: RuntimeCommitStatus::Deferred,
        }],
        notifications_pending: true,
        degradation_count: 1,
    };
    let result = dispatch_audited(
        &audit,
        RpcTransport::Local,
        RpcOwner::new(SECRET.into()),
        None,
        || "known_command",
        None,
        || {
            Ok(std::future::ready(RpcCompletion::<()> {
                result: Err(error()),
                completion: completion.clone(),
            }))
        },
    )
    .await;
    assert!(result.is_err());
    let records = capture.0.lock().unwrap();
    assert!(
        matches!(&records[1].phase, AuditPhase::Finished { outcome: RpcAuditOutcome::Failure { .. }, completion: recorded } if recorded == &completion)
    );
    assert!(!format!("{records:?}").contains(SECRET));
}

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

#[test]
fn linked_declarations_match_the_resource_table_and_unlinked_commands_stay_known() {
    let commands = register_commands(RESOURCES, inventory::iter::<CommandEntry>).unwrap();
    let linked: Vec<_> = commands
        .values()
        .filter(|c| c.handler.is_some())
        .map(|c| c.metadata.name)
        .collect();
    assert_eq!(linked.len(), 57);
    for name in [
        "activate_profile",
        "clash_api_delete_connections",
        "clash_api_flush_dns_cache",
        "clash_api_flush_fake_ip_cache",
        "clash_api_get_configs",
        "clash_api_get_group_delay",
        "clash_api_get_providers_proxies",
        "clash_api_get_providers_rules",
        "clash_api_get_rules",
        "clash_api_get_settings",
        "clash_api_get_version",
        "clash_api_healthcheck_proxy_provider",
        "clash_api_query_dns",
        "clash_api_update_providers_rules",
        "clear_proxy_fixed",
        "create_profile",
        "create_profile_folder",
        "create_proxy_group_folder",
        "delete_profile",
        "enhance_profiles",
        "get_clash_config",
        "get_clash_info",
        "get_config_features",
        "get_core_status",
        "get_core_status_summary",
        "get_hotkey_functions",
        "get_hotkeys",
        "get_profile_sync_logs",
        "get_profile_sync_runs",
        "get_profile_sync_runs_summary",
        "get_profile_sync_status",
        "get_profile_sync_status_summary",
        "get_profiles",
        "get_profiles_listing",
        "get_proxies",
        "get_proxy_group_icons",
        "get_service_status_summary",
        "import_profile",
        "mutate_proxies",
        "patch_profile_metadata",
        "patch_remote_profile_options",
        "probe_direct_egress",
        "read_profile_file",
        "reorder_profile",
        "reorder_profiles_by_list",
        "replace_profile_definition",
        "retry_configuration_runtime",
        "rpc_describe",
        "save_profile_file",
        "select_proxy",
        "set_global_transforms",
        "set_profile_layout",
        "set_profile_valid_fields",
        "set_proxy_group_layout",
        "status_service",
        "update_profile",
        "update_proxy_provider",
    ] {
        assert!(linked.contains(&name));
        assert!(commands[name].linked(RpcTarget::Desktop));
    }
    for name in [
        "activate_profile",
        "clear_proxy_fixed",
        "create_profile",
        "create_profile_folder",
        "create_proxy_group_folder",
        "delete_profile",
        "import_profile",
        "patch_profile_metadata",
        "patch_remote_profile_options",
        "reorder_profile",
        "reorder_profiles_by_list",
        "replace_profile_definition",
        "save_profile_file",
        "select_proxy",
        "set_global_transforms",
        "set_profile_layout",
        "set_profile_valid_fields",
        "set_proxy_group_layout",
        "update_profile",
    ] {
        let metadata = commands[name].metadata();
        assert_eq!(
            metadata.permission.access,
            chimera_contracts::auth::Access::Write
        );
    }
    assert!(!commands["get_runtime_config"].linked(RpcTarget::Desktop));
    assert_eq!(
        commands["get_runtime_config"]
            .neutral_handler(RpcTarget::Desktop)
            .unwrap_err()
            .kind,
        "unsupported"
    );
    let entries = inventory::iter::<CommandEntry>
        .into_iter()
        .collect::<Vec<_>>();
    let repeated = [entries[0], entries[0]];
    assert!(matches!(
        register_commands(RESOURCES, repeated),
        Err(SetupError::RegisterCommand { .. })
    ));
    assert!(matches!(
        register_commands([], [entries[0]]),
        Err(SetupError::UndeclaredCommand { .. })
    ));
}

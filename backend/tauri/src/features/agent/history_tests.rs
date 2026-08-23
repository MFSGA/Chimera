use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{
    AgentAuditHistoryEntry, AgentAuditOutcome, AgentDiagnosticHistoryEntry, AgentHealthTrend,
    AgentHistoryDocument, AgentHistoryStore, MAX_CORRUPT_HISTORY_FILES, history_proposal_reference,
    history_snapshot, is_lower_hex, normalize_history_document, summarize_history, trim,
};
use crate::features::agent::{
    adapters::fs_history::{
        MAX_HISTORY_FILE_BYTES, corrupt_history_file_name_for_test,
        corrupt_history_sort_key_for_test, prune_corrupt_documents,
        prune_corrupt_documents_with_limit, read_document_from, recover_temporary_document,
        write_document_to, write_private_history_file,
    },
    model::{
        AgentActionKind, AgentCoreState, AgentFindingCode, AgentHealth, AgentProbeCode,
        AgentServiceState,
    },
    ports::AgentHistoryPersistencePort,
};

#[cfg(unix)]
use crate::features::agent::adapters::fs_history::path_entry_exists;

struct MemoryHistoryPersistence {
    document: tokio::sync::Mutex<AgentHistoryDocument>,
    loads: AtomicUsize,
    saves: AtomicUsize,
}

impl MemoryHistoryPersistence {
    fn new(document: AgentHistoryDocument) -> Self {
        Self {
            document: tokio::sync::Mutex::new(document),
            loads: AtomicUsize::new(0),
            saves: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl AgentHistoryPersistencePort for MemoryHistoryPersistence {
    async fn load(&self) -> anyhow::Result<AgentHistoryDocument> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        Ok(self.document.lock().await.clone())
    }

    async fn save(&self, document: &AgentHistoryDocument) -> anyhow::Result<()> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        *self.document.lock().await = document.clone();
        Ok(())
    }
}

#[tokio::test]
async fn history_store_uses_the_injected_persistence_port() {
    let persistence = Arc::new(MemoryHistoryPersistence::new(
        AgentHistoryDocument::default(),
    ));
    let mut store = AgentHistoryStore::new(persistence.clone());

    let snapshot = store.snapshot().await;
    assert!(snapshot.diagnostics.is_empty());
    assert_eq!(persistence.loads.load(Ordering::SeqCst), 1);

    store.clear().await.expect("clear injected history store");
    assert_eq!(persistence.loads.load(Ordering::SeqCst), 1);
    assert_eq!(persistence.saves.load(Ordering::SeqCst), 1);
}

#[test]
fn trim_keeps_newest_entries() {
    let mut entries = VecDeque::from([1, 2, 3, 4]);
    trim(&mut entries, 2);
    assert_eq!(entries, VecDeque::from([3, 4]));
}

#[test]
fn history_identifiers_are_fixed_lower_hex_and_legacy_ids_are_hashed() {
    let reference = history_proposal_reference("legacy-proposal-token-canary");
    assert!(is_lower_hex(&reference, 32));
    assert!(!reference.contains("token-canary"));

    let valid_revision = "a".repeat(64);
    let mut document = AgentHistoryDocument {
        diagnostics: VecDeque::from([
            AgentDiagnosticHistoryEntry {
                schema_version: 1,
                captured_at: 1,
                revision: valid_revision.clone(),
                health: AgentHealth::Healthy,
                core_state: AgentCoreState::Running,
                service_state: AgentServiceState::Running,
                finding_codes: vec![],
                probe_failure_codes: vec![],
            },
            AgentDiagnosticHistoryEntry {
                schema_version: 1,
                captured_at: 2,
                revision: "subscription-url-canary".into(),
                health: AgentHealth::Critical,
                core_state: AgentCoreState::Stopped,
                service_state: AgentServiceState::Unknown,
                finding_codes: vec![],
                probe_failure_codes: vec![],
            },
        ]),
        audits: VecDeque::from([
            AgentAuditHistoryEntry {
                schema_version: 1,
                recorded_at: 1,
                proposal_id: "legacy-proposal-token-canary".into(),
                action: AgentActionKind::RestartCore,
                snapshot_revision: valid_revision,
                outcome: AgentAuditOutcome::Verified,
            },
            AgentAuditHistoryEntry {
                schema_version: 1,
                recorded_at: 2,
                proposal_id: "secret-canary".into(),
                action: AgentActionKind::RestartCore,
                snapshot_revision: "connection-target-canary".into(),
                outcome: AgentAuditOutcome::ActionFailed,
            },
        ]),
    };

    normalize_history_document(&mut document);

    assert_eq!(document.diagnostics.len(), 1);
    assert_eq!(document.audits.len(), 1);
    assert_eq!(document.audits[0].proposal_id, reference);
    let serialized = serde_json::to_string(&document).unwrap();
    for forbidden in [
        "legacy-proposal-token-canary",
        "subscription-url-canary",
        "secret-canary",
        "connection-target-canary",
    ] {
        assert!(!serialized.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn history_normalization_rejects_invalid_timestamps_and_deduplicates_codes() {
    let revision = "a".repeat(64);
    let proposal_id = "b".repeat(32);
    let mut document = AgentHistoryDocument {
        diagnostics: VecDeque::from([
            AgentDiagnosticHistoryEntry {
                schema_version: 1,
                captured_at: 1,
                revision: revision.clone(),
                health: AgentHealth::Warning,
                core_state: AgentCoreState::Running,
                service_state: AgentServiceState::Running,
                finding_codes: vec![
                    AgentFindingCode::RecentCoreErrors,
                    AgentFindingCode::RecentCoreErrors,
                    AgentFindingCode::TunRuntimeMismatch,
                ],
                probe_failure_codes: vec![
                    AgentProbeCode::TelemetryUnavailable,
                    AgentProbeCode::TelemetryUnavailable,
                ],
            },
            AgentDiagnosticHistoryEntry {
                schema_version: 1,
                captured_at: i64::MAX,
                revision: revision.clone(),
                health: AgentHealth::Critical,
                core_state: AgentCoreState::Unknown,
                service_state: AgentServiceState::Unknown,
                finding_codes: vec![],
                probe_failure_codes: vec![],
            },
        ]),
        audits: VecDeque::from([
            AgentAuditHistoryEntry {
                schema_version: 1,
                recorded_at: 1,
                proposal_id: proposal_id.clone(),
                action: AgentActionKind::RestartCore,
                snapshot_revision: revision.clone(),
                outcome: AgentAuditOutcome::Verified,
            },
            AgentAuditHistoryEntry {
                schema_version: 1,
                recorded_at: -1,
                proposal_id,
                action: AgentActionKind::RestartCore,
                snapshot_revision: revision,
                outcome: AgentAuditOutcome::ActionFailed,
            },
        ]),
    };

    normalize_history_document(&mut document);

    assert_eq!(document.diagnostics.len(), 1);
    assert_eq!(
        document.diagnostics[0].finding_codes,
        vec![
            AgentFindingCode::RecentCoreErrors,
            AgentFindingCode::TunRuntimeMismatch,
        ]
    );
    assert_eq!(
        document.diagnostics[0].probe_failure_codes,
        vec![AgentProbeCode::TelemetryUnavailable]
    );
    assert_eq!(document.audits.len(), 1);
}

#[test]
fn history_deserialization_rejects_unknown_fields_at_every_level() {
    let revision = "a".repeat(64);
    let proposal_id = "b".repeat(32);
    let snapshot_revision = "c".repeat(64);
    let diagnostic = format!(
        r#"{{"schema_version":1,"captured_at":1,"revision":"{revision}","health":"healthy","core_state":"running","service_state":"running","finding_codes":[],"probe_failure_codes":[]}}"#
    );
    let audit = format!(
        r#"{{"schema_version":1,"recorded_at":1,"proposal_id":"{proposal_id}","action":"restart_core","snapshot_revision":"{snapshot_revision}","outcome":"verified"}}"#
    );
    let documents = [
        format!(r#"{{"diagnostics":[{diagnostic}],"audits":[{audit}],"token":"canary"}}"#),
        format!(
            r#"{{"diagnostics":[{{"raw_logs":"canary",{}}}],"audits":[{audit}]}}"#,
            &diagnostic[1..diagnostic.len() - 1]
        ),
        format!(
            r#"{{"diagnostics":[{diagnostic}],"audits":[{{"subscription_url":"canary",{}}}]}}"#,
            &audit[1..audit.len() - 1]
        ),
    ];

    for document in documents {
        assert!(serde_json::from_str::<AgentHistoryDocument>(&document).is_err());
    }
}

#[test]
fn history_tracing_contains_no_raw_errors_or_storage_paths() {
    let source = include_str!("history.rs");
    let marker = ["tracing", "::"].concat();
    let mut remaining = source;
    let mut invocation_count = 0;

    while let Some(start) = remaining.find(&marker) {
        remaining = &remaining[start..];
        let end = remaining
            .find(");")
            .expect("tracing invocation must terminate");
        let invocation = &remaining[..end + 2];
        invocation_count += 1;

        for forbidden in ["error", "path", "temporary", "quarantined", "bytes"] {
            assert!(
                !invocation.contains(forbidden),
                "history tracing must not include {forbidden}: {invocation}"
            );
        }
        remaining = &remaining[end + 2..];
    }

    assert!(invocation_count > 0, "expected history tracing invocations");
}

#[test]
fn history_writer_requires_exclusive_create_and_durable_sync() {
    let source = include_str!("adapters/fs_history.rs");
    assert!(source.contains("options.write(true).create_new(true);"));
    assert!(source.contains("file.flush()?;"));
    assert!(source.contains("file.sync_all()"));
    assert!(source.contains("sync_parent_directory(path, blocking_io).await"));
    let forbidden = ["truncate", "(true)"].concat();
    assert!(!source.contains(&forbidden));
}

#[tokio::test]
async fn private_writer_never_truncates_a_preexisting_path() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json.tmp");
    tokio::fs::write(&path, b"preexisting")
        .await
        .expect("seed preexisting temporary");

    assert!(
        write_private_history_file(&path, b"replacement".to_vec())
            .await
            .is_err()
    );
    assert_eq!(
        tokio::fs::read(&path).await.expect("read preexisting path"),
        b"preexisting"
    );
}

#[tokio::test]
async fn oversized_history_is_rejected_before_unbounded_allocation() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    tokio::fs::write(&path, vec![b'x'; MAX_HISTORY_FILE_BYTES as usize + 1])
        .await
        .expect("seed oversized history");

    assert!(read_document_from(&path).await.is_err());
    assert_eq!(
        tokio::fs::metadata(&path)
            .await
            .expect("oversized history remains inspectable")
            .len(),
        MAX_HISTORY_FILE_BYTES + 1
    );
}

#[tokio::test]
async fn oversized_history_is_rejected_before_creating_recovery_files() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    let mut document = AgentHistoryDocument::default();
    document.diagnostics.push_back(AgentDiagnosticHistoryEntry {
        schema_version: 1,
        captured_at: 1,
        revision: "a".repeat(MAX_HISTORY_FILE_BYTES as usize + 1),
        health: AgentHealth::Healthy,
        core_state: AgentCoreState::Running,
        service_state: AgentServiceState::Running,
        finding_codes: Vec::new(),
        probe_failure_codes: Vec::new(),
    });

    assert!(write_document_to(&path, &document).await.is_err());
    assert!(!path.exists());
    assert!(!path.with_extension("json.tmp").exists());
}

#[cfg(windows)]
#[test]
fn windows_history_io_uses_reparse_safe_write_through_operations() {
    let source = include_str!("adapters/fs_history.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production history source");
    assert!(production.contains("FILE_FLAG_OPEN_REPARSE_POINT"));
    assert!(production.contains("FILE_ATTRIBUTE_REPARSE_POINT"));
    assert!(production.contains("MOVEFILE_REPLACE_EXISTING"));
    assert!(production.contains("MOVEFILE_WRITE_THROUGH"));
}

#[cfg(windows)]
#[tokio::test]
async fn windows_primary_symlink_is_rejected_then_replaced_without_touching_target() {
    use std::os::windows::fs::symlink_file;

    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("target.json");
    let path = directory.path().join("agent-history.json");
    let target_bytes = b"target remains unchanged";
    tokio::fs::write(&target, target_bytes)
        .await
        .expect("seed symlink target");
    symlink_file(&target, &path).expect("create history symlink");

    assert!(read_document_from(&path).await.is_err());
    assert_eq!(
        tokio::fs::read(&target).await.expect("read symlink target"),
        target_bytes
    );

    write_document_to(&path, &AgentHistoryDocument::default())
        .await
        .expect("replace history symlink entry");

    assert!(
        !tokio::fs::symlink_metadata(&path)
            .await
            .expect("primary metadata")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        tokio::fs::read(&target)
            .await
            .expect("read untouched target"),
        target_bytes
    );
    let bytes = tokio::fs::read(&path).await.expect("read primary history");
    let document: AgentHistoryDocument =
        serde_json::from_slice(&bytes).expect("parse primary history");
    assert!(document.diagnostics.is_empty());
    assert!(document.audits.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn primary_symlink_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("target.json");
    let path = directory.path().join("agent-history.json");
    let bytes = serde_json::to_vec(&AgentHistoryDocument::default()).expect("serialize history");
    tokio::fs::write(&target, &bytes)
        .await
        .expect("seed symlink target");
    tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
        .await
        .expect("set target permissions");
    symlink(&target, &path).expect("create history symlink");

    assert!(read_document_from(&path).await.is_err());
    assert_eq!(tokio::fs::read(&target).await.expect("read target"), bytes);
    assert_eq!(
        tokio::fs::metadata(&target)
            .await
            .expect("target metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
}

#[cfg(unix)]
#[tokio::test]
async fn temporary_symlink_is_removed_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    let temporary = path.with_extension("json.tmp");
    let target = directory.path().join("target.json");
    tokio::fs::write(&target, b"target remains unchanged")
        .await
        .expect("seed symlink target");
    symlink(&target, &temporary).expect("create temporary symlink");

    write_document_to(&path, &AgentHistoryDocument::default())
        .await
        .expect("write history without following temporary symlink");

    assert_eq!(
        tokio::fs::read(&target).await.expect("read symlink target"),
        b"target remains unchanged"
    );
    assert!(
        tokio::fs::metadata(&path)
            .await
            .expect("primary metadata")
            .is_file()
    );
    assert!(
        !path_entry_exists(&temporary)
            .await
            .expect("temporary state")
    );
}

#[test]
fn audit_outcomes_serialize_to_stable_closed_codes() {
    let cases = [
        (AgentAuditOutcome::Proposed, "proposed"),
        (AgentAuditOutcome::Verified, "verified"),
        (
            AgentAuditOutcome::ActionNotAvailable,
            "action_not_available",
        ),
        (AgentAuditOutcome::ProposalNotFound, "proposal_not_found"),
        (AgentAuditOutcome::ProposalExpired, "proposal_expired"),
        (AgentAuditOutcome::DigestMismatch, "digest_mismatch"),
        (AgentAuditOutcome::StateChanged, "state_changed"),
        (AgentAuditOutcome::RateLimited, "rate_limited"),
        (AgentAuditOutcome::LimitReached, "limit_reached"),
        (
            AgentAuditOutcome::ConfirmationDeclined,
            "confirmation_declined",
        ),
        (AgentAuditOutcome::ActionFailed, "action_failed"),
        (AgentAuditOutcome::PartialApply, "partial_apply"),
        (AgentAuditOutcome::VerificationFailed, "verification_failed"),
        (AgentAuditOutcome::BridgeStartFailed, "bridge_start_failed"),
        (
            AgentAuditOutcome::HistoryClearFailed,
            "history_clear_failed",
        ),
    ];

    for (outcome, code) in cases {
        assert_eq!(outcome.as_str(), code);
        assert_eq!(
            serde_json::to_string(&outcome).unwrap(),
            format!("\"{code}\"")
        );
        let decoded = serde_json::from_str::<AgentAuditOutcome>(&format!("\"{code}\""))
            .expect("known audit outcome must deserialize");
        assert_eq!(decoded, outcome);
    }
    assert!(serde_json::from_str::<AgentAuditOutcome>("\"raw internal error\"").is_err());
}

#[test]
fn empty_document_produces_an_empty_public_snapshot() {
    let snapshot = history_snapshot(AgentHistoryDocument::default());
    assert!(snapshot.diagnostics.is_empty());
    assert!(snapshot.audits.is_empty());
    assert_eq!(snapshot.summary.diagnostic_samples, 0);
    assert_eq!(snapshot.summary.action_attempts, 0);
    assert!(matches!(
        snapshot.summary.health_trend,
        AgentHealthTrend::InsufficientData
    ));
}

#[test]
fn summary_derives_health_trend_issue_frequency_and_action_outcomes() {
    let diagnostics = VecDeque::from([
        AgentDiagnosticHistoryEntry {
            schema_version: 1,
            captured_at: 1,
            revision: "first".into(),
            health: AgentHealth::Healthy,
            core_state: AgentCoreState::Running,
            service_state: AgentServiceState::Stopped,
            finding_codes: vec![
                AgentFindingCode::WeakControllerSecret,
                AgentFindingCode::HostIpv4Only,
            ],
            probe_failure_codes: vec![],
        },
        AgentDiagnosticHistoryEntry {
            schema_version: 1,
            captured_at: 2,
            revision: "second".into(),
            health: AgentHealth::Critical,
            core_state: AgentCoreState::Stopped,
            service_state: AgentServiceState::Stopped,
            finding_codes: vec![
                AgentFindingCode::WeakControllerSecret,
                AgentFindingCode::ActiveProfileMissing,
                AgentFindingCode::HostInternetUnreachable,
            ],
            probe_failure_codes: vec![
                AgentProbeCode::TelemetryUnavailable,
                AgentProbeCode::HostConnectivityUnavailable,
            ],
        },
    ]);
    let audits = VecDeque::from([
        AgentAuditHistoryEntry {
            schema_version: 1,
            recorded_at: 1,
            proposal_id: "proposal".into(),
            action: AgentActionKind::RestartCore,
            snapshot_revision: "first".into(),
            outcome: AgentAuditOutcome::Proposed,
        },
        AgentAuditHistoryEntry {
            schema_version: 1,
            recorded_at: 2,
            proposal_id: "verified".into(),
            action: AgentActionKind::RestartCore,
            snapshot_revision: "first".into(),
            outcome: AgentAuditOutcome::Verified,
        },
        AgentAuditHistoryEntry {
            schema_version: 1,
            recorded_at: 3,
            proposal_id: "partial".into(),
            action: AgentActionKind::RestartCore,
            snapshot_revision: "second".into(),
            outcome: AgentAuditOutcome::PartialApply,
        },
        AgentAuditHistoryEntry {
            schema_version: 1,
            recorded_at: 4,
            proposal_id: "failed".into(),
            action: AgentActionKind::RestartCore,
            snapshot_revision: "second".into(),
            outcome: AgentAuditOutcome::ActionFailed,
        },
    ]);

    let summary = summarize_history(&diagnostics, &audits);

    assert_eq!(summary.diagnostic_samples, 2);
    assert_eq!(summary.unhealthy_samples, 1);
    assert_eq!(summary.latest_health, Some(AgentHealth::Critical));
    assert!(matches!(summary.health_trend, AgentHealthTrend::Worsening));
    assert_eq!(summary.finding_counts[0].count, 2);
    assert_eq!(
        summary.finding_counts[0].code,
        AgentFindingCode::WeakControllerSecret
    );
    assert!(summary.finding_counts.iter().any(|entry| {
        entry.code == AgentFindingCode::HostInternetUnreachable && entry.count == 1
    }));
    assert!(
        summary
            .finding_counts
            .iter()
            .any(|entry| { entry.code == AgentFindingCode::HostIpv4Only && entry.count == 1 })
    );
    assert!(summary.probe_failure_counts.iter().any(|entry| {
        entry.code == AgentProbeCode::HostConnectivityUnavailable && entry.count == 1
    }));
    assert_eq!(summary.action_attempts, 3);
    assert_eq!(summary.verified_actions, 1);
    assert_eq!(summary.attention_actions, 2);
    assert_eq!(summary.partial_actions, 1);
}

#[tokio::test]
async fn corrupt_history_is_quarantined_and_recovers_empty() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    tokio::fs::write(&path, b"not-json")
        .await
        .expect("seed corrupt history");

    assert!(read_document_from(&path).await.is_err());
    assert!(!tokio::fs::try_exists(&path).await.expect("check source"));

    let mut entries = tokio::fs::read_dir(directory.path())
        .await
        .expect("read directory");
    let quarantined = entries
        .next_entry()
        .await
        .expect("read entry")
        .expect("quarantined file");
    let name = quarantined.file_name().to_string_lossy().into_owned();
    assert!(name.starts_with("agent-history.corrupt-"));
    assert!(name.ends_with(".json"));

    let recovered = read_document_from(&path)
        .await
        .expect("missing history recovers empty");
    assert!(recovered.diagnostics.is_empty());
    assert!(recovered.audits.is_empty());
}

#[test]
fn corrupt_history_names_bind_timestamp_and_nonce() {
    let first = corrupt_history_file_name_for_test(1234, [0; 16]);
    let second = corrupt_history_file_name_for_test(1234, [1; 16]);

    assert_eq!(
        first,
        "agent-history.corrupt-1234-00000000000000000000000000000000.json"
    );
    assert_eq!(
        second,
        "agent-history.corrupt-1234-01010101010101010101010101010101.json"
    );
    assert_ne!(first, second);
}

#[test]
fn corrupt_history_sort_keys_accept_only_legacy_or_nonce_bound_names() {
    assert_eq!(
        corrupt_history_sort_key_for_test("agent-history.corrupt-0007.json"),
        Some((7, 0))
    );
    assert_eq!(
        corrupt_history_sort_key_for_test(
            "agent-history.corrupt-1234-01010101010101010101010101010101.json"
        ),
        Some((1234, 0x01010101010101010101010101010101))
    );
    for invalid in [
        "agent-history.corrupt--1.json",
        "agent-history.corrupt-not-a-time.json",
        "agent-history.corrupt-1234-short.json",
        "agent-history.corrupt-1234-ABCDEFABCDEFABCDEFABCDEFABCDEFAB.json",
        "agent-history.corrupt-1234-0000000000000000000000000000000g.json",
        "agent-history.corrupt-1234-00000000000000000000000000000000.extra.json",
    ] {
        assert_eq!(
            corrupt_history_sort_key_for_test(invalid),
            None,
            "{invalid}"
        );
    }
}

#[tokio::test]
async fn corrupt_history_retention_keeps_only_the_newest_files() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let history_path = directory.path().join("agent-history.json");
    for timestamp in 1..=(MAX_CORRUPT_HISTORY_FILES + 2) {
        let path = directory
            .path()
            .join(format!("agent-history.corrupt-{timestamp:04}.json"));
        tokio::fs::write(path, b"corrupt")
            .await
            .expect("seed quarantined history");
    }

    prune_corrupt_documents(&history_path)
        .await
        .expect("prune quarantined histories");

    let mut directory_entries = tokio::fs::read_dir(directory.path())
        .await
        .expect("read directory");
    let mut names = Vec::new();
    while let Some(entry) = directory_entries.next_entry().await.expect("read entry") {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    assert_eq!(names.len(), MAX_CORRUPT_HISTORY_FILES);
    assert_eq!(
        names,
        vec![
            "agent-history.corrupt-0003.json",
            "agent-history.corrupt-0004.json",
            "agent-history.corrupt-0005.json",
        ]
    );
}

#[tokio::test]
async fn corrupt_history_retention_stops_at_the_scan_budget() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let history_path = directory.path().join("agent-history.json");
    for timestamp in 1..=(MAX_CORRUPT_HISTORY_FILES + 2) {
        let path = directory
            .path()
            .join(format!("agent-history.corrupt-{timestamp:04}.json"));
        tokio::fs::write(path, b"corrupt")
            .await
            .expect("seed quarantined history");
    }

    prune_corrupt_documents_with_limit(&history_path, MAX_CORRUPT_HISTORY_FILES + 1)
        .await
        .expect("prune within bounded scan budget");

    let mut directory_entries = tokio::fs::read_dir(directory.path())
        .await
        .expect("read directory");
    let mut remaining = 0;
    while directory_entries
        .next_entry()
        .await
        .expect("read entry")
        .is_some()
    {
        remaining += 1;
    }
    assert_eq!(remaining, MAX_CORRUPT_HISTORY_FILES + 1);
}

#[tokio::test]
async fn valid_temporary_history_recovers_when_primary_is_missing() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(
        &temporary,
        serde_json::to_vec(&AgentHistoryDocument::default()).expect("serialize history"),
    )
    .await
    .expect("seed temporary history");

    let document = read_document_from(&path)
        .await
        .expect("recover temporary history");

    assert!(document.diagnostics.is_empty());
    assert!(tokio::fs::try_exists(&path).await.expect("check primary"));
    assert!(
        !tokio::fs::try_exists(&temporary)
            .await
            .expect("check temporary")
    );
}

#[tokio::test]
async fn stale_temporary_history_is_removed_when_primary_exists() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(
        &path,
        serde_json::to_vec(&AgentHistoryDocument::default()).expect("serialize history"),
    )
    .await
    .expect("seed primary history");
    tokio::fs::write(&temporary, b"stale")
        .await
        .expect("seed temporary history");

    recover_temporary_document(&path)
        .await
        .expect("remove stale temporary history");

    assert!(tokio::fs::try_exists(&path).await.expect("check primary"));
    assert!(
        !tokio::fs::try_exists(&temporary)
            .await
            .expect("check temporary")
    );
}

#[tokio::test]
async fn invalid_temporary_history_is_removed_when_primary_is_missing() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, b"not-json")
        .await
        .expect("seed temporary history");

    let document = read_document_from(&path)
        .await
        .expect("discard invalid temporary history");

    assert!(document.diagnostics.is_empty());
    assert!(!tokio::fs::try_exists(&path).await.expect("check primary"));
    assert!(
        !tokio::fs::try_exists(&temporary)
            .await
            .expect("check temporary")
    );
}

#[tokio::test]
async fn failed_replacement_discards_temporary_when_primary_remains() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    let temporary = path.with_extension("json.tmp");
    tokio::fs::create_dir(&path)
        .await
        .expect("seed non-removable primary directory");

    assert!(
        write_document_to(&path, &AgentHistoryDocument::default())
            .await
            .is_err()
    );
    assert!(
        tokio::fs::metadata(&path)
            .await
            .expect("primary metadata")
            .is_dir()
    );
    assert!(
        !tokio::fs::try_exists(&temporary)
            .await
            .expect("check failed-write temporary")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn history_primary_recovery_and_quarantine_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    async fn mode(path: &std::path::Path) -> u32 {
        tokio::fs::metadata(path)
            .await
            .expect("history metadata")
            .permissions()
            .mode()
            & 0o777
    }

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    write_document_to(&path, &AgentHistoryDocument::default())
        .await
        .expect("write private history");
    assert_eq!(mode(&path).await, 0o600);

    tokio::fs::remove_file(&path)
        .await
        .expect("remove primary for recovery test");
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(
        &temporary,
        serde_json::to_vec(&AgentHistoryDocument::default()).expect("serialize history"),
    )
    .await
    .expect("seed recovery temporary");
    tokio::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o666))
        .await
        .expect("widen temporary permissions");
    recover_temporary_document(&path)
        .await
        .expect("recover private temporary");
    assert_eq!(mode(&path).await, 0o600);

    tokio::fs::write(&path, b"not-json")
        .await
        .expect("seed corrupt history");
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))
        .await
        .expect("widen corrupt history permissions");
    assert!(read_document_from(&path).await.is_err());

    let mut entries = tokio::fs::read_dir(directory.path())
        .await
        .expect("read quarantine directory");
    let mut quarantined = None;
    while let Some(entry) = entries.next_entry().await.expect("read quarantine entry") {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("agent-history.corrupt-")
        {
            quarantined = Some(entry.path());
            break;
        }
    }
    let quarantined = quarantined.expect("quarantined history");
    assert_eq!(mode(&quarantined).await, 0o600);
}

#[tokio::test]
async fn writer_replaces_an_existing_history_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("agent-history.json");
    tokio::fs::write(&path, b"stale")
        .await
        .expect("seed history file");
    write_document_to(&path, &AgentHistoryDocument::default())
        .await
        .expect("replace history file");
    let bytes = tokio::fs::read(path).await.expect("read history file");
    let document: AgentHistoryDocument =
        serde_json::from_slice(&bytes).expect("parse replaced history file");
    assert!(document.diagnostics.is_empty());
    assert!(document.audits.is_empty());
}

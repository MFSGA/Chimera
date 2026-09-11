use std::{
    collections::VecDeque,
    io::Write,
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::utils::dirs::app_data_dir;

use super::model::{AgentActionKind, AgentProposal};

const HISTORY_FILE: &str = "agent-history.json";
const HISTORY_SCHEMA_VERSION: u32 = 1;
const MAX_DIAGNOSTIC_HISTORY: usize = 100;
const MAX_AUDIT_HISTORY: usize = 200;
const MAX_HISTORY_FILE_BYTES: u64 = 1024 * 1024;

static HISTORY_GATE: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AgentAuditOutcome {
    Proposed,
    Verified,
    ActionNotAvailable,
    ProposalNotFound,
    ProposalExpired,
    DigestMismatch,
    StateChanged,
    RateLimited,
    LimitReached,
    ConfirmationDeclined,
    ActionFailed,
    PartialApply,
    VerificationFailed,
}

impl AgentAuditOutcome {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Verified => "verified",
            Self::ActionNotAvailable => "action_not_available",
            Self::ProposalNotFound => "proposal_not_found",
            Self::ProposalExpired => "proposal_expired",
            Self::DigestMismatch => "digest_mismatch",
            Self::StateChanged => "state_changed",
            Self::RateLimited => "rate_limited",
            Self::LimitReached => "limit_reached",
            Self::ConfirmationDeclined => "confirmation_declined",
            Self::ActionFailed => "action_failed",
            Self::PartialApply => "partial_apply",
            Self::VerificationFailed => "verification_failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentDiagnosticHistoryEntry {
    schema_version: u32,
    captured_at: i64,
    revision: String,
    health: String,
    core_state: String,
    service_state: String,
    finding_codes: Vec<String>,
    probe_failure_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentAuditHistoryEntry {
    schema_version: u32,
    recorded_at: i64,
    proposal_id: String,
    action: AgentActionKind,
    snapshot_revision: String,
    outcome: AgentAuditOutcome,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentHistoryDocument {
    diagnostics: VecDeque<AgentDiagnosticHistoryEntry>,
    audits: VecDeque<AgentAuditHistoryEntry>,
}

pub(super) async fn record_audit(proposal: &AgentProposal, outcome: AgentAuditOutcome) {
    if !is_lower_hex(&proposal.snapshot_revision, 64) {
        tracing::warn!(target: "agent_audit", "discarding audit with invalid snapshot revision");
        return;
    }

    let _guard = HISTORY_GATE.lock().await;
    let path = match history_path() {
        Ok(path) => path,
        Err(_) => {
            tracing::warn!(target: "agent_audit", "failed to resolve agent history path");
            return;
        }
    };
    let mut document = match load_document(path.clone()).await {
        Ok(document) => document,
        Err(_) => {
            tracing::warn!(target: "agent_audit", "failed to load agent history");
            return;
        }
    };

    document.audits.push_back(AgentAuditHistoryEntry {
        schema_version: HISTORY_SCHEMA_VERSION,
        recorded_at: chrono::Utc::now().timestamp_millis(),
        proposal_id: proposal_reference(&proposal.id),
        action: proposal.action.kind(),
        snapshot_revision: proposal.snapshot_revision.clone(),
        outcome,
    });
    trim(&mut document.audits, MAX_AUDIT_HISTORY);

    if save_document(path, document).await.is_err() {
        tracing::warn!(target: "agent_audit", "failed to persist agent history");
    }
}

pub(super) fn proposal_reference(proposal_id: &str) -> String {
    hex::encode(&Sha256::digest(proposal_id.as_bytes())[..16])
}

fn history_path() -> anyhow::Result<PathBuf> {
    Ok(app_data_dir()?.join(HISTORY_FILE))
}

async fn load_document(path: PathBuf) -> anyhow::Result<AgentHistoryDocument> {
    tokio::task::spawn_blocking(move || load_document_blocking(&path))
        .await
        .map_err(|_| anyhow::anyhow!("agent history load task failed"))?
}

fn load_document_blocking(path: &Path) -> anyhow::Result<AgentHistoryDocument> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > MAX_HISTORY_FILE_BYTES {
        return Err(anyhow::anyhow!("agent history path is invalid"));
    }

    let bytes = std::fs::read(path)?;
    if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
        return Err(anyhow::anyhow!("agent history file exceeds size limit"));
    }

    match serde_json::from_slice::<AgentHistoryDocument>(&bytes) {
        Ok(mut document) => {
            normalize_history_document(&mut document);
            Ok(document)
        }
        Err(_) => {
            quarantine_corrupt_document(path)?;
            Ok(Default::default())
        }
    }
}

async fn save_document(path: PathBuf, document: AgentHistoryDocument) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(&document)?;
    if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
        return Err(anyhow::anyhow!("agent history document exceeds size limit"));
    }

    tokio::task::spawn_blocking(move || write_document_blocking(&path, &bytes))
        .await
        .map_err(|_| anyhow::anyhow!("agent history save task failed"))?
}

fn write_document_blocking(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("agent history path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    AtomicFile::new(path, AllowOverwrite).write(|file| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)
    })?;
    Ok(())
}

fn quarantine_corrupt_document(path: &Path) -> anyhow::Result<()> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let suffix = nanoid::nanoid!(8);
    let quarantined = path.with_file_name(format!(
        "agent-history.corrupt-{timestamp}-{suffix}.json"
    ));
    std::fs::rename(path, quarantined)?;
    Ok(())
}

fn normalize_history_document(document: &mut AgentHistoryDocument) {
    document.diagnostics.retain_mut(normalize_diagnostic_entry);
    document.audits.retain_mut(|entry| {
        if entry.schema_version != HISTORY_SCHEMA_VERSION
            || !is_valid_history_timestamp(entry.recorded_at)
            || !is_lower_hex(&entry.snapshot_revision, 64)
        {
            return false;
        }
        if !is_lower_hex(&entry.proposal_id, 32) {
            entry.proposal_id = proposal_reference(&entry.proposal_id);
        }
        true
    });
    trim(&mut document.diagnostics, MAX_DIAGNOSTIC_HISTORY);
    trim(&mut document.audits, MAX_AUDIT_HISTORY);
}

fn normalize_diagnostic_entry(entry: &mut AgentDiagnosticHistoryEntry) -> bool {
    if entry.schema_version != HISTORY_SCHEMA_VERSION
        || !is_valid_history_timestamp(entry.captured_at)
        || !is_lower_hex(&entry.revision, 64)
        || !matches!(
            entry.health.as_str(),
            "healthy" | "warning" | "critical" | "degraded"
        )
        || !matches!(entry.core_state.as_str(), "running" | "stopped" | "unknown")
        || !matches!(
            entry.service_state.as_str(),
            "not_installed" | "stopped" | "running" | "unknown"
        )
        || !entry.finding_codes.iter().all(|code| is_finding_code(code))
        || !entry
            .probe_failure_codes
            .iter()
            .all(|code| is_probe_failure_code(code))
    {
        return false;
    }
    deduplicate_codes(&mut entry.finding_codes);
    deduplicate_codes(&mut entry.probe_failure_codes);
    true
}

fn is_finding_code(code: &str) -> bool {
    matches!(
        code,
        "weak_controller_secret"
            | "system_proxy_without_running_core"
            | "system_proxy_endpoint_mismatch"
            | "runtime_config_missing"
            | "active_profile_missing"
            | "service_mode_inconsistent"
            | "clash_connector_disconnected"
            | "tun_runtime_mismatch"
            | "recent_core_errors"
    )
}

fn is_probe_failure_code(code: &str) -> bool {
    matches!(
        code,
        "core_status_unavailable"
            | "core_config_unavailable"
            | "system_proxy_unavailable"
            | "service_status_unavailable"
            | "service_status_timeout"
            | "telemetry_unavailable"
    )
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_valid_history_timestamp(timestamp: i64) -> bool {
    timestamp >= 0 && chrono::DateTime::from_timestamp_millis(timestamp).is_some()
}

fn deduplicate_codes<T: Clone + PartialEq>(codes: &mut Vec<T>) {
    let mut seen = Vec::with_capacity(codes.len());
    codes.retain(|code| {
        if seen.contains(code) {
            false
        } else {
            seen.push(code.clone());
            true
        }
    });
}

fn trim<T>(entries: &mut VecDeque<T>, maximum: usize) {
    while entries.len() > maximum {
        entries.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{
        AgentAuditHistoryEntry, AgentAuditOutcome, AgentHistoryDocument, is_lower_hex,
        normalize_history_document, proposal_reference, trim,
    };
    use crate::features::agent::model::AgentActionKind;

    #[test]
    fn proposal_references_are_fixed_lower_hex_and_hide_raw_tokens() {
        let proposal_id = "proposal-token-canary";
        let reference = proposal_reference(proposal_id);

        assert!(is_lower_hex(&reference, 32));
        assert!(!reference.contains(proposal_id));
        assert_eq!(reference, proposal_reference(proposal_id));
    }

    #[test]
    fn audit_history_normalization_hashes_legacy_ids_and_rejects_invalid_revisions() {
        let valid_revision = "a".repeat(64);
        let reference = proposal_reference("legacy-proposal-token-canary");
        let mut document = AgentHistoryDocument {
            diagnostics: VecDeque::new(),
            audits: VecDeque::from([
                AgentAuditHistoryEntry {
                    schema_version: 1,
                    recorded_at: 1,
                    proposal_id: "legacy-proposal-token-canary".into(),
                    action: AgentActionKind::SetRoutingMode,
                    snapshot_revision: valid_revision,
                    outcome: AgentAuditOutcome::Verified,
                },
                AgentAuditHistoryEntry {
                    schema_version: 1,
                    recorded_at: 2,
                    proposal_id: "secret-canary".into(),
                    action: AgentActionKind::SetRoutingMode,
                    snapshot_revision: "connection-target-canary".into(),
                    outcome: AgentAuditOutcome::ActionFailed,
                },
            ]),
        };

        normalize_history_document(&mut document);

        assert_eq!(document.audits.len(), 1);
        assert_eq!(document.audits[0].proposal_id, reference);
        let serialized = serde_json::to_string(&document).unwrap();
        for forbidden in [
            "legacy-proposal-token-canary",
            "secret-canary",
            "connection-target-canary",
        ] {
            assert!(!serialized.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn trim_keeps_only_the_newest_entries() {
        let mut entries = VecDeque::from([1, 2, 3, 4]);
        trim(&mut entries, 2);
        assert_eq!(entries, VecDeque::from([3, 4]));
    }
}

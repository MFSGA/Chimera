use std::{collections::VecDeque, sync::Arc};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use super::{
    AgentActionRequest, AgentCommandError, AgentNetworkSnapshot, ports::AgentHistoryPersistencePort,
};

const HISTORY_SCHEMA_VERSION: u32 = 1;
const MAX_DIAGNOSTIC_HISTORY: usize = 100;
const MAX_AUDIT_HISTORY: usize = 200;
pub(super) const MAX_CORRUPT_HISTORY_FILES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentAuditOutcome {
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
    BridgeStartFailed,
    HistoryClearFailed,
}

impl AgentAuditOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
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
            Self::BridgeStartFailed => "bridge_start_failed",
            Self::HistoryClearFailed => "history_clear_failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentDiagnosticHistoryEntry {
    pub schema_version: u32,
    pub captured_at: i64,
    pub revision: String,
    pub health: super::model::AgentHealth,
    pub core_state: super::model::AgentCoreState,
    pub service_state: super::model::AgentServiceState,
    pub finding_codes: Vec<super::model::AgentFindingCode>,
    pub probe_failure_codes: Vec<super::model::AgentProbeCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentAuditHistoryEntry {
    pub schema_version: u32,
    pub recorded_at: i64,
    pub proposal_id: String,
    pub action: super::model::AgentActionKind,
    pub snapshot_revision: String,
    pub outcome: AgentAuditOutcome,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentHistoryDocument {
    pub(super) diagnostics: VecDeque<AgentDiagnosticHistoryEntry>,
    pub(super) audits: VecDeque<AgentAuditHistoryEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentHealthTrend {
    InsufficientData,
    Stable,
    Improving,
    Worsening,
}

#[derive(Debug, Clone, Serialize, Type)]
pub(crate) struct AgentFindingHistoryCount {
    pub code: super::model::AgentFindingCode,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub(crate) struct AgentProbeFailureHistoryCount {
    pub code: super::model::AgentProbeCode,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub(crate) struct AgentHistorySummary {
    pub diagnostic_samples: u32,
    pub unhealthy_samples: u32,
    pub latest_health: Option<super::model::AgentHealth>,
    pub health_trend: AgentHealthTrend,
    pub finding_counts: Vec<AgentFindingHistoryCount>,
    pub probe_failure_counts: Vec<AgentProbeFailureHistoryCount>,
    pub action_attempts: u32,
    pub verified_actions: u32,
    pub attention_actions: u32,
    pub partial_actions: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub(crate) struct AgentHistorySnapshot {
    pub summary: AgentHistorySummary,
    pub diagnostics: Vec<AgentDiagnosticHistoryEntry>,
    pub audits: Vec<AgentAuditHistoryEntry>,
}

pub(crate) struct AgentHistoryStore {
    document: Option<AgentHistoryDocument>,
    persistence: Arc<dyn AgentHistoryPersistencePort>,
}

impl AgentHistoryStore {
    pub(crate) fn new(persistence: Arc<dyn AgentHistoryPersistencePort>) -> Self {
        Self {
            document: None,
            persistence,
        }
    }

    pub(crate) async fn record_snapshot(&mut self, snapshot: &AgentNetworkSnapshot) {
        let entry = AgentDiagnosticHistoryEntry {
            schema_version: HISTORY_SCHEMA_VERSION,
            captured_at: snapshot.captured_at,
            revision: snapshot.revision.clone(),
            health: snapshot.health,
            core_state: snapshot.core.state,
            service_state: snapshot.service.state,
            finding_codes: snapshot.findings.iter().map(|item| item.code).collect(),
            probe_failure_codes: snapshot
                .probe_failures
                .iter()
                .map(|item| item.code)
                .collect(),
        };
        self.update(|document| {
            if document
                .diagnostics
                .back()
                .is_some_and(|previous| previous.revision == entry.revision)
            {
                return false;
            }
            document.diagnostics.push_back(entry);
            trim(&mut document.diagnostics, MAX_DIAGNOSTIC_HISTORY);
            true
        })
        .await;
    }

    pub(crate) async fn record_audit(
        &mut self,
        proposal_id: &str,
        action: &AgentActionRequest,
        snapshot_revision: &str,
        outcome: AgentAuditOutcome,
    ) {
        if !is_lower_hex(snapshot_revision, 64) {
            tracing::warn!(target: "agent_audit", "discarding audit with invalid snapshot revision");
            return;
        }
        let entry = AgentAuditHistoryEntry {
            schema_version: HISTORY_SCHEMA_VERSION,
            recorded_at: chrono::Utc::now().timestamp_millis(),
            proposal_id: history_proposal_reference(proposal_id),
            action: action.kind(),
            snapshot_revision: snapshot_revision.to_owned(),
            outcome,
        };
        self.update(|document| {
            document.audits.push_back(entry);
            trim(&mut document.audits, MAX_AUDIT_HISTORY);
            true
        })
        .await;
    }

    pub(crate) async fn snapshot(&mut self) -> AgentHistorySnapshot {
        let document = load_cached(&mut self.document, self.persistence.as_ref()).await;
        history_snapshot(document)
    }

    pub(crate) async fn clear(&mut self) -> Result<AgentHistorySnapshot, AgentCommandError> {
        let document = AgentHistoryDocument::default();
        self.persistence.save(&document).await.map_err(|_| {
            tracing::warn!(target: "agent_audit", "failed to clear agent history");
            AgentCommandError::HistoryClearFailed
        })?;
        self.document = Some(document.clone());
        Ok(history_snapshot(document))
    }

    async fn update<F>(&mut self, mutate: F)
    where
        F: FnOnce(&mut AgentHistoryDocument) -> bool,
    {
        let current = load_cached(&mut self.document, self.persistence.as_ref()).await;
        let mut next = current.clone();
        if !mutate(&mut next) {
            return;
        }
        match self.persistence.save(&next).await {
            Ok(()) => self.document = Some(next),
            Err(_) => {
                tracing::warn!(target: "agent_audit", "failed to persist agent history");
            }
        }
    }
}

async fn load_cached(
    guard: &mut Option<AgentHistoryDocument>,
    persistence: &dyn AgentHistoryPersistencePort,
) -> AgentHistoryDocument {
    if let Some(document) = guard.as_ref() {
        return document.clone();
    }
    let document = match persistence.load().await {
        Ok(document) => document,
        Err(_) => {
            tracing::warn!(target: "agent_audit", "failed to load agent history");
            AgentHistoryDocument::default()
        }
    };
    *guard = Some(document.clone());
    document
}

fn history_snapshot(document: AgentHistoryDocument) -> AgentHistorySnapshot {
    let summary = summarize_history(&document.diagnostics, &document.audits);
    AgentHistorySnapshot {
        summary,
        diagnostics: document.diagnostics.into_iter().collect(),
        audits: document.audits.into_iter().collect(),
    }
}

fn summarize_history(
    diagnostics: &VecDeque<AgentDiagnosticHistoryEntry>,
    audits: &VecDeque<AgentAuditHistoryEntry>,
) -> AgentHistorySummary {
    let diagnostic_samples = diagnostics.len() as u32;
    let unhealthy_samples = diagnostics
        .iter()
        .filter(|entry| entry.health != super::model::AgentHealth::Healthy)
        .count() as u32;
    let latest_health = diagnostics.back().map(|entry| entry.health);
    let health_trend = match (diagnostics.front(), diagnostics.back()) {
        (Some(first), Some(last)) if diagnostics.len() >= 2 => {
            match health_rank(last.health).cmp(&health_rank(first.health)) {
                std::cmp::Ordering::Less => AgentHealthTrend::Improving,
                std::cmp::Ordering::Equal => AgentHealthTrend::Stable,
                std::cmp::Ordering::Greater => AgentHealthTrend::Worsening,
            }
        }
        _ => AgentHealthTrend::InsufficientData,
    };

    let mut finding_counts = Vec::<AgentFindingHistoryCount>::new();
    let mut probe_failure_counts = Vec::<AgentProbeFailureHistoryCount>::new();
    for entry in diagnostics {
        for code in &entry.finding_codes {
            increment_finding_count(&mut finding_counts, *code);
        }
        for code in &entry.probe_failure_codes {
            increment_probe_failure_count(&mut probe_failure_counts, *code);
        }
    }
    finding_counts.sort_by_key(|entry| std::cmp::Reverse(entry.count));
    probe_failure_counts.sort_by_key(|entry| std::cmp::Reverse(entry.count));

    let mut action_attempts: u32 = 0;
    let mut verified_actions: u32 = 0;
    let mut partial_actions: u32 = 0;
    for entry in audits {
        if entry.outcome == AgentAuditOutcome::Proposed {
            continue;
        }
        action_attempts += 1;
        if entry.outcome == AgentAuditOutcome::Verified {
            verified_actions += 1;
        }
        if entry.outcome == AgentAuditOutcome::PartialApply {
            partial_actions += 1;
        }
    }

    AgentHistorySummary {
        diagnostic_samples,
        unhealthy_samples,
        latest_health,
        health_trend,
        finding_counts,
        probe_failure_counts,
        action_attempts,
        verified_actions,
        attention_actions: action_attempts.saturating_sub(verified_actions),
        partial_actions,
    }
}

fn health_rank(health: super::model::AgentHealth) -> u8 {
    match health {
        super::model::AgentHealth::Healthy => 0,
        super::model::AgentHealth::Warning => 1,
        super::model::AgentHealth::Degraded => 2,
        super::model::AgentHealth::Critical => 3,
    }
}

fn increment_finding_count(
    counts: &mut Vec<AgentFindingHistoryCount>,
    code: super::model::AgentFindingCode,
) {
    if let Some(entry) = counts.iter_mut().find(|entry| entry.code == code) {
        entry.count += 1;
    } else {
        counts.push(AgentFindingHistoryCount { code, count: 1 });
    }
}

fn increment_probe_failure_count(
    counts: &mut Vec<AgentProbeFailureHistoryCount>,
    code: super::model::AgentProbeCode,
) {
    if let Some(entry) = counts.iter_mut().find(|entry| entry.code == code) {
        entry.count += 1;
    } else {
        counts.push(AgentProbeFailureHistoryCount { code, count: 1 });
    }
}

fn trim<T>(entries: &mut VecDeque<T>, maximum: usize) {
    while entries.len() > maximum {
        entries.pop_front();
    }
}

fn history_proposal_reference(proposal_id: &str) -> String {
    hex::encode(&Sha256::digest(proposal_id.as_bytes())[..16])
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

fn deduplicate_codes<T: Copy + PartialEq>(codes: &mut Vec<T>) {
    let mut seen = Vec::with_capacity(codes.len());
    codes.retain(|code| {
        if seen.contains(code) {
            false
        } else {
            seen.push(*code);
            true
        }
    });
}

pub(super) fn normalize_history_document(document: &mut AgentHistoryDocument) {
    document.diagnostics.retain_mut(|entry| {
        if entry.schema_version != HISTORY_SCHEMA_VERSION
            || !is_valid_history_timestamp(entry.captured_at)
            || !is_lower_hex(&entry.revision, 64)
        {
            return false;
        }
        deduplicate_codes(&mut entry.finding_codes);
        deduplicate_codes(&mut entry.probe_failure_codes);
        true
    });
    document.audits.retain_mut(|entry| {
        if entry.schema_version != HISTORY_SCHEMA_VERSION
            || !is_valid_history_timestamp(entry.recorded_at)
            || !is_lower_hex(&entry.snapshot_revision, 64)
        {
            return false;
        }
        if !is_lower_hex(&entry.proposal_id, 32) {
            entry.proposal_id = history_proposal_reference(&entry.proposal_id);
        }
        true
    });
    trim(&mut document.diagnostics, MAX_DIAGNOSTIC_HISTORY);
    trim(&mut document.audits, MAX_AUDIT_HISTORY);
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

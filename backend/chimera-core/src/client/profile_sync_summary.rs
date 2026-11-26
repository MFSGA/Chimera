//! Secret-free Profile job observations.
use super::profiles::ProfileSyncStatus;

/// Secret-free status for ordinary UI surfaces. Registration errors may carry
/// subscription URLs, so expose only their presence.
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct ProfileSyncSummary {
    pub scheduled: bool,
    pub next_run_at: Option<String>,
    pub active: Vec<SyncRunSummary>,
    pub journal_degraded: bool,
    pub has_registration_error: bool,
    pub history_limit: usize,
}

impl From<&ProfileSyncStatus> for ProfileSyncSummary {
    fn from(status: &ProfileSyncStatus) -> Self {
        Self {
            scheduled: status.scheduled,
            next_run_at: status.next_run_at.clone(),
            active: status.active.iter().map(Into::into).collect(),
            journal_degraded: status.journal_degraded,
            has_registration_error: status.registration_error.is_some(),
            history_limit: status.history_limit,
        }
    }
}

/// A sync run without job keys, output, failure text, or log counters.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct SyncRunSummary {
    pub id: String,
    pub admission_sequence: String,
    pub trigger: String,
    pub scheduled_at: Option<String>,
    pub admitted_at: String,
    pub finished_at: Option<String>,
    pub state: SyncRunStateSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncRunStateSummary {
    Admitted,
    Running,
    Cancelling,
    Finalizing,
    Finished {
        outcome: SyncOutcomeKind,
        journal_degraded: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcomeKind {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    Skipped,
}

impl From<&chimera_jobs::dto::RunDto> for SyncRunSummary {
    fn from(run: &chimera_jobs::dto::RunDto) -> Self {
        use chimera_jobs::dto::{JournalDto, OutcomeDto, RunStateDto};

        let state = match &run.state {
            RunStateDto::Admitted => SyncRunStateSummary::Admitted,
            RunStateDto::Running => SyncRunStateSummary::Running,
            RunStateDto::Cancelling => SyncRunStateSummary::Cancelling,
            RunStateDto::Finalizing => SyncRunStateSummary::Finalizing,
            RunStateDto::Finished { completion } => SyncRunStateSummary::Finished {
                outcome: match &completion.outcome {
                    OutcomeDto::Succeeded => SyncOutcomeKind::Succeeded,
                    OutcomeDto::Failed { .. } => SyncOutcomeKind::Failed,
                    OutcomeDto::Cancelled => SyncOutcomeKind::Cancelled,
                    OutcomeDto::Interrupted => SyncOutcomeKind::Interrupted,
                    OutcomeDto::Skipped => SyncOutcomeKind::Skipped,
                },
                journal_degraded: matches!(completion.journal, JournalDto::Degraded { .. }),
            },
        };
        Self {
            id: run.id.clone(),
            admission_sequence: run.admission_sequence.clone(),
            trigger: run.trigger.clone(),
            scheduled_at: run.scheduled_at.clone(),
            admitted_at: run.admitted_at.clone(),
            finished_at: run.finished_at.clone(),
            state,
        }
    }
}

#[derive(Debug, serde::Serialize, specta::Type)]
pub struct SyncRunPageSummary {
    pub items: Vec<SyncRunSummary>,
    pub next: Option<chimera_jobs::dto::RunCursorDto>,
}

impl From<&chimera_jobs::dto::RunPageDto> for SyncRunPageSummary {
    fn from(page: &chimera_jobs::dto::RunPageDto) -> Self {
        Self {
            items: page.items.iter().map(Into::into).collect(),
            next: page.next.clone(),
        }
    }
}

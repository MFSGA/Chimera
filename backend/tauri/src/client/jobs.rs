//! Composition of the shared job owner; business registrations stay with their domain.
use std::path::{Path, PathBuf};

use crate::utils::{
    recovery_io::IoFailure,
    redb_ext::{RedbFailure, find_failure},
};
use chimera_jobs::{JobsClient, JobsService, Limits, LogCapture, LogPolicy, RedbJobStore};
use snafu::ResultExt as _;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::instrument::WithSubscriber;

pub(crate) const SYNC_LOG_TARGET: &str = "chimera::profile_sync";
pub(crate) use chimera_core::client::profiles::DEFAULT_HISTORY_LIMIT;

pub(crate) fn capture() -> LogCapture {
    let mut policy = LogPolicy::default();
    policy.targets.insert(
        SYNC_LOG_TARGET.into(),
        ["message", "stage", "code"]
            .into_iter()
            .map(String::from)
            .collect(),
    );
    LogCapture::new(policy)
}

pub(crate) async fn start(
    path: PathBuf,
    capture: LogCapture,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> Result<JobsClient, Error> {
    let mut service = match start_service(&path, capture.clone()).await {
        Err(error) if error.unusable_journal() => {
            let journal = path.clone();
            crate::utils::blocking::join(
                tokio::task::spawn_blocking(move || {
                    crate::core::corrupt::preserve(&journal, &error)
                })
                .await,
            )
            .context(PreserveStoreSnafu)?;
            tracing::warn!(path = %path.display(), action = "reset_jobs_journal",
                "resetting the jobs journal discards run history; schedules are registered again");
            start_service(&path, capture).await?
        }
        result => result?,
    };
    let client = service.client();
    tasks.spawn(async move {
        shutdown.cancelled().await;
        // The owner must stay alive through finalization; an incomplete drain
        // continues on the same service instead of dropping active work.
        loop {
            match service.shutdown().await {
                Ok(report) if report.closed => break,
                Ok(_) => continue,
                Err(chimera_jobs::Error::TimedOut) => {
                    tracing::warn!("jobs shutdown acknowledgement timed out; retaining the owner");
                }
                Err(error) => panic!("jobs owner stopped before its cleanup completed: {error}"),
            }
        }
    });
    Ok(client)
}

async fn start_service(path: &Path, capture: LogCapture) -> Result<JobsService, Error> {
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let store_path = path.to_owned();
    let store = crate::utils::blocking::join(
        tokio::task::spawn_blocking(move || open_store(&store_path)).await,
    )?;
    let mut limits = Limits::default();
    limits.retention.per_job = DEFAULT_HISTORY_LIMIT;
    // Profile sync coalesces elapsed interval slots after suspend; do not
    // discard an overdue sync.
    limits.lateness = std::time::Duration::MAX;
    JobsService::start(Box::new(store), capture, limits)
        .with_subscriber(dispatch)
        .await
        .context(StartOwnerSnafu)
}

fn open_store(path: &Path) -> Result<RedbJobStore, Error> {
    use crate::core::corrupt;
    match corrupt::retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(metadata) if !metadata.is_file() || corrupt::is_link(&metadata) => {
            return NotAFileSnafu { path }.fail();
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error).context(InspectStoreSnafu { path });
        }
        _ => {}
    }
    corrupt::retry(
        || RedbJobStore::open(path),
        |error| corrupt::error_io(error) == Some(IoFailure::Transient),
    )
    .context(OpenStoreSnafu { path })
}

pub(crate) async fn wait(
    client: &JobsClient,
    id: chimera_jobs::RunId,
) -> Result<chimera_jobs::Completion, chimera_jobs::Error> {
    loop {
        match client.wait(id, None).await {
            Err(chimera_jobs::Error::WaitTimedOut(_)) => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
pub(crate) async fn test_client_with_owner(
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> JobsClient {
    let directory = tempfile::tempdir().unwrap();
    let client = start(
        directory.path().join("jobs.redb"),
        capture(),
        shutdown.clone(),
        tasks,
    )
    .await
    .unwrap();
    tokio::spawn({
        let tasks = tasks.clone();
        async move {
            let _directory = directory;
            shutdown.cancelled().await;
            tasks.wait().await;
        }
    });
    client
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to inspect jobs at {}", path.display()))]
    InspectStore {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("jobs at {} is not a regular file", path.display()))]
    NotAFile { path: PathBuf },
    #[snafu(display("failed to open jobs at {}", path.display()))]
    OpenStore {
        path: PathBuf,
        source: chimera_jobs::Error,
    },
    #[snafu(display("failed to start jobs owner"))]
    StartOwner { source: chimera_jobs::Error },
    #[snafu(display("failed to preserve the unusable jobs journal"))]
    PreserveStore { source: crate::core::corrupt::Error },
}

impl Error {
    /// The journal only holds run history, so a node that is not a regular
    /// file, or a file confirmed corrupt or written in an incompatible layout,
    /// is preserved and rebuilt. Permission, lock and other IO failures are
    /// reported instead.
    fn unusable_journal(&self) -> bool {
        let source = match self {
            Self::NotAFile { .. } => return true,
            Self::OpenStore { source, .. } | Self::StartOwner { source } => source,
            Self::InspectStore { .. } | Self::PreserveStore { .. } => return false,
        };
        matches!(
            source,
            chimera_jobs::Error::JournalRecoveryDecode(_)
                | chimera_jobs::Error::JournalPruneDecode(_)
        ) || matches!(
            find_failure(source),
            Some(RedbFailure::Corrupt | RedbFailure::Incompatible)
        )
    }
}

pub use chimera_core::client::profiles::ProfileSyncStatus;

pub use chimera_core::client::profile_sync_summary::*;

#[cfg(test)]
pub(crate) struct ExplicitTestTime(tokio::task::JoinHandle<()>);
#[cfg(test)]
impl Drop for ExplicitTestTime {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[cfg(test)]
pub(crate) fn explicit_test_time() -> ExplicitTestTime {
    // Blocking journal IO must not make Tokio automatically advance a paused
    // clock while an acknowledgement is still coming from an OS thread.
    ExplicitTestTime(tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    }))
}

#[cfg(test)]
pub(crate) async fn settled(client: &JobsClient) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !client.inspect().await.unwrap().active.is_empty() {
        assert!(std::time::Instant::now() < deadline, "jobs did not settle");
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;
    use chimera_jobs::dto::{
        CompletionDto, JournalDto, OutcomeDto, OutputDto, RunDto, RunPageDto, RunStateDto,
    };

    #[test]
    fn dashboard_summaries_omit_owner_text_and_failure_payloads() {
        let secret = "https://private.invalid/config?token=secret";
        let run = RunDto {
            id: "run-7".into(),
            job: secret.into(),
            definition_version: "revision".into(),
            admission_sequence: "9007199254740993".into(),
            trigger: "manual".into(),
            scheduled_at: None,
            admitted_at: "2026-01-01T00:00:00Z".into(),
            finished_at: Some("2026-01-01T00:00:01Z".into()),
            state: RunStateDto::Finished {
                completion: CompletionDto {
                    outcome: OutcomeDto::Failed {
                        code: secret.into(),
                        message: secret.into(),
                    },
                    output: OutputDto::JsonText {
                        value: secret.into(),
                    },
                    journal: JournalDto::Degraded {
                        code: secret.into(),
                    },
                },
            },
            last_log_sequence: "42".into(),
            dropped_log_count: "1".into(),
        };
        let status = ProfileSyncStatus {
            scheduled: true,
            next_run_at: None,
            latest_run_sequence: Some(run.admission_sequence.clone()),
            active: vec![run.clone()],
            journal_degraded: true,
            registration_error: Some(secret.into()),
            history_limit: DEFAULT_HISTORY_LIMIT,
            source: None,
        };
        let summary = ProfileSyncSummary::from(&status);
        let page = SyncRunPageSummary::from(&RunPageDto {
            items: vec![run],
            next: None,
        });
        let encoded = format!(
            "{}{}",
            serde_json::to_string(&summary).unwrap(),
            serde_json::to_string(&page).unwrap()
        );
        assert!(!encoded.contains(secret));
        assert!(!encoded.contains("last_log_sequence"));
        assert!(!encoded.contains("latest_run_sequence"));
        assert!(summary.has_registration_error);
        assert_eq!(page.items[0].admission_sequence, "9007199254740993");
        assert_eq!(
            page.items[0].state,
            SyncRunStateSummary::Finished {
                outcome: SyncOutcomeKind::Failed,
                journal_degraded: true,
            }
        );
    }
}

#[cfg(test)]
#[path = "jobs_recovery_tests.rs"]
mod recovery_tests;

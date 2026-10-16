//! Composition of the shared job owner; business registrations stay with their domain.
use std::path::PathBuf;

use chimera_config::profile::{ProfileId, ProfileSource};
use chimera_jobs::{JobsClient, JobsService, Limits, LogCapture, LogPolicy, RedbJobStore};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::instrument::WithSubscriber;

pub(crate) const SYNC_LOG_TARGET: &str = "chimera::profile_sync";
pub(crate) const DEFAULT_HISTORY_LIMIT: usize = 10;

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
) -> anyhow::Result<JobsClient> {
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let store = tokio::task::spawn_blocking(move || RedbJobStore::open(path))
        .await
        .map_err(|error| anyhow::anyhow!("jobs journal opener task failed: {error}"))??;
    let mut limits = Limits::default();
    limits.retention.per_job = DEFAULT_HISTORY_LIMIT;
    // Profile sync coalesces elapsed interval slots after suspend; do not
    // discard an overdue sync.
    limits.lateness = std::time::Duration::MAX;
    let mut service = JobsService::start(Box::new(store), capture, limits)
        .with_subscriber(dispatch)
        .await?;
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

#[derive(Debug, serde::Serialize, specta::Type)]
pub struct ProfileSyncStatus {
    pub scheduled: bool,
    pub next_run_at: Option<String>,
    pub latest_run_sequence: Option<String>,
    pub active: Vec<chimera_jobs::dto::RunDto>,
    pub journal_degraded: bool,
    pub registration_error: Option<String>,
    pub history_limit: usize,
}

impl super::profiles::ProfilesClient {
    pub(crate) fn profile_sync_key(
        &self,
        uid: &ProfileId,
    ) -> Result<String, crate::state::profiles::ProfilesError> {
        let snapshot = self.inner.snapshot.load();
        let item = snapshot
            .items
            .get(uid)
            .ok_or_else(|| crate::state::profiles::ProfilesError::ProfileNotFound(uid.clone()))?;
        if !matches!(item.definition.source(), Some(ProfileSource::Remote { .. })) {
            return Err(crate::state::profiles::ProfilesError::NotARemoteProfile);
        }
        Ok(crate::state::profiles::jobs::sync_key(uid))
    }

    pub(crate) async fn profile_sync_status(
        &self,
        uid: ProfileId,
    ) -> Result<ProfileSyncStatus, crate::state::profiles::ProfilesError> {
        let key = self.profile_sync_key(&uid)?;
        let view = self.inner.jobs.client.inspect().await?;
        let latest_run_sequence = self
            .inner
            .jobs
            .client
            .runs(key.clone(), None, DEFAULT_HISTORY_LIMIT)
            .await?
            .items
            .into_iter()
            .map(|run| run.admission_sequence)
            .max()
            .map(|sequence| sequence.to_string());
        let definition = view.jobs.iter().find(|job| job.definition.key == key);
        Ok(ProfileSyncStatus {
            scheduled: definition.is_some_and(|job| {
                !matches!(job.definition.schedule, chimera_jobs::Schedule::Manual)
            }),
            next_run_at: definition.and_then(|job| job.next_run_at.map(|time| time.to_string())),
            latest_run_sequence,
            active: view
                .active
                .into_iter()
                .filter(|run| run.job == key)
                .map(Into::into)
                .collect(),
            journal_degraded: view.journal_degraded,
            registration_error: view
                .scopes
                .get(crate::state::profiles::jobs::SCOPE)
                .and_then(|scope| scope.last_error.clone()),
            history_limit: DEFAULT_HISTORY_LIMIT,
        })
    }

    pub(crate) async fn profile_sync_runs(
        &self,
        uid: ProfileId,
        after: Option<chimera_jobs::dto::RunCursorDto>,
    ) -> Result<chimera_jobs::dto::RunPageDto, crate::state::profiles::ProfilesError> {
        let key = self.profile_sync_key(&uid)?;
        Ok(self
            .inner
            .jobs
            .client
            .runs(key, after.map(TryInto::try_into).transpose()?, 10)
            .await?
            .into())
    }

    pub(crate) async fn profile_sync_logs(
        &self,
        uid: ProfileId,
        run: String,
        after: Option<String>,
    ) -> Result<chimera_jobs::dto::LogPageDto, crate::state::profiles::ProfilesError> {
        let key = self.profile_sync_key(&uid)?;
        let id = run
            .parse()
            .map_err(|_| chimera_jobs::Error::Invalid("invalid run ID".into()))?;
        let client = self.inner.jobs.client.clone();
        if client.get_run(id).await?.job != key {
            return Err(chimera_jobs::Error::NotFound.into());
        }
        let after = after
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| chimera_jobs::Error::Invalid("invalid log cursor".into()))?
            .unwrap_or(0);
        Ok(client.logs(id, after, 100).await?.into())
    }

    #[cfg(test)]
    pub(crate) fn jobs(&self) -> JobsClient {
        self.inner.jobs.client.clone()
    }
}

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

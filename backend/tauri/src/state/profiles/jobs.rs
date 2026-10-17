//! Job registrations derived from the profiles serial owner's committed slice.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chimera_config::profile::{ProfileId, ProfileSource, Profiles, RemoteProfileOptionsPatch};
use chimera_jobs::{Job, JobDefinition, JobError, JobsClient, RunId, Schedule, Trigger};
use ractor::{ActorRef, rpc::CallResult};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::{CommitReport, ProfilesActorMessage, ProfilesError};

pub(crate) const SCOPE: &str = "profiles";
pub(crate) fn sync_key(uid: &ProfileId) -> String {
    format!("profiles/sync/{uid}")
}

type SyncReply = oneshot::Sender<Result<CommitReport, ProfilesError>>;

#[derive(Clone)]
pub(crate) struct ProfileJobs {
    pub client: JobsClient,
    // Only transports non-serializable RPC replies; domain state and execution
    // remain in ProfilesActor. The handler takes its sender before doing work.
    replies: Arc<Mutex<HashMap<RunId, SyncReply>>>,
}
struct SyncWaiter {
    id: RunId,
    replies: Arc<Mutex<HashMap<RunId, SyncReply>>>,
}
impl Drop for SyncWaiter {
    fn drop(&mut self) {
        self.replies.lock().unwrap().remove(&self.id);
    }
}

#[derive(Clone)]
pub struct ProfileSyncContext(pub chimera_jobs::JobContext);
impl std::fmt::Debug for ProfileSyncContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileSyncContext")
            .field("run_id", &self.0.run_id)
            .finish()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SyncInput {
    request: Option<RunId>,
    patch: Option<RemoteProfileOptionsPatch>,
}
#[derive(Serialize)]
struct SyncSummary {
    commit: crate::client::runtime::CommitReceipt,
    degradation_count: usize,
}

impl ProfileJobs {
    pub fn new(client: JobsClient) -> Self {
        Self {
            client,
            replies: Arc::default(),
        }
    }

    pub async fn sync(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<CommitReport, ProfilesError> {
        let id = RunId::new_v4();
        let (send, receive) = oneshot::channel();
        self.replies.lock().unwrap().insert(id, send);
        let _waiter = SyncWaiter {
            id,
            replies: self.replies.clone(),
        };
        let input = SyncInput {
            request: Some(id),
            patch,
        };
        let run = match self.client.run_with(sync_key(&uid), &input).await {
            Ok(run) => run.id,
            // Admission uncertainty is resolved with the same identity; it is
            // never retried as a new download.
            Err(chimera_jobs::Error::AdmissionUnknown(run)) => run,
            Err(error) => {
                self.replies.lock().unwrap().remove(&id);
                return Err(error.into());
            }
        };
        let completion = crate::client::jobs::wait(&self.client, run).await?;
        let mut journal_unavailable = completion.journal != chimera_jobs::JournalState::Durable;
        if !journal_unavailable {
            // Durable result publication precedes final maintenance. Do not
            // return while the same key is still reserved by that owner.
            loop {
                match self.client.inspect().await {
                    Ok(view) if view.active.iter().any(|active| active.id == run) => {
                        tokio::task::yield_now().await
                    }
                    Ok(_) => break,
                    Err(_) => {
                        journal_unavailable = true;
                        break;
                    }
                }
            }
        }
        let mut report = receive.await.map_err(|_| ProfilesError::RefreshFailed {
            message: "profile sync result unavailable".into(),
        })??;
        if journal_unavailable {
            report
                .runtime_degradations
                .push(crate::client::runtime::Degradation {
                    phase: crate::client::runtime::DegradationPhase::SystemEffect,
                    code: "jobs_journal_unavailable".into(),
                    message: "Profile committed, but the sync journal is unavailable".into(),
                    retryable: true,
                });
        }
        Ok(report)
    }

    pub async fn reconcile(
        &self,
        profiles: &Profiles,
        actor: &ActorRef<ProfilesActorMessage>,
        revision: u64,
        running: bool,
    ) -> Result<(), chimera_jobs::Error> {
        let mut registrations = Vec::new();
        for (uid, item) in &profiles.items {
            let Some(ProfileSource::Remote { option, .. }) = item.definition.source() else {
                continue;
            };
            let mut definition = JobDefinition::manual(sync_key(uid));
            definition.scope = SCOPE.into();
            definition.name = "Profile sync".into();
            if running && let Some(minutes) = option.update_interval_minutes.minutes() {
                definition.schedule = Schedule::Interval {
                    every_ms: minutes.get().saturating_mul(60_000),
                };
            }
            let (actor, uid, replies) = (actor.clone(), uid.clone(), self.replies.clone());
            registrations.push(Job::new(definition, SyncInput::default(), move |context, input: SyncInput| {
                let (actor, uid, replies) = (actor.clone(), uid.clone(), replies.clone());
                async move {
                    let reply = input.request.and_then(|id| replies.lock().unwrap().remove(&id));
                    tracing::info!(target: "chimera::profile_sync", stage = "started", "Profile sync started");
                    let result = match actor.call(|reply| ProfilesActorMessage::SyncRemote { uid, patch: input.patch, context: ProfileSyncContext(context.clone()), reply }, None).await {
                        Ok(CallResult::Success(result)) => result,
                        _ => Err(ProfilesError::Rpc("profiles actor stopped".into())),
                    };
                    let summary = match &result {
                        Ok(report) => {
                            let count = report.degradations.len() + report.runtime_degradations.len();
                            tracing::info!(target: "chimera::profile_sync", stage = "committed", "Profile sync committed");
                            if count > 0 {
                                tracing::warn!(target: "chimera::profile_sync", stage = "degraded", "Profile committed with degraded effects");
                            }
                            Ok(SyncSummary { commit: report.receipt.clone(), degradation_count: count })
                        }
                        Err(error) => {
                            let error = sync_error(error);
                            tracing::warn!(target: "chimera::profile_sync", stage = "failed", code = %error.code, "Profile sync failed");
                            Err(error)
                        }
                    };
                    if let Some(reply) = reply { let _ = reply.send(result); }
                    summary
                }
            })?);
        }
        self.client.reconcile(SCOPE, revision, registrations).await
    }

    pub async fn catch_up(&self, profiles: &Profiles) {
        for (uid, item) in &profiles.items {
            let Some(ProfileSource::Remote {
                option,
                materialized,
                ..
            }) = item.definition.source()
            else {
                continue;
            };
            let Some(minutes) = option.update_interval_minutes.minutes() else {
                continue;
            };
            let overdue = materialized.updated_at.is_none_or(|at| {
                (time::OffsetDateTime::now_utc() - at).whole_seconds() as i128
                    >= i128::from(minutes.get()) * 60
            });
            if overdue
                && let Err(error) = self
                    .client
                    .submit(
                        sync_key(uid),
                        RunId::new_v4(),
                        None,
                        Trigger::StartupCatchUp,
                    )
                    .await
            {
                tracing::warn!(%error, "failed to admit profile startup catch-up");
            }
        }
    }
}

fn sync_error(error: &ProfilesError) -> JobError {
    use ProfilesError as Error;
    use chimera_jobs::Error as JobsError;

    match error {
        Error::RefreshFailed { message } if message.starts_with("download failed:") => {
            JobError::new(
                "subscription_download_failed",
                "Could not download the subscription",
            )
        }
        Error::RefreshFailed { message }
            if message.starts_with("downloaded content is not ")
                || message.starts_with("subscription does not contain ")
                || message == "downloaded script is empty"
                || message.starts_with("stale download no longer valid") =>
        {
            JobError::new(
                "profile_content_rejected",
                "Downloaded profile failed validation",
            )
        }
        Error::RefreshFailed { message } if message == "profile deleted during refresh" => {
            JobError::new(
                "profile_deleted",
                "Profile was deleted before sync could commit",
            )
        }
        Error::RefreshFailed { message }
            if message.contains("subscription definition changed during refresh") =>
        {
            JobError::new(
                "profile_changed",
                "Profile changed before sync could commit",
            )
        }
        Error::RefreshFailed { message } if message == "refresh already in progress" => {
            JobError::new("profile_sync_busy", "A profile sync is already in progress")
        }
        Error::ProfileNotFound(_) => JobError::new("profile_not_found", "Profile no longer exists"),
        Error::NotARemoteProfile => JobError::new(
            "profile_not_remote",
            "Profile is no longer a remote profile",
        ),
        Error::ShuttingDown | Error::ProfileSyncJobs(JobsError::ShuttingDown) => JobError::new(
            "shutting_down",
            "Application stopped before sync could commit",
        ),
        Error::ProfileSyncJobs(JobsError::Busy) => {
            JobError::new("profile_sync_busy", "A profile sync is already in progress")
        }
        Error::Rpc(_) => JobError::new(
            "profile_sync_unavailable",
            "Profile sync could not reach the profile service",
        ),
        _ => JobError::new("profile_sync_failed", "Profile sync did not commit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_errors_keep_safe_failure_categories() {
        let cases = [
            (
                ProfilesError::RefreshFailed {
                    message: "download failed: https://private.example/sub?token=secret".into(),
                },
                "subscription_download_failed",
                "Could not download the subscription",
            ),
            (
                ProfilesError::RefreshFailed {
                    message: "subscription does not contain `proxies` or `proxy-providers`".into(),
                },
                "profile_content_rejected",
                "Downloaded profile failed validation",
            ),
            (
                ProfilesError::RefreshFailed {
                    message: "profile deleted during refresh".into(),
                },
                "profile_deleted",
                "Profile was deleted before sync could commit",
            ),
            (
                ProfilesError::RefreshFailed {
                    message: "subscription definition changed during refresh".into(),
                },
                "profile_changed",
                "Profile changed before sync could commit",
            ),
        ];

        for (source, code, message) in cases {
            let mapped = sync_error(&source);
            assert_eq!(mapped.code, code);
            assert_eq!(mapped.message, message);
            assert!(!mapped.message.contains("private.example"));
            assert!(!mapped.message.contains("secret"));
        }
    }

    #[test]
    fn unknown_refresh_errors_stay_generic_and_do_not_leak_details() {
        let mapped = sync_error(&ProfilesError::RefreshFailed {
            message: "internal path /Users/adam/private-profile.yaml".into(),
        });

        assert_eq!(mapped.code, "profile_sync_failed");
        assert_eq!(mapped.message, "Profile sync did not commit");
    }
}

//! Typed client for the ProfilesActor. Committed reads come from the state
//! snapshot handle; writes go through the actor with no RPC timeout.

use crate::state::mutation::{ProfileContentLock, ProfileContentReader, ProfileMutationPort};

use chimera_migration::modules::profiles::ProfilesFormat;
use std::{sync::Arc, time::Duration};

use crate::{
    profile_import::{self, ProfileUrlProbe},
    state::{PersistentStateManagerSetup, StateSnapshot},
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use chimera_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileLayout, ProfileMetadata,
    ProfileMetadataPatch, Profiles, ProxyGroupLayout, RemoteProfileOptions,
    RemoteProfileOptionsPatch, TransformKind,
};
use chimera_contracts::profiles::error::SubscriptionFetchError;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::state::profiles::{
    CommitReport, ProfilesActor, ProfilesActorArgs, ProfilesActorMessage, ProfilesError, ReorderOp,
    ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
};

#[derive(Clone)]
pub struct ProfilesClient {
    inner: Arc<ProfilesClientInner>,
}

struct ProfilesClientInner {
    actor_ref: ActorRef<ProfilesActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<Profiles>,
    content_lock: ProfileContentLock,
    jobs: crate::state::profiles::jobs::ProfileJobs,
    // Shared with the actor; a probe downloads without touching its state.
    fetcher: Arc<dyn SubscriptionFetcher>,
    #[cfg(any(test, feature = "test-support"))]
    _test_owner: Option<TestJobsOwner>,
}

#[cfg(any(test, feature = "test-support"))]
struct TestJobsOwner {
    shutdown: CancellationToken,
    tasks: TaskTracker,
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TestJobsOwner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.tasks.close();
    }
}

impl ProfilesClient {
    pub async fn new_with_jobs(
        mutations: Arc<dyn ProfileMutationPort>,
        profiles_path: Utf8PathBuf,
        jobs: chimera_jobs::JobsClient,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        Self::build(
            mutations,
            profiles_path,
            jobs,
            fs,
            fetcher,
            materialization,
            shutdown,
            tasks,
            #[cfg(any(test, feature = "test-support"))]
            None,
        )
        .await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn attach_test_owner(&mut self, shutdown: CancellationToken, tasks: TaskTracker) {
        Arc::get_mut(&mut self.inner)
            .expect("new profiles client is uniquely held")
            ._test_owner = Some(TestJobsOwner { shutdown, tasks });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_actor_ref(&self) -> &ActorRef<ProfilesActorMessage> {
        &self.inner.actor_ref
    }

    async fn build(
        mutations: Arc<dyn ProfileMutationPort>,
        profiles_path: Utf8PathBuf,
        jobs: chimera_jobs::JobsClient,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
        #[cfg(any(test, feature = "test-support"))] test_owner: Option<TestJobsOwner>,
    ) -> anyhow::Result<Self> {
        let jobs = crate::state::profiles::jobs::ProfileJobs::new(jobs);
        let should_load = chimera_migration::fs::config_node_exists(profiles_path.as_std_path())?;
        let setup = PersistentStateManagerSetup::<Profiles, ProfilesFormat>::builder()
            .config_path(profiles_path)
            .assemble();
        let mut manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load profiles persistent state manager")?
        } else {
            setup
                .from_state(Profiles::default())
                .await
                .context("failed to initialize profiles persistent state manager")?
        };

        // Match ref: invalid folder presentation metadata must not prevent
        // loading an otherwise valid profile document. Persist the repair with
        // one new durable generation before exposing the snapshot.
        let mut loaded = manager.snapshot().as_ref().clone();
        let repair = loaded.repair_folders();
        let proxy_group_repair = loaded.repair_proxy_group_layouts();
        let dangling_current = loaded.clear_dangling_current();
        if !repair.is_empty() || !proxy_group_repair.is_empty() {
            tracing::warn!(
                ?repair,
                ?proxy_group_repair,
                "repaired malformed profile arrangements"
            );
        }
        if !repair.is_empty() || !proxy_group_repair.is_empty() || dangling_current.is_some() {
            if dangling_current.is_some() {
                loaded.validate().map_err(|errors| {
                    anyhow::anyhow!("profiles.yaml failed validation after repair: {errors:?}")
                })?;
            }
            loaded
                .bump_revision()
                .context("failed to advance profiles revision after repairing arrangements")?;
            manager
                .upsert(loaded)
                .await
                .context("failed to persist repaired profile arrangements")?;
            if let Some(id) = &dangling_current {
                tracing::warn!(
                    original_id = %id,
                    action = "clear_dangling_current",
                    "cleared missing selected profile"
                );
            }
        }

        let snapshot = manager.snapshot_handle();
        let content_lock = ProfileContentLock::default();
        snapshot
            .load()
            .state
            .validate()
            .map_err(|errors| anyhow::anyhow!("profiles.yaml failed validation: {errors:?}"))?;

        let actor_ref = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorArgs {
                mutations,
                content_lock: content_lock.clone(),
                manager,
                fs,
                fetcher: fetcher.clone(),
                materialization,
                jobs: jobs.clone(),
            },
        )
        .await
        .context("failed to spawn profiles actor")?
        .0;
        chimera_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor_ref.get_cell());

        Ok(Self {
            inner: Arc::new(ProfilesClientInner {
                actor_ref,
                snapshot,
                content_lock,
                jobs,
                fetcher,
                #[cfg(any(test, feature = "test-support"))]
                _test_owner: test_owner,
            }),
        })
    }

    /// The last committed profiles document. Reads bypass the mailbox, so an
    /// in-flight transaction parked in `on_prepare` cannot delay them.
    pub fn snapshot(&self) -> Arc<Profiles> {
        Arc::new(self.inner.snapshot.load().state.clone())
    }

    /// Read-only handle for collaborators that must observe committed state
    /// without holding a client that could write it.
    pub fn snapshot_handle(&self) -> StateSnapshot<Profiles> {
        self.inner.snapshot.clone()
    }

    /// Ref: a committed profile document and its managed bytes must be read
    /// under one consistency fence. The client remains read-only to callers.
    pub fn content_reader(&self) -> ProfileContentReader {
        ProfileContentReader::new(self.inner.snapshot.clone(), self.inner.content_lock.clone())
    }

    pub async fn save_file(
        &self,
        uid: ProfileId,
        content: String,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SaveFile {
                uid,
                content,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn set_current(
        &self,
        current: Option<ProfileId>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetCurrent { current, reply },
            None,
        )
        .await
    }

    /// Atomically activate `uid` only if nothing is currently selected.
    /// Returns `Some(report)` when it activated, `None` when a current already
    /// existed (so the caller's activation was intentionally skipped).
    pub async fn set_current_if_none(
        &self,
        uid: ProfileId,
    ) -> Result<Option<CommitReport>, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetCurrentIfNone { uid, reply },
            None,
        )
        .await
    }

    pub async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetGlobalTransforms { ids, reply },
            None,
        )
        .await
    }

    pub async fn set_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetValidFields { fields, reply },
            None,
        )
        .await
    }

    #[allow(dead_code)]
    pub async fn replace(&self, profiles: Profiles) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::Replace { profiles, reply },
            None,
        )
        .await
    }

    pub async fn add(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::Add {
                request,
                initial_file,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn delete(&self, uid: ProfileId) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Delete { uid, reply }, None)
            .await
    }

    pub async fn reorder(&self, op: ReorderOp) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Reorder { op, reply }, None)
            .await
    }

    /// Replace folders and display order using the same actor transaction as reorder.
    pub async fn set_layout(&self, layout: ProfileLayout) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetLayout { layout, reply },
            None,
        )
        .await
    }

    /// Create a folder around the given Config members in one transaction.
    pub async fn create_folder(
        &self,
        name: String,
        members: Vec<ProfileId>,
    ) -> Result<(CommitReport, FolderId), ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::CreateFolder {
                name,
                members,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn set_proxy_group_layout(
        &self,
        profile: ProfileId,
        layout: ProxyGroupLayout,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetProxyGroupLayout {
                profile,
                layout,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn create_proxy_group_folder(
        &self,
        profile: ProfileId,
        name: String,
        members: Vec<String>,
    ) -> Result<(CommitReport, FolderId), ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::CreateProxyGroupFolder {
                profile,
                name,
                members,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn patch_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::PatchMetadata { uid, patch, reply },
            None,
        )
        .await
    }

    pub async fn patch_remote_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::PatchRemoteOptions { uid, patch, reply },
            None,
        )
        .await
    }

    pub async fn refresh(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<CommitReport, ProfilesError> {
        self.profile_sync_key(&uid)?;
        self.inner.jobs.sync(uid, patch).await
    }

    /// Releases scheduled refreshes, external-file watchers and journal
    /// recovery after core startup reconciliation has completed.
    pub async fn start_producers(&self) -> Result<(), ProfilesError> {
        self.call(|reply| ProfilesActorMessage::StartProducers { reply }, None)
            .await
    }

    /// Downloads a profile URL and suggests how to import it. Nothing is
    /// written, so this does not go through the actor.
    // The desktop IPC is introduced with the generated frontend contract.
    #[allow(dead_code)]
    pub async fn probe(
        &self,
        url: url::Url,
        option: RemoteProfileOptions,
    ) -> Result<ProfileUrlProbe, ProfilesError> {
        let fetched = self
            .inner
            .fetcher
            .fetch(&url, &option)
            .await
            .map_err(|error| match error.downcast::<SubscriptionFetchError>() {
                Ok(source) => ProfilesError::FetchSubscription {
                    url: url.clone(),
                    source,
                },
                Err(error) => ProfilesError::ImportFailed {
                    message: error.to_string(),
                },
            })?;
        Ok(profile_import::probe_fetched(url, option, fetched))
    }

    /// Fetch-before-commit remote import. No durable placeholder is written until
    /// download + validation succeed and the caller is still awaiting the result.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn import(
        &self,
        url: url::Url,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
    ) -> Result<CommitReport, ProfilesError> {
        self.import_into_folder(url, metadata, option, update_interval_explicit, None)
            .await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn import_into_folder(
        &self,
        url: url::Url,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
        folder: Option<FolderId>,
    ) -> Result<CommitReport, ProfilesError> {
        self.import_with_transform(
            url,
            None,
            metadata,
            option,
            update_interval_explicit,
            folder,
        )
        .await
    }

    /// The ref import command carries the confirmed kind through the single source actor.
    pub async fn import_with_transform(
        &self,
        url: url::Url,
        transform: Option<TransformKind>,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
        folder: Option<FolderId>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::ImportRemote {
                url,
                transform,
                metadata,
                option,
                update_interval_explicit,
                folder,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn replace_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::ReplaceDefinition {
                uid,
                definition,
                reply,
            },
            None,
        )
        .await
    }

    async fn call<F, T>(&self, make: F, timeout: Option<Duration>) -> Result<T, ProfilesError>
    where
        F: FnOnce(RpcReplyPort<Result<T, ProfilesError>>) -> ProfilesActorMessage,
        T: Send + 'static,
    {
        match self.inner.actor_ref.call(make, timeout).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) => Err(ProfilesError::Rpc("reply dropped".into())),
            Ok(CallResult::Timeout) => Err(ProfilesError::Rpc("call timed out".into())),
            Err(e) => Err(ProfilesError::Rpc(e.to_string())),
        }
    }
}

impl Drop for ProfilesClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

use chimera_config::profile::ProfileSource;
use chimera_jobs::JobsClient;
pub const DEFAULT_HISTORY_LIMIT: usize = 10;
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

impl ProfilesClient {
    pub fn profile_sync_key(
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

    pub async fn profile_sync_status(
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

    pub async fn profile_sync_runs(
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

    pub async fn profile_sync_logs(
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn jobs(&self) -> JobsClient {
        self.inner.jobs.client.clone()
    }
}

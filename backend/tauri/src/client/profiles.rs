//! Typed client for the ProfilesActor. Committed reads come from the state
//! snapshot handle; writes go through the actor with no RPC timeout.

use crate::state::mutation::MutationCoordinator;

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use chimera_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch, Profiles,
    RemoteProfileOptions, RemoteProfileOptionsPatch,
};
use chimera_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::{
    core::migration::modules::profiles::ProfilesFormat,
    state::profiles::{
        CommitReport, NewProfileRequest, ProfilesActor, ProfilesActorArgs, ProfilesActorMessage,
        ProfilesError, RefreshOrigin, ReorderOp,
        ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
    },
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
}

impl ProfilesClient {
    pub(crate) async fn new(
        mutations: MutationCoordinator,
        profiles_path: Utf8PathBuf,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
    ) -> anyhow::Result<Self> {
        let should_load = profiles_path.exists();
        let setup = PersistentStateManagerSetup::<Profiles, ProfilesFormat>::builder()
            .config_path(profiles_path)
            .assemble();
        let manager = if should_load {
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

        let snapshot = manager.snapshot_handle();
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
                manager,
                fs,
                fetcher,
                materialization,
            },
        )
        .await
        .context("failed to spawn profiles actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ProfilesClientInner {
                actor_ref,
                snapshot,
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
    pub(crate) fn snapshot_handle(&self) -> StateSnapshot<Profiles> {
        self.inner.snapshot.clone()
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
        self.call(
            |reply| ProfilesActorMessage::RefreshRemote {
                uid,
                patch,
                origin: RefreshOrigin::Manual,
                reply: Some(reply),
            },
            None,
        )
        .await
    }

    /// Fetch-before-commit remote import. No durable placeholder is written until
    /// download + validation succeed and the caller is still awaiting the result.
    pub async fn import(
        &self,
        url: url::Url,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::ImportRemote {
                url,
                metadata,
                option,
                update_interval_explicit,
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

//! Profile persistence ports used by the application client during the staged migration.

use std::{collections::HashSet, io::Write, panic::AssertUnwindSafe, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use atomicwrites::{AtomicFile, OverwriteBehavior};
use futures::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::{ChimeraClient, Degradation, DegradationPhase, MutationOutcome};

use crate::config::profile::{
    builder::ProfileBuilder,
    item::{
        Profile, ProfileKindGetter, ProfileMetaGetter,
        remote::{
            PreparedSubscriptionUpdate, RemoteProfile, RemoteProfileBuilder,
            RemoteProfileImportMode, RemoteProfileOptions, RemoteProfileOptionsBuilder,
            SubscriptionInfo,
        },
        shared::{PreparedProfileFile, ProfileSharedBuilder},
        utils::generate_uid,
    },
    item_type::{ProfileItemType, ProfileUid},
    profiles::Profiles,
};

pub(crate) trait ProfilesReadPort: Send + Sync {
    fn snapshot(&self) -> anyhow::Result<Profiles>;
}

#[async_trait]
pub(crate) trait ProfileFsPort: Send + Sync {
    async fn resolve_path(&self, file: &str) -> anyhow::Result<std::path::PathBuf>;
    async fn read(&self, file: &str) -> anyhow::Result<String>;
    async fn write_atomic(&self, file: &str, content: &str) -> anyhow::Result<()>;
    async fn remove(&self, file: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait SubscriptionFetcher: Send + Sync {
    async fn fetch(&self, profile: RemoteProfile) -> anyhow::Result<PreparedSubscriptionUpdate>;
}

pub(crate) struct LegacySubscriptionFetcher;

#[async_trait]
impl SubscriptionFetcher for LegacySubscriptionFetcher {
    async fn fetch(&self, profile: RemoteProfile) -> anyhow::Result<PreparedSubscriptionUpdate> {
        profile.prepare_subscription_update(None).await
    }
}

#[async_trait]
pub(crate) trait RemoteProfileImporter: Send + Sync {
    async fn prepare(
        &self,
        uid: ProfileUid,
        url: url::Url,
        name: Option<String>,
        option: Option<RemoteProfileOptionsBuilder>,
        mode: RemoteProfileImportMode,
    ) -> anyhow::Result<(RemoteProfile, String)>;
}

pub(crate) struct LegacyRemoteProfileImporter;

#[async_trait]
impl RemoteProfileImporter for LegacyRemoteProfileImporter {
    async fn prepare(
        &self,
        uid: ProfileUid,
        url: url::Url,
        name: Option<String>,
        option: Option<RemoteProfileOptionsBuilder>,
        mode: RemoteProfileImportMode,
    ) -> anyhow::Result<(RemoteProfile, String)> {
        let mut builder = RemoteProfileBuilder::default();
        builder.assign_managed_identity(uid);
        builder.url(url);
        if let Some(name) = name {
            builder.set_name(name);
        }
        if let Some(option) = option {
            builder.option(option);
        }
        builder
            .build_prepared_with_mode(mode)
            .await
            .map(|prepared| prepared.into_parts())
            .context("failed to build a remote profile")
    }
}

#[async_trait]
pub(crate) trait ProfilesWritePort: Send + Sync {
    async fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)>;

    async fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)>;

    async fn patch_profile(&self, uid: &ProfileUid, profile: ProfileBuilder) -> anyhow::Result<()>;

    async fn patch_metadata(
        &self,
        uid: &ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<()>;

    async fn patch_remote_options(
        &self,
        uid: &ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<()>;

    async fn reorder(&self, active_id: &ProfileUid, over_id: &ProfileUid) -> anyhow::Result<()>;

    async fn reorder_by_list(&self, list: &[ProfileUid]) -> anyhow::Result<()>;

    async fn set_current(&self, uid: Option<&ProfileUid>) -> anyhow::Result<()>;

    async fn set_valid_fields(&self, fields: &[String]) -> anyhow::Result<()>;

    async fn set_profile_transform_chain(
        &self,
        uid: &ProfileUid,
        transforms: &[ProfileUid],
    ) -> anyhow::Result<bool>;

    async fn set_global_transform_chain(&self, transforms: &[ProfileUid]) -> anyhow::Result<bool>;

    async fn refresh_remote(
        &self,
        uid: &ProfileUid,
        options: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<bool>;

    async fn import_remote(
        &self,
        url: url::Url,
        name: Option<String>,
        option: Option<RemoteProfileOptionsBuilder>,
        mode: RemoteProfileImportMode,
    ) -> anyhow::Result<(ProfileUid, bool)>;

    async fn replace_remote_definition(
        &self,
        uid: &ProfileUid,
        file: &str,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
        transforms: &[ProfileUid],
    ) -> anyhow::Result<bool>;
}

pub(crate) struct LegacyProfileFsPort;

enum ProfilesActorMessage {
    Add {
        profile: Profile,
        reply: RpcReplyPort<anyhow::Result<(ProfileUid, bool)>>,
    },
    Delete {
        uid: ProfileUid,
        reply: RpcReplyPort<anyhow::Result<(String, bool)>>,
    },
    PatchProfile {
        uid: ProfileUid,
        profile: ProfileBuilder,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    PatchMetadata {
        uid: ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    PatchRemoteOptions {
        uid: ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Reorder {
        active_id: ProfileUid,
        over_id: ProfileUid,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    ReorderByList {
        list: Vec<ProfileUid>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    SetCurrent {
        uid: Option<ProfileUid>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    SetValidFields {
        fields: Vec<String>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    SetProfileTransformChain {
        uid: ProfileUid,
        transforms: Vec<ProfileUid>,
        reply: RpcReplyPort<anyhow::Result<bool>>,
    },
    SetGlobalTransformChain {
        transforms: Vec<ProfileUid>,
        reply: RpcReplyPort<anyhow::Result<bool>>,
    },
    RefreshRemote {
        uid: ProfileUid,
        options: Option<RemoteProfileOptionsBuilder>,
        reply: RpcReplyPort<anyhow::Result<bool>>,
    },
    CommitRemoteRefresh {
        uid: ProfileUid,
        expected_fingerprint: String,
        outcome: RemoteRefreshOutcome,
        reply: RpcReplyPort<anyhow::Result<bool>>,
    },
    ImportRemote {
        url: url::Url,
        name: Option<String>,
        option: Option<RemoteProfileOptionsBuilder>,
        mode: RemoteProfileImportMode,
        reply: RpcReplyPort<anyhow::Result<(ProfileUid, bool)>>,
    },
    CommitRemoteImport {
        prepared_file: PreparedProfileFile,
        outcome: RemoteImportOutcome,
        reply: RpcReplyPort<anyhow::Result<(ProfileUid, bool)>>,
    },
    ReplaceRemoteDefinition {
        uid: ProfileUid,
        file: String,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
        transforms: Vec<ProfileUid>,
        reply: RpcReplyPort<anyhow::Result<bool>>,
    },
    #[cfg(all(test, feature = "e2e"))]
    MutateForTest {
        mutation: Box<dyn FnOnce(&mut Profiles) -> anyhow::Result<()> + Send + 'static>,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
}

enum RemoteRefreshOutcome {
    Succeeded {
        previous_file: String,
        prepared: PreparedSubscriptionUpdate,
    },
    Failed(String),
}

enum RemoteImportOutcome {
    Succeeded {
        profile: RemoteProfile,
        content: String,
    },
    Failed(String),
}

struct ProfilesActorState {
    profiles: Profiles,
    snapshot_tx: tokio::sync::watch::Sender<Profiles>,
    profile_files: Arc<dyn ProfileFsPort>,
    fetcher: Arc<dyn SubscriptionFetcher>,
    importer: Arc<dyn RemoteProfileImporter>,
    pending_refreshes: HashSet<ProfileUid>,
}

struct ProfilesActor;

impl ProfilesActorState {
    fn publish(&mut self, next: Profiles) {
        self.profiles = next.clone();
        self.snapshot_tx.send_replace(next);
    }

    async fn mutate<T>(
        &mut self,
        mutation: impl FnOnce(&mut Profiles) -> anyhow::Result<T>,
    ) -> anyhow::Result<T>
    where
        T: Send,
    {
        let mut next = self.profiles.clone();
        let value = mutation(&mut next)?;
        let persisted = tokio::task::spawn_blocking({
            let next = next.clone();
            move || next.save_file()
        })
        .await
        .context("profile state persistence task failed")?;
        persisted?;
        self.publish(next);
        Ok(value)
    }

    fn remote_profile_fingerprint(profile: &RemoteProfile) -> anyhow::Result<String> {
        serde_yaml::to_string(&(
            &profile.url,
            &profile.option,
            &profile.shared.file,
            profile.shared.updated,
            &profile.chain,
            &profile.extra,
        ))
        .context("failed to fingerprint remote profile definition")
    }

    fn remote_profile(&self, uid: &ProfileUid) -> anyhow::Result<RemoteProfile> {
        let item = self.profiles.get_item(uid)?;
        item.as_remote()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("profile `{uid}` is not remote"))
    }

    fn reserve_remote_identity(&self) -> anyhow::Result<(ProfileUid, PreparedProfileFile)> {
        for uid in std::iter::repeat_with(|| generate_uid(&ProfileItemType::Remote))
            .take(PROFILE_IDENTITY_ATTEMPTS)
        {
            let file = ProfileSharedBuilder::default_file_name(&ProfileItemType::Remote, &uid);
            let collides_with_state = self
                .profiles
                .items
                .iter()
                .any(|profile| profile.uid() == uid || profile.file() == file);
            if collides_with_state {
                continue;
            }
            if let Some(prepared) = PreparedProfileFile::reserve(&file)? {
                return Ok((uid, prepared));
            }
        }
        anyhow::bail!("failed to reserve a unique managed profile identity")
    }

    async fn commit_remote_import(
        &mut self,
        mut prepared_file: PreparedProfileFile,
        profile: RemoteProfile,
        content: String,
    ) -> anyhow::Result<(ProfileUid, bool)> {
        let uid = profile.uid().to_string();
        let file = profile.shared.file.clone();
        self.profile_files.write_atomic(&file, &content).await?;
        prepared_file.mark_materialized();

        let activatable = profile.kind().is_config();
        let uid_for_state = uid.clone();
        let result = self
            .mutate(move |profiles| {
                let activate = activatable && profiles.current.is_empty();
                profiles.append_item(profile.into())?;
                if activate {
                    profiles.current = vec![uid_for_state.clone()];
                }
                Ok((uid_for_state, activate))
            })
            .await;
        match result {
            Ok(value) => {
                prepared_file.commit();
                Ok(value)
            }
            Err(error) => {
                if let Err(cleanup_error) = self.profile_files.remove(&file).await {
                    return Err(error.context(format!(
                        "failed to remove materialized profile after import commit failure: {cleanup_error:#}"
                    )));
                }
                Err(error)
            }
        }
    }

    async fn commit_remote_refresh(
        &mut self,
        uid: ProfileUid,
        expected_fingerprint: String,
        previous_file: String,
        prepared: PreparedSubscriptionUpdate,
    ) -> anyhow::Result<bool> {
        let current = self.remote_profile(&uid)?;
        let current_fingerprint = Self::remote_profile_fingerprint(&current)?;
        anyhow::ensure!(
            current_fingerprint == expected_fingerprint,
            "profile changed while refresh was in progress"
        );

        let mut updated = current.clone();
        let content = updated.apply_prepared_subscription_update(prepared)?;
        let file = current.shared.file.clone();
        self.profile_files.write_atomic(&file, &content).await?;
        let affects_current = self
            .profiles
            .current
            .iter()
            .any(|current_uid| current_uid == &uid);
        let result = self
            .mutate(move |profiles| {
                profiles.replace_item(&uid, updated.into())?;
                Ok(affects_current)
            })
            .await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Err(restore_error) =
                    self.profile_files.write_atomic(&file, &previous_file).await
                {
                    return Err(error.context(format!(
                        "failed to restore materialized profile after refresh commit failure: {restore_error:#}"
                    )));
                }
                Err(error)
            }
        }
    }
}

impl Actor for ProfilesActor {
    type Msg = ProfilesActorMessage;
    type State = ProfilesActorState;
    type Arguments = ProfilesActorState;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(state)
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ProfilesActorMessage::Add { profile, reply } => {
                let uid = profile.uid().to_string();
                let activatable = profile.kind().is_config();
                let result = state
                    .mutate(move |profiles| {
                        let activate = activatable && profiles.current.is_empty();
                        profiles.append_item(profile)?;
                        if activate {
                            profiles.current = vec![uid.clone()];
                        }
                        Ok((uid, activate))
                    })
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Delete { uid, reply } => {
                let result = state
                    .mutate(move |profiles| {
                        let file = profiles.get_item(&uid)?.file().to_string();
                        let affects_current = profiles.delete_item(&uid)?;
                        Ok((file, affects_current))
                    })
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchProfile {
                uid,
                profile,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| {
                        let current = profiles
                            .items
                            .iter_mut()
                            .find(|item| item.uid() == uid)
                            .ok_or_else(|| {
                                anyhow::anyhow!("failed to get the profile item `uid:{uid}`")
                            })?;
                        match (current, profile) {
                            (Profile::Remote(item), ProfileBuilder::Remote(builder)) => builder
                                .patch_profile(item)
                                .context("failed to patch remote profile")?,
                            (Profile::Local(item), ProfileBuilder::Local(builder)) => {
                                item.apply(builder)
                            }
                            (Profile::Merge(item), ProfileBuilder::Merge(builder)) => {
                                item.apply(builder)
                            }
                            (Profile::Script(item), ProfileBuilder::Script(builder)) => {
                                item.apply(builder)
                            }
                            _ => anyhow::bail!("profile type mismatch"),
                        }
                        Ok(())
                    })
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchMetadata {
                uid,
                name,
                desc,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| profiles.patch_metadata(&uid, name, desc))
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchRemoteOptions {
                uid,
                user_agent,
                with_proxy,
                self_proxy,
                update_interval_minutes,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| {
                        profiles.patch_remote_options(
                            &uid,
                            user_agent,
                            with_proxy,
                            self_proxy,
                            update_interval_minutes,
                        )
                    })
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Reorder {
                active_id,
                over_id,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| profiles.reorder(&active_id, &over_id))
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::ReorderByList { list, reply } => {
                let result = state
                    .mutate(move |profiles| profiles.reorder_by_list(&list))
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetCurrent { uid, reply } => {
                let result = state
                    .mutate(move |profiles| profiles.activate(uid.as_deref()))
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetValidFields { fields, reply } => {
                let result = state
                    .mutate(move |profiles| {
                        profiles.valid = fields;
                        Ok(())
                    })
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetProfileTransformChain {
                uid,
                transforms,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| profiles.set_profile_transform_chain(&uid, transforms))
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetGlobalTransformChain { transforms, reply } => {
                let result = state
                    .mutate(move |profiles| profiles.set_global_transform_chain(transforms))
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::RefreshRemote {
                uid,
                options,
                reply,
            } => {
                if state.pending_refreshes.contains(&uid) {
                    let _ = reply.send(Err(anyhow::anyhow!("profile refresh already in progress")));
                    return Ok(());
                }

                if let Some(options) = options {
                    let patched = state
                        .mutate({
                            let uid = uid.clone();
                            move |profiles| {
                                let item = profiles
                                    .items
                                    .iter_mut()
                                    .find(|item| item.uid() == uid)
                                    .ok_or_else(|| anyhow::anyhow!("profile `{uid}` not found"))?;
                                let Profile::Remote(profile) = item else {
                                    anyhow::bail!("profile `{uid}` is not remote");
                                };
                                profile.option.apply(options);
                                Ok(())
                            }
                        })
                        .await;
                    if let Err(error) = patched {
                        let _ = reply.send(Err(error));
                        return Ok(());
                    }
                }

                let remote = match state.remote_profile(&uid) {
                    Ok(remote) => remote,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return Ok(());
                    }
                };
                let expected_fingerprint =
                    match ProfilesActorState::remote_profile_fingerprint(&remote) {
                        Ok(fingerprint) => fingerprint,
                        Err(error) => {
                            let _ = reply.send(Err(error));
                            return Ok(());
                        }
                    };
                state.pending_refreshes.insert(uid.clone());
                let profile_files = state.profile_files.clone();
                let fetcher = state.fetcher.clone();
                let actor = myself.clone();
                tokio::spawn(async move {
                    let outcome = async {
                        let previous_file = profile_files.read(&remote.shared.file).await?;
                        let prepared = fetcher.fetch(remote).await?;
                        Ok::<_, anyhow::Error>((previous_file, prepared))
                    }
                    .await;
                    let outcome = match outcome {
                        Ok((previous_file, prepared)) => RemoteRefreshOutcome::Succeeded {
                            previous_file,
                            prepared,
                        },
                        Err(error) => RemoteRefreshOutcome::Failed(error.to_string()),
                    };
                    let _ = actor.cast(ProfilesActorMessage::CommitRemoteRefresh {
                        uid,
                        expected_fingerprint,
                        outcome,
                        reply,
                    });
                });
            }
            ProfilesActorMessage::CommitRemoteRefresh {
                uid,
                expected_fingerprint,
                outcome,
                reply,
            } => {
                state.pending_refreshes.remove(&uid);
                let result = match outcome {
                    RemoteRefreshOutcome::Succeeded {
                        previous_file,
                        prepared,
                    } => {
                        state
                            .commit_remote_refresh(
                                uid,
                                expected_fingerprint,
                                previous_file,
                                prepared,
                            )
                            .await
                    }
                    RemoteRefreshOutcome::Failed(message) => Err(anyhow::anyhow!(message)),
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::ImportRemote {
                url,
                name,
                option,
                mode,
                reply,
            } => {
                let (uid, prepared_file) = match state.reserve_remote_identity() {
                    Ok(reserved) => reserved,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return Ok(());
                    }
                };
                let importer = state.importer.clone();
                let actor = myself.clone();
                tokio::spawn(async move {
                    let outcome =
                        match AssertUnwindSafe(importer.prepare(uid, url, name, option, mode))
                            .catch_unwind()
                            .await
                        {
                            Ok(Ok((profile, content))) => {
                                RemoteImportOutcome::Succeeded { profile, content }
                            }
                            Ok(Err(error)) => RemoteImportOutcome::Failed(error.to_string()),
                            Err(_) => RemoteImportOutcome::Failed(
                                "remote profile import task panicked".to_string(),
                            ),
                        };
                    let _ = actor.cast(ProfilesActorMessage::CommitRemoteImport {
                        prepared_file,
                        outcome,
                        reply,
                    });
                });
            }
            ProfilesActorMessage::CommitRemoteImport {
                prepared_file,
                outcome,
                reply,
            } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let result = match outcome {
                    RemoteImportOutcome::Succeeded { profile, content } => {
                        state
                            .commit_remote_import(prepared_file, profile, content)
                            .await
                    }
                    RemoteImportOutcome::Failed(message) => Err(anyhow::anyhow!(message)),
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::ReplaceRemoteDefinition {
                uid,
                file,
                updated_at,
                url,
                option,
                subscription,
                transforms,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| {
                        profiles.replace_remote_definition_with_transforms(
                            &uid,
                            &file,
                            updated_at,
                            url,
                            option,
                            subscription,
                            transforms,
                        )
                    })
                    .await;
                let _ = reply.send(result);
            }
            #[cfg(all(test, feature = "e2e"))]
            ProfilesActorMessage::MutateForTest { mutation, reply } => {
                let result = state.mutate(mutation).await;
                let _ = reply.send(result);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct ProfilesClient {
    inner: Arc<ProfilesClientInner>,
}

struct ProfilesClientInner {
    actor_ref: ActorRef<ProfilesActorMessage>,
    snapshot_rx: tokio::sync::watch::Receiver<Profiles>,
}

impl Drop for ProfilesClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

impl ProfilesClient {
    pub(crate) async fn spawn(profile_files: Arc<dyn ProfileFsPort>) -> anyhow::Result<Self> {
        Self::spawn_from_profiles(
            Profiles::new(),
            profile_files,
            Arc::new(LegacySubscriptionFetcher),
            Arc::new(LegacyRemoteProfileImporter),
        )
        .await
    }

    async fn spawn_from_profiles(
        profiles: Profiles,
        profile_files: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        importer: Arc<dyn RemoteProfileImporter>,
    ) -> anyhow::Result<Self> {
        let (snapshot_tx, snapshot_rx) = tokio::sync::watch::channel(profiles.clone());
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorState {
                profiles,
                snapshot_tx,
                profile_files,
                fetcher,
                importer,
                pending_refreshes: HashSet::new(),
            },
        )
        .await
        .context("failed to spawn profiles actor")?;
        Ok(Self {
            inner: Arc::new(ProfilesClientInner {
                actor_ref,
                snapshot_rx,
            }),
        })
    }

    #[cfg(all(test, feature = "e2e"))]
    async fn spawn_with_profiles(profiles: Profiles) -> anyhow::Result<Self> {
        Self::spawn_from_profiles(
            profiles,
            Arc::new(LegacyProfileFsPort),
            Arc::new(LegacySubscriptionFetcher),
            Arc::new(LegacyRemoteProfileImporter),
        )
        .await
    }

    async fn call<T>(
        &self,
        make: impl FnOnce(RpcReplyPort<anyhow::Result<T>>) -> ProfilesActorMessage,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
    {
        match self.inner.actor_ref.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) => anyhow::bail!("profiles actor reply dropped"),
            Ok(CallResult::Timeout) => anyhow::bail!("profiles actor call timed out"),
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(all(test, feature = "e2e"))]
    async fn mutate_for_test(
        &self,
        mutation: impl FnOnce(&mut Profiles) -> anyhow::Result<()> + Send + 'static,
    ) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::MutateForTest {
            mutation: Box::new(mutation),
            reply,
        })
        .await
    }
}

impl ProfilesReadPort for ProfilesClient {
    fn snapshot(&self) -> anyhow::Result<Profiles> {
        Ok(self.inner.snapshot_rx.borrow().clone())
    }
}

#[async_trait]
impl ProfileFsPort for LegacyProfileFsPort {
    async fn resolve_path(&self, file: &str) -> anyhow::Result<std::path::PathBuf> {
        let file = file.to_string();
        tokio::task::spawn_blocking(move || {
            crate::config::profile::item::utils::resolve_managed_profile_path(&file)
        })
        .await
        .context("profile path resolution task failed")?
    }

    async fn read(&self, file: &str) -> anyhow::Result<String> {
        let file = file.to_string();
        tokio::task::spawn_blocking(move || {
            let path = crate::config::profile::item::utils::resolve_managed_profile_path(&file)?;
            std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read profile file {}", path.display()))
        })
        .await
        .context("profile file read task failed")?
    }

    async fn write_atomic(&self, file: &str, content: &str) -> anyhow::Result<()> {
        let file = file.to_string();
        let content = content.to_string();
        tokio::task::spawn_blocking(move || {
            let path = crate::config::profile::item::utils::resolve_managed_profile_path(&file)?;
            AtomicFile::new(&path, OverwriteBehavior::AllowOverwrite)
                .write(|target| target.write_all(content.as_bytes()))
                .with_context(|| {
                    format!("failed to atomically save profile file {}", path.display())
                })
        })
        .await
        .context("profile file write task failed")?
    }

    async fn remove(&self, file: &str) -> anyhow::Result<()> {
        let file = file.to_string();
        tokio::task::spawn_blocking(move || {
            let path = crate::config::profile::item::utils::resolve_managed_profile_path(&file)?;
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error)
                    .with_context(|| format!("failed to remove profile file {}", path.display())),
            }
        })
        .await
        .context("profile file removal task failed")?
    }
}

#[async_trait]
impl ProfilesWritePort for ProfilesClient {
    async fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)> {
        self.call(|reply| ProfilesActorMessage::Add { profile, reply })
            .await
    }

    async fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)> {
        self.call(|reply| ProfilesActorMessage::Delete {
            uid: uid.clone(),
            reply,
        })
        .await
    }

    async fn patch_profile(&self, uid: &ProfileUid, profile: ProfileBuilder) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::PatchProfile {
            uid: uid.clone(),
            profile,
            reply,
        })
        .await
    }

    async fn patch_metadata(
        &self,
        uid: &ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::PatchMetadata {
            uid: uid.clone(),
            name,
            desc,
            reply,
        })
        .await
    }

    async fn patch_remote_options(
        &self,
        uid: &ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::PatchRemoteOptions {
            uid: uid.clone(),
            user_agent,
            with_proxy,
            self_proxy,
            update_interval_minutes,
            reply,
        })
        .await
    }

    async fn reorder(&self, active_id: &ProfileUid, over_id: &ProfileUid) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::Reorder {
            active_id: active_id.clone(),
            over_id: over_id.clone(),
            reply,
        })
        .await
    }

    async fn reorder_by_list(&self, list: &[ProfileUid]) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::ReorderByList {
            list: list.to_vec(),
            reply,
        })
        .await
    }

    async fn set_current(&self, uid: Option<&ProfileUid>) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::SetCurrent {
            uid: uid.cloned(),
            reply,
        })
        .await
    }

    async fn set_valid_fields(&self, fields: &[String]) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::SetValidFields {
            fields: fields.to_vec(),
            reply,
        })
        .await
    }

    async fn set_profile_transform_chain(
        &self,
        uid: &ProfileUid,
        transforms: &[ProfileUid],
    ) -> anyhow::Result<bool> {
        self.call(|reply| ProfilesActorMessage::SetProfileTransformChain {
            uid: uid.clone(),
            transforms: transforms.to_vec(),
            reply,
        })
        .await
    }

    async fn set_global_transform_chain(&self, transforms: &[ProfileUid]) -> anyhow::Result<bool> {
        self.call(|reply| ProfilesActorMessage::SetGlobalTransformChain {
            transforms: transforms.to_vec(),
            reply,
        })
        .await
    }

    async fn refresh_remote(
        &self,
        uid: &ProfileUid,
        options: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<bool> {
        self.call(|reply| ProfilesActorMessage::RefreshRemote {
            uid: uid.clone(),
            options,
            reply,
        })
        .await
    }

    async fn import_remote(
        &self,
        url: url::Url,
        name: Option<String>,
        option: Option<RemoteProfileOptionsBuilder>,
        mode: RemoteProfileImportMode,
    ) -> anyhow::Result<(ProfileUid, bool)> {
        self.call(|reply| ProfilesActorMessage::ImportRemote {
            url,
            name,
            option,
            mode,
            reply,
        })
        .await
    }

    async fn replace_remote_definition(
        &self,
        uid: &ProfileUid,
        file: &str,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
        transforms: &[ProfileUid],
    ) -> anyhow::Result<bool> {
        self.call(|reply| ProfilesActorMessage::ReplaceRemoteDefinition {
            uid: uid.clone(),
            file: file.to_string(),
            updated_at,
            url,
            option,
            subscription,
            transforms: transforms.to_vec(),
            reply,
        })
        .await
    }
}

const PROFILE_IDENTITY_ATTEMPTS: usize = 32;

impl ChimeraClient {
    pub(crate) async fn get_profiles(&self) -> anyhow::Result<Profiles> {
        self.inner.profiles.snapshot()
    }

    pub(crate) fn reserve_managed_profile_identity(
        &self,
        kind: &ProfileItemType,
    ) -> anyhow::Result<(ProfileUid, PreparedProfileFile)> {
        let profiles = self.inner.profiles.snapshot()?;
        for uid in std::iter::repeat_with(|| generate_uid(kind)).take(PROFILE_IDENTITY_ATTEMPTS) {
            let file = ProfileSharedBuilder::default_file_name(kind, &uid);
            let collides_with_state = profiles
                .items
                .iter()
                .any(|profile| profile.uid() == uid || profile.file() == file);
            if collides_with_state {
                continue;
            }
            if let Some(prepared) = PreparedProfileFile::reserve(&file)? {
                return Ok((uid, prepared));
            }
        }
        anyhow::bail!("failed to reserve a unique managed profile identity")
    }

    pub(crate) async fn get_profile_materialized_path(
        &self,
        uid: ProfileUid,
    ) -> anyhow::Result<std::path::PathBuf> {
        let profiles = self.inner.profiles.snapshot()?;
        let item = profiles.get_item(&uid)?;
        self.inner.profile_files.resolve_path(item.file()).await
    }

    pub(crate) async fn read_profile_file(&self, uid: ProfileUid) -> anyhow::Result<String> {
        let profiles = self.inner.profiles.snapshot()?;
        let item = profiles.get_item(&uid)?;
        let raw = self.inner.profile_files.read(item.file()).await?;
        if matches!(item.kind(), ProfileItemType::Script(_)) {
            return Ok(raw);
        }
        let data = serde_yaml::from_str::<serde_yaml::Mapping>(&raw)?;
        serde_yaml::to_string(&data).context("failed to convert yaml to string")
    }

    pub(crate) async fn import_remote_profile(
        &self,
        url: url::Url,
        name: Option<String>,
        option: Option<RemoteProfileOptionsBuilder>,
        mode: RemoteProfileImportMode,
    ) -> anyhow::Result<MutationOutcome<ProfileUid>> {
        let (uid, activate) = self
            .inner
            .profile_writes
            .import_remote(url, name, option, mode)
            .await?;
        self.inner.ui_sink.refresh_profiles();
        let mut outcome = MutationOutcome::from_parts(uid, Vec::new());
        if activate {
            let runtime = self
                .after_profile_runtime_commit("remote profile import")
                .await;
            outcome = outcome.extend_degradations(runtime.degradations().to_vec());
        }
        Ok(outcome)
    }

    pub(crate) async fn commit_new_profile(
        &self,
        profile: Profile,
        mut prepared_file: PreparedProfileFile,
        materialized_content: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileUid>> {
        if let Some(content) = materialized_content {
            self.inner
                .profile_files
                .write_atomic(profile.file(), &content)
                .await?;
            prepared_file.mark_materialized();
        }

        let (uid, activate) = {
            let _commit = self.inner.profile_commit.lock().await;
            let result = self.inner.profile_writes.add(profile).await?;
            self.inner.ui_sink.refresh_profiles();
            result
        };
        let mut outcome = MutationOutcome::from_parts(uid, Vec::new());
        if activate {
            let runtime = self.after_profile_runtime_commit("profile creation").await;
            outcome = outcome.extend_degradations(runtime.degradations().to_vec());
        }
        prepared_file.commit();
        Ok(outcome)
    }

    pub(crate) async fn patch_profile(
        &self,
        uid: ProfileUid,
        profile: ProfileBuilder,
    ) -> anyhow::Result<MutationOutcome<()>> {
        {
            let _commit = self.inner.profile_commit.lock().await;
            self.inner
                .profile_writes
                .patch_profile(&uid, profile)
                .await?;
            self.inner.ui_sink.refresh_profiles();
        }
        Ok(self.after_profile_runtime_commit("profile patch").await)
    }

    pub(crate) async fn patch_profile_metadata(
        &self,
        uid: ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _commit = self.inner.profile_commit.lock().await;
        self.inner
            .profile_writes
            .patch_metadata(&uid, name, desc)
            .await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    pub(crate) async fn patch_remote_profile_options(
        &self,
        uid: ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _commit = self.inner.profile_commit.lock().await;
        self.inner
            .profile_writes
            .patch_remote_options(
                &uid,
                user_agent,
                with_proxy,
                self_proxy,
                update_interval_minutes,
            )
            .await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    pub(crate) async fn refresh_profile(
        &self,
        uid: ProfileUid,
        options: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let patched_options = options.is_some();
        let result = self
            .inner
            .profile_writes
            .refresh_remote(&uid, options)
            .await;
        if patched_options || result.is_ok() {
            self.inner.ui_sink.refresh_profiles();
        }
        let affects_current = result?;
        if affects_current {
            Ok(self
                .after_profile_runtime_commit("remote profile refresh")
                .await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    pub(crate) async fn replace_remote_profile_definition(
        &self,
        uid: ProfileUid,
        file: String,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
        transforms: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let affects_current = {
            let _commit = self.inner.profile_commit.lock().await;
            let affects_current = self
                .inner
                .profile_writes
                .replace_remote_definition(
                    &uid,
                    &file,
                    updated_at,
                    url,
                    option,
                    subscription,
                    &transforms,
                )
                .await?;
            self.inner.ui_sink.refresh_profiles();
            affects_current
        };
        if affects_current {
            Ok(self
                .after_profile_runtime_commit("profile definition replacement")
                .await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    pub(crate) async fn delete_profile(
        &self,
        uid: ProfileUid,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let (file, affects_current) = {
            let _commit = self.inner.profile_commit.lock().await;
            let result = self.inner.profile_writes.delete(&uid).await?;
            self.inner.ui_sink.refresh_profiles();
            result
        };
        let mut degradations = Vec::new();
        if let Err(error) = self.inner.profile_files.remove(&file).await {
            degradations.push(Degradation {
                phase: DegradationPhase::ProfileMaterialization,
                code: "cleanup_deferred".into(),
                message: error.to_string(),
                retryable: true,
            });
        }
        if affects_current {
            degradations.extend(
                self.after_profile_runtime_commit("profile deletion")
                    .await
                    .degradations()
                    .iter()
                    .cloned(),
            );
        }
        Ok(MutationOutcome::from_parts((), degradations))
    }

    pub(crate) async fn reorder_profile(
        &self,
        active_id: ProfileUid,
        over_id: ProfileUid,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _commit = self.inner.profile_commit.lock().await;
        self.inner
            .profile_writes
            .reorder(&active_id, &over_id)
            .await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    pub(crate) async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _commit = self.inner.profile_commit.lock().await;
        self.inner.profile_writes.reorder_by_list(&list).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    pub(super) async fn after_profile_runtime_commit(
        &self,
        operation: &str,
    ) -> MutationOutcome<()> {
        match self.rebuild_profile_runtime().await {
            Ok(()) => MutationOutcome::from_parts((), Vec::new()),
            Err(error) => {
                log::warn!(target: "app", "post-commit rebuild failed after {operation}; state stays committed: {error:?}");
                MutationOutcome::from_parts(
                    (),
                    vec![Degradation {
                        phase: DegradationPhase::RuntimeBuild,
                        code: "runtime_rebuild_failed".into(),
                        message: error.to_string(),
                        retryable: true,
                    }],
                )
            }
        }
    }

    pub(crate) async fn activate_profile(
        &self,
        uid: Option<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        {
            let _commit = self.inner.profile_commit.lock().await;
            self.inner.profile_writes.set_current(uid.as_ref()).await?;
            self.inner.ui_sink.refresh_profiles();
        }
        Ok(self
            .after_profile_runtime_commit("profile activation")
            .await)
    }

    pub(crate) async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        {
            let _commit = self.inner.profile_commit.lock().await;
            self.inner.profile_writes.set_valid_fields(&fields).await?;
            self.inner.ui_sink.refresh_profiles();
        }
        Ok(self
            .after_profile_runtime_commit("profile valid fields update")
            .await)
    }

    pub(crate) async fn set_profile_transform_chain(
        &self,
        uid: ProfileUid,
        transforms: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let affects_current = {
            let _commit = self.inner.profile_commit.lock().await;
            let affects_current = self
                .inner
                .profile_writes
                .set_profile_transform_chain(&uid, &transforms)
                .await?;
            self.inner.ui_sink.refresh_profiles();
            affects_current
        };
        if affects_current {
            Ok(self
                .after_profile_runtime_commit("profile transform chain update")
                .await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    pub(crate) async fn set_global_transform_chain(
        &self,
        transforms: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let changed = {
            let _commit = self.inner.profile_commit.lock().await;
            let changed = self
                .inner
                .profile_writes
                .set_global_transform_chain(&transforms)
                .await?;
            self.inner.ui_sink.refresh_profiles();
            changed
        };
        if changed {
            Ok(self
                .after_profile_runtime_commit("global transform chain update")
                .await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    pub(crate) async fn save_profile_file(
        &self,
        uid: ProfileUid,
        file_data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let affects_current = {
            let _commit = self.inner.profile_commit.lock().await;
            let profiles = self.inner.profiles.snapshot()?;
            let item = profiles.get_item(&uid)?;
            let kind = item.kind();
            anyhow::ensure!(
                !matches!(kind, ProfileItemType::Remote),
                "remote profiles are updater-owned"
            );
            if !matches!(kind, ProfileItemType::Script(_)) {
                serde_yaml::from_str::<serde_yaml::Mapping>(&file_data)
                    .context("failed to parse profile YAML")?;
            }
            self.inner
                .profile_files
                .write_atomic(item.file(), &file_data)
                .await?;
            profiles.is_runtime_relevant(&uid)
        };
        if affects_current {
            Ok(self.after_profile_runtime_commit("profile file save").await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    async fn rebuild_profile_runtime(&self) -> anyhow::Result<()> {
        let break_when = self.get_clash_config()?.break_connection.on_profile_change;
        self.rebuild_running_config().await?;
        self.inner.core.on_profile_change(break_when).await;
        Ok(())
    }
}

#[cfg(all(test, feature = "e2e"))]
mod actor_tests {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use super::{
        LegacyRemoteProfileImporter, ProfileFsPort, ProfilesClient, ProfilesReadPort,
        ProfilesWritePort, RemoteProfileImporter, SubscriptionFetcher,
    };
    use crate::config::profile::{
        item::{
            Profile,
            remote::{
                PreparedSubscriptionUpdate, RemoteProfile, RemoteProfileImportMode,
                RemoteProfileOptions, RemoteProfileOptionsBuilder, SubscriptionInfo,
            },
            shared::ProfileShared,
        },
        item_type::ProfileUid,
        profiles::Profiles,
    };

    static CONFIG_DIR_LOCK: Mutex<()> = Mutex::new(());

    fn remote_profile() -> RemoteProfile {
        RemoteProfile {
            url: url::Url::parse("https://example.com/profile.yaml").unwrap(),
            option: RemoteProfileOptions::default(),
            shared: ProfileShared {
                uid: "r-test".into(),
                name: "Test".into(),
                file: "r-test.yaml".into(),
                desc: None,
                updated: 7,
            },
            chain: Vec::new(),
            extra: SubscriptionInfo::default(),
        }
    }

    fn prepared_update() -> PreparedSubscriptionUpdate {
        let mut data = serde_yaml::Mapping::new();
        data.insert("mode".into(), "global".into());
        PreparedSubscriptionUpdate::for_test(
            data,
            SubscriptionInfo {
                upload: 10,
                download: 20,
                total: 30,
                expire: 40,
            },
        )
    }

    #[derive(Default)]
    struct MemoryProfileFs {
        files: Mutex<HashMap<String, String>>,
        writes: Mutex<Vec<(String, String)>>,
        fail_write: AtomicBool,
    }

    #[async_trait::async_trait]
    impl ProfileFsPort for MemoryProfileFs {
        async fn resolve_path(&self, file: &str) -> anyhow::Result<PathBuf> {
            Ok(PathBuf::from(file))
        }

        async fn read(&self, file: &str) -> anyhow::Result<String> {
            self.files
                .lock()
                .unwrap()
                .get(file)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing profile file {file}"))
        }

        async fn write_atomic(&self, file: &str, content: &str) -> anyhow::Result<()> {
            self.writes
                .lock()
                .unwrap()
                .push((file.to_string(), content.to_string()));
            if self.fail_write.load(Ordering::Acquire) {
                anyhow::bail!("injected profile file write failure");
            }
            self.files
                .lock()
                .unwrap()
                .insert(file.to_string(), content.to_string());
            Ok(())
        }

        async fn remove(&self, file: &str) -> anyhow::Result<()> {
            self.files.lock().unwrap().remove(file);
            Ok(())
        }
    }

    struct ImmediateFetcher;

    struct ImmediateImporter;

    #[async_trait::async_trait]
    impl RemoteProfileImporter for ImmediateImporter {
        async fn prepare(
            &self,
            uid: ProfileUid,
            url: url::Url,
            name: Option<String>,
            _option: Option<RemoteProfileOptionsBuilder>,
            _mode: RemoteProfileImportMode,
        ) -> anyhow::Result<(RemoteProfile, String)> {
            let mut profile = remote_profile();
            profile.shared.uid = uid.clone();
            profile.shared.file = format!("{uid}.yaml");
            profile.shared.name = name.unwrap_or_else(|| "Imported".to_string());
            profile.url = url;
            Ok((profile, "mode: direct\n".to_string()))
        }
    }

    struct FailingImporter;

    #[async_trait::async_trait]
    impl RemoteProfileImporter for FailingImporter {
        async fn prepare(
            &self,
            _uid: ProfileUid,
            _url: url::Url,
            _name: Option<String>,
            _option: Option<RemoteProfileOptionsBuilder>,
            _mode: RemoteProfileImportMode,
        ) -> anyhow::Result<(RemoteProfile, String)> {
            anyhow::bail!("injected remote import failure")
        }
    }

    struct BlockingImporter {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl RemoteProfileImporter for BlockingImporter {
        async fn prepare(
            &self,
            uid: ProfileUid,
            url: url::Url,
            name: Option<String>,
            option: Option<RemoteProfileOptionsBuilder>,
            mode: RemoteProfileImportMode,
        ) -> anyhow::Result<(RemoteProfile, String)> {
            self.started.notify_one();
            self.release.notified().await;
            ImmediateImporter
                .prepare(uid, url, name, option, mode)
                .await
        }
    }

    #[async_trait::async_trait]
    impl SubscriptionFetcher for ImmediateFetcher {
        async fn fetch(
            &self,
            _profile: RemoteProfile,
        ) -> anyhow::Result<PreparedSubscriptionUpdate> {
            Ok(prepared_update())
        }
    }

    struct BlockingFetcher {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl SubscriptionFetcher for BlockingFetcher {
        async fn fetch(
            &self,
            _profile: RemoteProfile,
        ) -> anyhow::Result<PreparedSubscriptionUpdate> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(prepared_update())
        }
    }

    fn profiles_with_remote() -> Profiles {
        Profiles {
            items: vec![Profile::Remote(remote_profile())],
            ..Profiles::default()
        }
    }

    #[tokio::test]
    async fn duplicate_remote_refresh_is_rejected_while_fetch_is_in_flight() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let fs = Arc::new(MemoryProfileFs::default());
        fs.files
            .lock()
            .unwrap()
            .insert("r-test.yaml".into(), "mode: rule\n".into());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let client = ProfilesClient::spawn_from_profiles(
            profiles_with_remote(),
            fs,
            Arc::new(BlockingFetcher {
                started: started.clone(),
                release: release.clone(),
            }),
            Arc::new(LegacyRemoteProfileImporter),
        )
        .await
        .expect("profiles actor should spawn");

        let first = {
            let client = client.clone();
            tokio::spawn(async move { client.refresh_remote(&"r-test".to_string(), None).await })
        };
        started.notified().await;

        let error = client
            .refresh_remote(&"r-test".to_string(), None)
            .await
            .expect_err("duplicate refresh should be rejected");
        assert!(error.to_string().contains("already in progress"));

        release.notify_one();
        first
            .await
            .expect("first refresh task should complete")
            .expect("first refresh should commit");

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn remote_refresh_rejects_stale_download_after_definition_change() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let fs = Arc::new(MemoryProfileFs::default());
        fs.files
            .lock()
            .unwrap()
            .insert("r-test.yaml".into(), "mode: rule\n".into());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let client = ProfilesClient::spawn_from_profiles(
            profiles_with_remote(),
            fs.clone(),
            Arc::new(BlockingFetcher {
                started: started.clone(),
                release: release.clone(),
            }),
            Arc::new(LegacyRemoteProfileImporter),
        )
        .await
        .expect("profiles actor should spawn");

        let refresh = {
            let client = client.clone();
            tokio::spawn(async move { client.refresh_remote(&"r-test".to_string(), None).await })
        };
        started.notified().await;

        client
            .patch_remote_options(&"r-test".to_string(), None, Some(true), None, None)
            .await
            .expect("definition mutation should commit while fetch is in flight");
        release.notify_one();

        let error = refresh
            .await
            .expect("refresh task should complete")
            .expect_err("stale download must be fenced");
        assert!(
            error
                .to_string()
                .contains("profile changed while refresh was in progress")
        );
        assert_eq!(
            fs.files
                .lock()
                .unwrap()
                .get("r-test.yaml")
                .map(String::as_str),
            Some("mode: rule\n")
        );

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn remote_refresh_file_write_failure_does_not_commit_profile_state() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let fs = Arc::new(MemoryProfileFs::default());
        fs.files
            .lock()
            .unwrap()
            .insert("r-test.yaml".into(), "mode: rule\n".into());
        fs.fail_write.store(true, Ordering::Release);
        let client = ProfilesClient::spawn_from_profiles(
            profiles_with_remote(),
            fs,
            Arc::new(ImmediateFetcher),
            Arc::new(LegacyRemoteProfileImporter),
        )
        .await
        .expect("profiles actor should spawn");

        let error = client
            .refresh_remote(&"r-test".to_string(), None)
            .await
            .expect_err("materialized file write should fail");
        assert!(
            error
                .to_string()
                .contains("injected profile file write failure")
        );

        let snapshot = client.snapshot().expect("snapshot should remain readable");
        let remote = snapshot
            .get_item(&"r-test".to_string())
            .expect("remote profile should remain")
            .as_remote()
            .expect("profile should remain remote");
        assert_eq!(remote.shared.updated, 7);
        assert_eq!(remote.extra.upload, 0);
        assert_eq!(remote.extra.download, 0);

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn remote_refresh_state_persist_failure_restores_materialized_file() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }
        std::fs::create_dir(config_dir.path().join("profiles.yaml"))
            .expect("profiles.yaml directory should force persistence failure");

        let fs = Arc::new(MemoryProfileFs::default());
        fs.files
            .lock()
            .unwrap()
            .insert("r-test.yaml".into(), "mode: rule\n".into());
        let client = ProfilesClient::spawn_from_profiles(
            profiles_with_remote(),
            fs.clone(),
            Arc::new(ImmediateFetcher),
            Arc::new(LegacyRemoteProfileImporter),
        )
        .await
        .expect("profiles actor should spawn");

        client
            .refresh_remote(&"r-test".to_string(), None)
            .await
            .expect_err("state persistence should fail");

        let writes = fs.writes.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert!(writes[0].1.contains("mode: global"));
        assert_eq!(
            writes[1],
            ("r-test.yaml".to_string(), "mode: rule\n".to_string())
        );
        drop(writes);
        assert_eq!(
            fs.files
                .lock()
                .unwrap()
                .get("r-test.yaml")
                .map(String::as_str),
            Some("mode: rule\n")
        );

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn concurrent_mutations_are_serialized_without_lost_updates() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let client = ProfilesClient::spawn_with_profiles(Profiles::default())
            .await
            .expect("profiles actor should spawn");
        let first = client.mutate_for_test(|profiles| {
            profiles.valid.push("first".to_string());
            Ok(())
        });
        let second = client.mutate_for_test(|profiles| {
            profiles.valid.push("second".to_string());
            Ok(())
        });
        let (first, second) = tokio::join!(first, second);
        first.expect("first mutation should commit");
        second.expect("second mutation should commit");

        let snapshot = client.snapshot().expect("snapshot should be readable");
        assert!(
            snapshot
                .valid
                .ends_with(&["first".to_string(), "second".to_string()])
        );
        let persisted: Profiles = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.path().join("profiles.yaml"))
                .expect("profiles.yaml should be persisted"),
        )
        .expect("persisted profiles should decode");
        assert_eq!(persisted.valid, snapshot.valid);

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn actor_commits_to_disk_and_rejects_failed_mutation_without_state_change() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let client = ProfilesClient::spawn_with_profiles(Profiles::default())
            .await
            .expect("profiles actor should spawn");
        client
            .set_valid_fields(&["actor-valid".to_string()])
            .await
            .expect("valid fields commit should succeed");

        let committed = client.snapshot().expect("snapshot should be readable");
        assert_eq!(committed.valid, vec!["actor-valid".to_string()]);

        let persisted: Profiles = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.path().join("profiles.yaml"))
                .expect("profiles.yaml should be persisted"),
        )
        .expect("persisted profiles should decode");
        assert_eq!(persisted.valid, committed.valid);

        let before_failure = std::fs::read_to_string(config_dir.path().join("profiles.yaml"))
            .expect("persisted state before failure");
        let error = client
            .set_current(Some(&"missing-profile".to_string()))
            .await
            .expect_err("invalid activation must fail");
        assert!(error.to_string().contains("failed to get the profile item"));

        let after = client.snapshot().expect("snapshot should remain readable");
        assert_eq!(after.valid, committed.valid);
        assert!(after.current.is_empty());
        assert_eq!(
            std::fs::read_to_string(config_dir.path().join("profiles.yaml"))
                .expect("persisted state after failure"),
            before_failure
        );

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn remote_import_commits_file_and_profile_state_atomically() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }
        std::fs::create_dir_all(config_dir.path().join("profiles"))
            .expect("profiles dir should exist for managed reservations");

        let fs = Arc::new(MemoryProfileFs::default());
        let client = ProfilesClient::spawn_from_profiles(
            Profiles::default(),
            fs.clone(),
            Arc::new(ImmediateFetcher),
            Arc::new(ImmediateImporter),
        )
        .await
        .expect("profiles actor should spawn");

        let (uid, activated) = client
            .import_remote(
                url::Url::parse("https://example.com/import.yaml").unwrap(),
                Some("Imported profile".to_string()),
                None,
                RemoteProfileImportMode::Direct,
            )
            .await
            .expect("remote import should commit");

        assert!(activated, "first config profile should become current");
        let snapshot = client.snapshot().expect("snapshot should be readable");
        assert_eq!(snapshot.current, vec![uid.clone()]);
        let imported = snapshot
            .get_item(&uid)
            .expect("imported profile should exist")
            .as_remote()
            .expect("imported profile should remain remote");
        assert_eq!(imported.shared.name, "Imported profile");
        assert_eq!(imported.shared.file, format!("{uid}.yaml"));
        assert_eq!(
            fs.files
                .lock()
                .unwrap()
                .get(&format!("{uid}.yaml"))
                .map(String::as_str),
            Some("mode: direct\n")
        );
        let persisted: Profiles = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.path().join("profiles.yaml"))
                .expect("profiles.yaml should be persisted"),
        )
        .expect("persisted profiles should decode");
        assert_eq!(persisted.current, snapshot.current);
        assert!(persisted.get_item(&uid).is_ok());
        assert!(
            std::fs::read_dir(config_dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".reserve"))
        );

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn remote_import_prepare_failure_leaves_no_file_or_profile_state() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }
        std::fs::create_dir_all(config_dir.path().join("profiles"))
            .expect("profiles dir should exist for managed reservations");

        let fs = Arc::new(MemoryProfileFs::default());
        let client = ProfilesClient::spawn_from_profiles(
            Profiles::default(),
            fs.clone(),
            Arc::new(ImmediateFetcher),
            Arc::new(FailingImporter),
        )
        .await
        .expect("profiles actor should spawn");

        let error = client
            .import_remote(
                url::Url::parse("https://example.com/fail.yaml").unwrap(),
                None,
                None,
                RemoteProfileImportMode::Default,
            )
            .await
            .expect_err("failed preparation must not commit");
        assert!(error.to_string().contains("injected remote import failure"));
        assert!(client.snapshot().unwrap().items.is_empty());
        assert!(fs.files.lock().unwrap().is_empty());
        assert!(fs.writes.lock().unwrap().is_empty());
        assert!(
            std::fs::read_dir(config_dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".reserve"))
        );

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[tokio::test]
    async fn cancelled_remote_import_drops_reservation_without_committing() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }
        std::fs::create_dir_all(config_dir.path().join("profiles"))
            .expect("profiles dir should exist for managed reservations");

        let fs = Arc::new(MemoryProfileFs::default());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let client = ProfilesClient::spawn_from_profiles(
            Profiles::default(),
            fs.clone(),
            Arc::new(ImmediateFetcher),
            Arc::new(BlockingImporter {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .await
        .expect("profiles actor should spawn");

        let waiter = {
            let client = client.clone();
            tokio::spawn(async move {
                client
                    .import_remote(
                        url::Url::parse("https://example.com/cancel.yaml").unwrap(),
                        None,
                        None,
                        RemoteProfileImportMode::Default,
                    )
                    .await
            })
        };
        started.notified().await;
        waiter.abort();
        let _ = waiter.await;
        release.notify_one();

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let no_reservations = std::fs::read_dir(config_dir.path())
                    .unwrap()
                    .filter_map(Result::ok)
                    .all(|entry| !entry.file_name().to_string_lossy().ends_with(".reserve"));
                if no_reservations {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled import reservation should be cleaned");

        assert!(client.snapshot().unwrap().items.is_empty());
        assert!(fs.files.lock().unwrap().is_empty());
        assert!(fs.writes.lock().unwrap().is_empty());

        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }
}

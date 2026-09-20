//! Profile persistence ports used by the application client during the staged migration.

use std::{io::Write, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use atomicwrites::{AtomicFile, OverwriteBehavior};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::{ChimeraClient, ChimeraClientInner, Degradation, DegradationPhase, MutationOutcome};

use crate::config::{
    core::Config,
    profile::{
        builder::ProfileBuilder,
        item::{
            Profile, ProfileKindGetter, ProfileMetaGetter,
            remote::{
                PreparedSubscriptionUpdate, RemoteProfile, RemoteProfileOptions,
                RemoteProfileOptionsBuilder, SubscriptionInfo,
            },
            shared::{PreparedProfileFile, ProfileSharedBuilder},
            utils::generate_uid,
        },
        item_type::{ProfileItemType, ProfileUid},
        profiles::Profiles,
    },
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

    async fn apply_remote_options(
        &self,
        uid: &ProfileUid,
        options: RemoteProfileOptionsBuilder,
    ) -> anyhow::Result<()>;

    async fn commit_refreshed(
        &self,
        uid: &ProfileUid,
        updated: RemoteProfile,
    ) -> anyhow::Result<bool>;

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
    ApplyRemoteOptions {
        uid: ProfileUid,
        options: RemoteProfileOptionsBuilder,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    CommitRefreshed {
        uid: ProfileUid,
        updated: RemoteProfile,
        reply: RpcReplyPort<anyhow::Result<bool>>,
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

struct ProfilesActorState {
    profiles: Profiles,
    snapshot_tx: tokio::sync::watch::Sender<Profiles>,
}

struct ProfilesActor;

impl ProfilesActorState {
    fn publish_and_sync_legacy(&mut self, next: Profiles) {
        self.profiles = next.clone();
        self.snapshot_tx.send_replace(next.clone());

        // Compatibility projection only. The actor is the sole writer; this
        // mirror remains until the remaining legacy runtime/diagnostics reads
        // have moved to the client snapshot.
        let legacy = Config::profiles();
        {
            let mut draft = legacy.draft();
            *draft = next;
        }
        legacy.apply();
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
        self.publish_and_sync_legacy(next);
        Ok(value)
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
        _myself: ActorRef<Self::Msg>,
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
            ProfilesActorMessage::ApplyRemoteOptions {
                uid,
                options,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| {
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
                    })
                    .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::CommitRefreshed {
                uid,
                updated,
                reply,
            } => {
                let result = state
                    .mutate(move |profiles| {
                        let affects_current = profiles
                            .current
                            .iter()
                            .any(|current_uid| current_uid == &uid);
                        profiles.replace_item(&uid, updated.into())?;
                        Ok(affects_current)
                    })
                    .await;
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
    pub(crate) async fn spawn() -> anyhow::Result<Self> {
        Self::spawn_from_profiles(Config::profiles().latest().clone()).await
    }

    async fn spawn_from_profiles(profiles: Profiles) -> anyhow::Result<Self> {
        let (snapshot_tx, snapshot_rx) = tokio::sync::watch::channel(profiles.clone());
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorState {
                profiles,
                snapshot_tx,
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
        Self::spawn_from_profiles(profiles).await
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

    async fn apply_remote_options(
        &self,
        uid: &ProfileUid,
        options: RemoteProfileOptionsBuilder,
    ) -> anyhow::Result<()> {
        self.call(|reply| ProfilesActorMessage::ApplyRemoteOptions {
            uid: uid.clone(),
            options,
            reply,
        })
        .await
    }

    async fn commit_refreshed(
        &self,
        uid: &ProfileUid,
        updated: RemoteProfile,
    ) -> anyhow::Result<bool> {
        self.call(|reply| ProfilesActorMessage::CommitRefreshed {
            uid: uid.clone(),
            updated,
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

pub(super) struct PendingProfileRefresh {
    inner: Arc<ChimeraClientInner>,
    uid: ProfileUid,
}

impl Drop for PendingProfileRefresh {
    fn drop(&mut self) {
        self.inner
            .pending_refreshes
            .lock()
            .expect("pending profile refresh lock")
            .remove(&self.uid);
    }
}

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

    pub(super) fn begin_profile_refresh(
        &self,
        uid: &ProfileUid,
    ) -> anyhow::Result<PendingProfileRefresh> {
        let mut pending = self
            .inner
            .pending_refreshes
            .lock()
            .expect("pending profile refresh lock");
        anyhow::ensure!(
            pending.insert(uid.clone()),
            "profile refresh already in progress"
        );
        drop(pending);
        Ok(PendingProfileRefresh {
            inner: self.inner.clone(),
            uid: uid.clone(),
        })
    }

    fn remote_profile_state_snapshot(&self, uid: &ProfileUid) -> anyhow::Result<RemoteProfile> {
        let profiles = self.inner.profiles.snapshot()?;
        let item = profiles.get_item(uid)?;
        item.as_remote()
            .ok_or_else(|| anyhow::anyhow!("profile `{uid}` is not remote"))
            .cloned()
    }

    pub(super) async fn remote_profile_snapshot(
        &self,
        uid: &ProfileUid,
    ) -> anyhow::Result<(RemoteProfile, String)> {
        let remote = self.remote_profile_state_snapshot(uid)?;
        let previous_file = self.inner.profile_files.read(&remote.shared.file).await?;
        Ok((remote, previous_file))
    }

    pub(super) fn remote_profile_fingerprint(profile: &RemoteProfile) -> anyhow::Result<String> {
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

    pub(super) fn ensure_refresh_is_current(
        expected_fingerprint: &str,
        current: &RemoteProfile,
    ) -> anyhow::Result<()> {
        let current_fingerprint = Self::remote_profile_fingerprint(current)?;
        anyhow::ensure!(
            current_fingerprint == expected_fingerprint,
            "profile changed while refresh was in progress"
        );
        Ok(())
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

    pub(super) async fn commit_refreshed_profile(
        &self,
        uid: ProfileUid,
        expected_fingerprint: String,
        previous_file: String,
        prepared: PreparedSubscriptionUpdate,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _commit = self.inner.profile_commit.lock().await;
        let current = self.remote_profile_state_snapshot(&uid)?;
        Self::ensure_refresh_is_current(&expected_fingerprint, &current)?;
        let mut updated = current.clone();
        let content = updated.apply_prepared_subscription_update(prepared)?;
        let file = current.shared.file.clone();
        self.inner
            .profile_files
            .write_atomic(&file, &content)
            .await?;
        let affects_current = match self
            .inner
            .profile_writes
            .commit_refreshed(&uid, updated)
            .await
        {
            Ok(affects_current) => affects_current,
            Err(error) => {
                if let Err(restore_error) = self
                    .inner
                    .profile_files
                    .write_atomic(&file, &previous_file)
                    .await
                {
                    return Err(error.context(format!(
                        "failed to restore materialized profile after refresh commit failure: {restore_error:#}"
                    )));
                }
                return Err(error);
            }
        };
        self.inner.ui_sink.refresh_profiles();
        drop(_commit);
        if affects_current {
            Ok(self
                .after_profile_runtime_commit("remote profile refresh")
                .await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    pub(crate) async fn refresh_profile(
        &self,
        uid: ProfileUid,
        options: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _pending = self.begin_profile_refresh(&uid)?;
        if let Some(options) = options {
            let _commit = self.inner.profile_commit.lock().await;
            self.inner
                .profile_writes
                .apply_remote_options(&uid, options)
                .await?;
            self.inner.ui_sink.refresh_profiles();
        }
        let (initial, previous_file) = self.remote_profile_snapshot(&uid).await?;
        let expected_fingerprint = Self::remote_profile_fingerprint(&initial)?;
        let prepared = initial.prepare_subscription_update(None).await?;
        self.commit_refreshed_profile(uid, expected_fingerprint, previous_file, prepared)
            .await
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
    use std::sync::Mutex;

    use super::{ProfilesClient, ProfilesReadPort, ProfilesWritePort};
    use crate::config::profile::profiles::Profiles;

    static CONFIG_DIR_LOCK: Mutex<()> = Mutex::new(());

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
}

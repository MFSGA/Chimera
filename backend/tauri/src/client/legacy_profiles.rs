//! Profile persistence ports used by the application client during the staged migration.

use std::{io::Write, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use atomicwrites::{AtomicFile, OverwriteBehavior};

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

pub(crate) struct LegacyProfilesReadPort;

pub(crate) struct LegacyProfileFsPort;

pub(crate) struct LegacyProfilesWritePort;

impl ProfilesReadPort for LegacyProfilesReadPort {
    fn snapshot(&self) -> anyhow::Result<Profiles> {
        Ok(Config::profiles().latest().clone())
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

impl LegacyProfilesWritePort {
    async fn persist<T, F>(update: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Profiles) -> anyhow::Result<T> + Send + 'static,
    {
        tokio::task::spawn_blocking(move || {
            let profiles = Config::profiles();
            let result = {
                let mut draft = profiles.draft();
                update(&mut draft).and_then(|value| draft.save_file().map(|_| value))
            };
            match result {
                Ok(value) => {
                    profiles.apply();
                    Ok(value)
                }
                Err(error) => {
                    profiles.discard();
                    Err(error)
                }
            }
        })
        .await
        .context("profile state persistence task failed")?
    }
}

#[async_trait]
impl ProfilesWritePort for LegacyProfilesWritePort {
    async fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)> {
        let uid = profile.uid().to_string();
        let activatable = profile.kind().is_config();
        Self::persist(move |profiles| {
            let activate = activatable && profiles.current.is_empty();
            profiles.append_item(profile)?;
            if activate {
                profiles.current = vec![uid.clone()];
            }
            Ok((uid, activate))
        })
        .await
    }

    async fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)> {
        let uid = uid.clone();
        Self::persist(move |profiles| {
            let file = profiles.get_item(&uid)?.file().to_string();
            let affects_current = profiles.delete_item(&uid)?;
            Ok((file, affects_current))
        })
        .await
    }

    async fn patch_profile(&self, uid: &ProfileUid, profile: ProfileBuilder) -> anyhow::Result<()> {
        let uid = uid.clone();
        Self::persist(move |profiles| {
            let current = profiles
                .items
                .iter_mut()
                .find(|item| item.uid() == uid)
                .ok_or_else(|| anyhow::anyhow!("failed to get the profile item `uid:{uid}`"))?;
            match (current, profile) {
                (Profile::Remote(item), ProfileBuilder::Remote(builder)) => builder
                    .patch_profile(item)
                    .context("failed to patch remote profile")?,
                (Profile::Local(item), ProfileBuilder::Local(builder)) => item.apply(builder),
                (Profile::Merge(item), ProfileBuilder::Merge(builder)) => item.apply(builder),
                (Profile::Script(item), ProfileBuilder::Script(builder)) => item.apply(builder),
                _ => anyhow::bail!("profile type mismatch"),
            }
            Ok(())
        })
        .await
    }

    async fn patch_metadata(
        &self,
        uid: &ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<()> {
        let uid = uid.clone();
        Self::persist(move |profiles| profiles.patch_metadata(&uid, name, desc)).await
    }

    async fn patch_remote_options(
        &self,
        uid: &ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<()> {
        let uid = uid.clone();
        Self::persist(move |profiles| {
            profiles.patch_remote_options(
                &uid,
                user_agent,
                with_proxy,
                self_proxy,
                update_interval_minutes,
            )
        })
        .await
    }

    async fn reorder(&self, active_id: &ProfileUid, over_id: &ProfileUid) -> anyhow::Result<()> {
        let active_id = active_id.clone();
        let over_id = over_id.clone();
        Self::persist(move |profiles| profiles.reorder(&active_id, &over_id)).await
    }

    async fn reorder_by_list(&self, list: &[ProfileUid]) -> anyhow::Result<()> {
        let list = list.to_vec();
        Self::persist(move |profiles| profiles.reorder_by_list(&list)).await
    }

    async fn set_current(&self, uid: Option<&ProfileUid>) -> anyhow::Result<()> {
        let uid = uid.cloned();
        Self::persist(move |profiles| profiles.activate(uid.as_deref())).await
    }

    async fn set_valid_fields(&self, fields: &[String]) -> anyhow::Result<()> {
        let fields = fields.to_vec();
        Self::persist(move |profiles| {
            profiles.valid = fields;
            Ok(())
        })
        .await
    }

    async fn set_profile_transform_chain(
        &self,
        uid: &ProfileUid,
        transforms: &[ProfileUid],
    ) -> anyhow::Result<bool> {
        let uid = uid.clone();
        let transforms = transforms.to_vec();
        Self::persist(move |profiles| profiles.set_profile_transform_chain(&uid, transforms)).await
    }

    async fn set_global_transform_chain(&self, transforms: &[ProfileUid]) -> anyhow::Result<bool> {
        let transforms = transforms.to_vec();
        Self::persist(move |profiles| profiles.set_global_transform_chain(transforms)).await
    }

    async fn apply_remote_options(
        &self,
        uid: &ProfileUid,
        options: RemoteProfileOptionsBuilder,
    ) -> anyhow::Result<()> {
        let uid = uid.clone();
        Self::persist(move |profiles| {
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
        .await
    }

    async fn commit_refreshed(
        &self,
        uid: &ProfileUid,
        updated: RemoteProfile,
    ) -> anyhow::Result<bool> {
        let uid = uid.clone();
        Self::persist(move |profiles| {
            let affects_current = profiles
                .current
                .iter()
                .any(|current_uid| current_uid == &uid);
            profiles.replace_item(&uid, updated.into())?;
            Ok(affects_current)
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
        let uid = uid.clone();
        let file = file.to_string();
        let transforms = transforms.to_vec();
        Self::persist(move |profiles| {
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
    pub(crate) async fn legacy_get_profiles(&self) -> anyhow::Result<Profiles> {
        self.inner.profiles.snapshot()
    }

    pub(crate) fn reserve_managed_profile_identity(
        &self,
        kind: &ProfileItemType,
    ) -> anyhow::Result<(ProfileUid, PreparedProfileFile)> {
        todo!()
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
        todo!()
    }

    pub(super) async fn remote_profile_snapshot(
        &self,
        uid: &ProfileUid,
    ) -> anyhow::Result<(RemoteProfile, String)> {
        let remote = self.remote_profile_state_snapshot(uid)?;
        todo!()
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
        let profile = profiles.get_item(&uid)?;
        self.inner.profile_files.resolve_path(profile.file()).await
    }

    pub(crate) async fn legacy_read_profile_file(&self, uid: ProfileUid) -> anyhow::Result<String> {
        let profiles = self.inner.profiles.snapshot()?;
        let profile = profiles.get_item(&uid)?;
        let content = self.inner.profile_files.read(profile.file()).await?;
        if profile.kind().is_config() {
            crate::service::profile_file::normalize_yaml_document(&content)
        } else {
            Ok(content)
        }
    }

    pub(crate) async fn commit_new_profile(
        &self,
        profile: Profile,
        mut prepared_file: PreparedProfileFile,
        materialized_content: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileUid>> {
        todo!()
    }

    pub(crate) async fn patch_profile(
        &self,
        uid: ProfileUid,
        profile: ProfileBuilder,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn legacy_patch_profile_metadata(
        &self,
        uid: ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn legacy_patch_remote_profile_options(
        &self,
        uid: ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(super) async fn commit_refreshed_profile(
        &self,
        uid: ProfileUid,
        expected_fingerprint: String,
        previous_file: String,
        prepared: PreparedSubscriptionUpdate,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn refresh_profile(
        &self,
        uid: ProfileUid,
        options: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
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
        todo!()
    }

    pub(crate) async fn legacy_delete_profile(
        &self,
        uid: ProfileUid,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn legacy_reorder_profile(
        &self,
        active_id: ProfileUid,
        over_id: ProfileUid,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn legacy_reorder_profiles_by_list(
        &self,
        list: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
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

    pub(crate) async fn legacy_activate_profile(
        &self,
        uid: Option<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn legacy_set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        {
            let _commit = self.inner.profile_commit.lock().await;
            // self.inner.profile_writes.set_valid_fields(&fields).await?;
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
        todo!()
    }

    pub(crate) async fn set_global_transform_chain(
        &self,
        transforms: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    pub(crate) async fn legacy_save_profile_file(
        &self,
        uid: ProfileUid,
        file_data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        todo!()
    }

    async fn rebuild_profile_runtime(&self) -> anyhow::Result<()> {
        let break_when = self.get_clash_config()?.break_connection.on_profile_change;
        self.rebuild_running_config().await?;
        self.inner.core.on_profile_change(break_when).await;
        Ok(())
    }
}

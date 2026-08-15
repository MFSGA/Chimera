//! Transitional instance-owned facade for the runtime/rebuild path.
//!
//! This mirrors REF's `NyanpasuClient` ownership direction without pretending the
//! rest of Chimera has already completed the actor/DI migration. Legacy globals
//! are contained behind ports here so IPC callers can migrate incrementally.

use std::{
    collections::HashSet,
    io::Write,
    sync::{Arc, Mutex as StdMutex},
};

use anyhow::Context;
use async_trait::async_trait;
use atomicwrites::{AtomicFile, OverwriteBehavior};
use chimera_ipc::api::status::CoreState;
use serde::{Deserialize, Serialize};

use super::{
    core::{CoreLifecycleLease as CoreManagerLifecycleLease, CoreManager, RunType},
    transaction::{RuntimePatchCoordinator, TransactionOutcome},
};
const PROFILE_IDENTITY_ATTEMPTS: usize = 32;

use crate::{
    config::{
        chimera::{ClashCore, IVerge},
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
        runtime::ClashConfigOverrides,
    },
    core::{connection_interruption::ConnectionInterruptionService, handle::Handle},
};

/// Public mutation wire aligned with REF: desired state is committed first;
/// post-commit side-effect failures degrade instead of turning the mutation
/// into an error that would imply the commit was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Applied {
        value: T,
    },
    CommittedDegraded {
        value: T,
        degradations: Vec<Degradation>,
    },
}

impl<T> MutationOutcome<T> {
    pub fn from_parts(value: T, degradations: Vec<Degradation>) -> Self {
        if degradations.is_empty() {
            Self::Applied { value }
        } else {
            Self::CommittedDegraded {
                value,
                degradations,
            }
        }
    }

    #[allow(dead_code)]
    pub fn degradations(&self) -> &[Degradation] {
        match self {
            Self::Applied { .. } => &[],
            Self::CommittedDegraded { degradations, .. } => degradations,
        }
    }

    fn into_parts(self) -> (T, Vec<Degradation>) {
        match self {
            Self::Applied { value } => (value, Vec::new()),
            Self::CommittedDegraded {
                value,
                degradations,
            } => (value, degradations),
        }
    }

    fn extend_degradations(self, extra: Vec<Degradation>) -> Self {
        let (value, mut degradations) = self.into_parts();
        degradations.extend(extra);
        Self::from_parts(value, degradations)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Degradation {
    pub phase: DegradationPhase,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DegradationPhase {
    LegacyMirror,
    ProfileMaterialization,
    RuntimeBuild,
    RuntimeCheck,
    RuntimePromote,
    RuntimePublish,
    RuntimeApply,
    CoreRollback,
    SystemEffect,
    UiEffect,
}

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
}

#[async_trait]
pub(crate) trait CoreLifecycleLease: Send {
    async fn rebuild_running_config(&mut self) -> anyhow::Result<()>;
    #[allow(dead_code)]
    async fn stop(&mut self) -> anyhow::Result<()>;
    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn on_profile_change(&self);
}

pub(crate) trait UiEventSink: Send + Sync {
    fn refresh_clash(&self);
    fn refresh_profiles(&self);
}

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

pub(crate) trait ProfilesWritePort: Send + Sync {
    fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)>;

    fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)>;

    fn patch_profile(&self, uid: &ProfileUid, profile: ProfileBuilder) -> anyhow::Result<()>;

    fn patch_metadata(
        &self,
        uid: &ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<()>;

    fn patch_remote_options(
        &self,
        uid: &ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<()>;

    fn reorder(&self, active_id: &ProfileUid, over_id: &ProfileUid) -> anyhow::Result<()>;

    fn reorder_by_list(&self, list: &[ProfileUid]) -> anyhow::Result<()>;

    fn set_current(&self, uid: Option<&ProfileUid>) -> anyhow::Result<()>;

    fn set_valid_fields(&self, fields: &[String]) -> anyhow::Result<()>;

    fn apply_remote_options(
        &self,
        uid: &ProfileUid,
        options: RemoteProfileOptionsBuilder,
    ) -> anyhow::Result<()>;

    fn commit_refreshed(&self, uid: &ProfileUid, updated: RemoteProfile) -> anyhow::Result<bool>;

    fn replace_remote_definition(
        &self,
        uid: &ProfileUid,
        file: &str,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
    ) -> anyhow::Result<bool>;
}

struct LegacyProfilesReadPort;

struct LegacyProfileFsPort;

struct LegacyProfilesWritePort;

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
    fn persist<T>(
        &self,
        update: impl FnOnce(&mut Profiles) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
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
    }
}

impl ProfilesWritePort for LegacyProfilesWritePort {
    fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)> {
        let uid = profile.uid().to_string();
        self.persist(|profiles| {
            let activate = profiles.current.is_empty();
            profiles.append_item(profile)?;
            if activate {
                profiles.current = vec![uid.clone()];
            }
            Ok((uid, activate))
        })
    }

    fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)> {
        self.persist(|profiles| {
            let file = profiles.get_item(uid)?.file().to_string();
            let affects_current = profiles.delete_item(uid)?;
            Ok((file, affects_current))
        })
    }

    fn patch_profile(&self, uid: &ProfileUid, profile: ProfileBuilder) -> anyhow::Result<()> {
        self.persist(|profiles| {
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
                _ => anyhow::bail!("profile type mismatch"),
            }
            Ok(())
        })
    }

    fn patch_metadata(
        &self,
        uid: &ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<()> {
        self.persist(|profiles| profiles.patch_metadata(uid, name, desc))
    }

    fn patch_remote_options(
        &self,
        uid: &ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<()> {
        self.persist(|profiles| {
            profiles.patch_remote_options(
                uid,
                user_agent,
                with_proxy,
                self_proxy,
                update_interval_minutes,
            )
        })
    }

    fn reorder(&self, active_id: &ProfileUid, over_id: &ProfileUid) -> anyhow::Result<()> {
        self.persist(|profiles| profiles.reorder(active_id, over_id))
    }

    fn reorder_by_list(&self, list: &[ProfileUid]) -> anyhow::Result<()> {
        self.persist(|profiles| profiles.reorder_by_list(list))
    }

    fn set_current(&self, uid: Option<&ProfileUid>) -> anyhow::Result<()> {
        self.persist(|profiles| profiles.activate(uid.map(String::as_str)))
    }

    fn set_valid_fields(&self, fields: &[String]) -> anyhow::Result<()> {
        self.persist(|profiles| {
            profiles.valid = fields.to_vec();
            Ok(())
        })
    }

    fn apply_remote_options(
        &self,
        uid: &ProfileUid,
        options: RemoteProfileOptionsBuilder,
    ) -> anyhow::Result<()> {
        self.persist(|profiles| {
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
    }

    fn commit_refreshed(&self, uid: &ProfileUid, updated: RemoteProfile) -> anyhow::Result<bool> {
        self.persist(|profiles| {
            let affects_current = profiles
                .current
                .iter()
                .any(|current_uid| current_uid == uid);
            profiles.replace_item(uid, updated.into())?;
            Ok(affects_current)
        })
    }

    fn replace_remote_definition(
        &self,
        uid: &ProfileUid,
        file: &str,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
    ) -> anyhow::Result<bool> {
        self.persist(|profiles| {
            let affects_current = profiles.current.iter().any(|current| current == uid);
            profiles.replace_remote_definition(uid, file, updated_at, url, option, subscription)?;
            Ok(affects_current)
        })
    }
}

struct LegacyCoreLifecyclePort;

struct LegacyCoreLifecycleLease {
    lease: CoreManagerLifecycleLease<'static>,
}

#[async_trait]
impl CoreLifecycleLease for LegacyCoreLifecycleLease {
    async fn rebuild_running_config(&mut self) -> anyhow::Result<()> {
        self.lease.rebuild_running_config().await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.lease.stop_core().await
    }

    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()> {
        self.lease.change_core(clash_core).await
    }
}

#[async_trait]
impl CoreLifecyclePort for LegacyCoreLifecyclePort {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        Ok(Box::new(LegacyCoreLifecycleLease {
            lease: CoreManager::global().begin_lifecycle().await,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at, run_type) = CoreManager::global().status().await;
        Ok(CoreStatusSnapshot {
            state: state.into_owned(),
            state_changed_at,
            run_type,
        })
    }

    async fn on_profile_change(&self) {
        let _ = ConnectionInterruptionService::on_profile_change().await;
    }
}

struct LegacyUiEventSink;

impl UiEventSink for LegacyUiEventSink {
    fn refresh_clash(&self) {
        Handle::refresh_clash();
    }

    fn refresh_profiles(&self) {
        Handle::refresh_profiles();
    }
}

#[derive(Clone)]
pub(crate) struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

struct NyanpasuClientInner {
    core: Arc<dyn CoreLifecyclePort>,
    profiles: Arc<dyn ProfilesReadPort>,
    profile_files: Arc<dyn ProfileFsPort>,
    profile_writes: Arc<dyn ProfilesWritePort>,
    ui_sink: Arc<dyn UiEventSink>,
    runtime_patch: RuntimePatchCoordinator,
    profile_commit: tokio::sync::Mutex<()>,
    verge_patch: tokio::sync::Mutex<()>,
    pending_refreshes: StdMutex<HashSet<ProfileUid>>,
}

struct PendingProfileRefresh {
    inner: Arc<NyanpasuClientInner>,
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

impl NyanpasuClient {
    pub(crate) fn legacy() -> Self {
        Self::with_parts(
            Arc::new(LegacyCoreLifecyclePort),
            Arc::new(LegacyProfilesReadPort),
            Arc::new(LegacyProfileFsPort),
            Arc::new(LegacyProfilesWritePort),
            Arc::new(LegacyUiEventSink),
        )
    }

    fn with_parts(
        core: Arc<dyn CoreLifecyclePort>,
        profiles: Arc<dyn ProfilesReadPort>,
        profile_files: Arc<dyn ProfileFsPort>,
        profile_writes: Arc<dyn ProfilesWritePort>,
        ui_sink: Arc<dyn UiEventSink>,
    ) -> Self {
        let inner = NyanpasuClientInner {
            core,
            profiles,
            profile_files,
            profile_writes,
            ui_sink,
            runtime_patch: RuntimePatchCoordinator::default(),
            profile_commit: tokio::sync::Mutex::new(()),
            verge_patch: tokio::sync::Mutex::new(()),
            pending_refreshes: StdMutex::new(HashSet::new()),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.core.status().await
    }

    pub(crate) async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.change_core(clash_core).await
    }

    pub(crate) async fn patch_verge(&self, patch: IVerge) -> anyhow::Result<()> {
        let _patch = self.inner.verge_patch.lock().await;
        crate::feat::patch_verge_uncoordinated(patch).await
    }

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

    fn begin_profile_refresh(&self, uid: &ProfileUid) -> anyhow::Result<PendingProfileRefresh> {
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

    async fn remote_profile_snapshot(
        &self,
        uid: &ProfileUid,
    ) -> anyhow::Result<(RemoteProfile, String)> {
        let remote = self.remote_profile_state_snapshot(uid)?;
        let previous_file = self.inner.profile_files.read(&remote.shared.file).await?;
        Ok((remote, previous_file))
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

    fn ensure_refresh_is_current(
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
            let result = self.inner.profile_writes.add(profile)?;
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
            self.inner.profile_writes.patch_profile(&uid, profile)?;
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
        self.inner.profile_writes.patch_metadata(&uid, name, desc)?;
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
        self.inner.profile_writes.patch_remote_options(
            &uid,
            user_agent,
            with_proxy,
            self_proxy,
            update_interval_minutes,
        )?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    async fn commit_refreshed_profile(
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
        let affects_current = match self.inner.profile_writes.commit_refreshed(&uid, updated) {
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
                .apply_remote_options(&uid, options)?;
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
    ) -> anyhow::Result<MutationOutcome<()>> {
        let affects_current = {
            let _commit = self.inner.profile_commit.lock().await;
            let affects_current = self.inner.profile_writes.replace_remote_definition(
                &uid,
                &file,
                updated_at,
                url,
                option,
                subscription,
            )?;
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
            let result = self.inner.profile_writes.delete(&uid)?;
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
        self.inner.profile_writes.reorder(&active_id, &over_id)?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    pub(crate) async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileUid>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let _commit = self.inner.profile_commit.lock().await;
        self.inner.profile_writes.reorder_by_list(&list)?;
        self.inner.ui_sink.refresh_profiles();
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }

    async fn after_profile_runtime_commit(&self, operation: &str) -> MutationOutcome<()> {
        match self.rebuild_running_config().await {
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
            self.inner.profile_writes.set_current(uid.as_ref())?;
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
            self.inner.profile_writes.set_valid_fields(&fields)?;
            self.inner.ui_sink.refresh_profiles();
        }
        Ok(self
            .after_profile_runtime_commit("profile valid fields update")
            .await)
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
            anyhow::ensure!(
                !matches!(item.kind(), ProfileItemType::Remote),
                "remote profiles are updater-owned"
            );
            serde_yaml::from_str::<serde_yaml::Mapping>(&file_data)
                .context("failed to parse profile YAML")?;
            self.inner
                .profile_files
                .write_atomic(item.file(), &file_data)
                .await?;
            profiles.current.iter().any(|current| current == &uid)
        };
        if affects_current {
            Ok(self.after_profile_runtime_commit("profile file save").await)
        } else {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
    }

    pub(crate) async fn patch_running_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        let mapping = overrides.to_mapping();
        let persist_overrides = overrides.clone();
        self.inner
            .runtime_patch
            .apply(
                mapping,
                super::api::get_configs,
                |patch| async move { super::api::patch_configs(&patch).await },
                move |_patch| {
                    let overrides = persist_overrides.clone();
                    async move { crate::feat::patch_clash_overrides(overrides).await }
                },
            )
            .await
    }

    pub(crate) async fn rebuild_running_config(&self) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.rebuild_running_config().await?;
        self.inner.ui_sink.refresh_clash();
        self.inner.core.on_profile_change().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::config::profile::item::{
        local::LocalProfile,
        remote::{RemoteProfileOptions, SubscriptionInfo},
        shared::ProfileShared,
    };

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        fail_rebuild: bool,
    }

    struct RecordingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
        fail_rebuild: bool,
    }

    #[async_trait]
    impl CoreLifecycleLease for RecordingLease {
        async fn rebuild_running_config(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            if self.fail_rebuild {
                anyhow::bail!("injected rebuild failure");
            }
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop");
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("change-core");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            self.events.lock().unwrap().push("begin");
            Ok(Box::new(RecordingLease {
                events: self.events.clone(),
                fail_rebuild: self.fail_rebuild,
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 7,
                run_type: RunType::Normal,
            })
        }

        async fn on_profile_change(&self) {
            self.events.lock().unwrap().push("profile-change");
        }
    }

    struct StaticProfilesRead {
        profiles: Profiles,
    }

    impl ProfilesReadPort for StaticProfilesRead {
        fn snapshot(&self) -> anyhow::Result<Profiles> {
            Ok(self.profiles.clone())
        }
    }

    struct NoopProfileFs;

    #[async_trait]
    impl ProfileFsPort for NoopProfileFs {
        async fn resolve_path(&self, file: &str) -> anyhow::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from(file))
        }
        async fn read(&self, _file: &str) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn write_atomic(&self, _file: &str, _content: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove(&self, _file: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct RecordingProfileFs {
        previous_file: String,
        reads: Arc<Mutex<Vec<String>>>,
        writes: Arc<Mutex<Vec<(String, String)>>>,
        fail_write: bool,
    }

    #[async_trait]
    impl ProfileFsPort for RecordingProfileFs {
        async fn resolve_path(&self, file: &str) -> anyhow::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from(file))
        }
        async fn read(&self, file: &str) -> anyhow::Result<String> {
            self.reads.lock().unwrap().push(file.to_string());
            Ok(self.previous_file.clone())
        }

        async fn write_atomic(&self, file: &str, content: &str) -> anyhow::Result<()> {
            self.writes
                .lock()
                .unwrap()
                .push((file.to_string(), content.to_string()));
            if self.fail_write {
                anyhow::bail!("injected profile file write failure");
            }
            Ok(())
        }

        async fn remove(&self, _file: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct NoopProfilesWrite {
        fail_refresh: bool,
        refresh_commits: Option<Arc<Mutex<usize>>>,
        patch_commits: Option<Arc<Mutex<usize>>>,
    }

    impl ProfilesWritePort for NoopProfilesWrite {
        fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)> {
            Ok((profile.uid().to_string(), false))
        }
        fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)> {
            Ok((format!("{uid}.yaml"), false))
        }
        fn patch_profile(&self, _uid: &ProfileUid, _profile: ProfileBuilder) -> anyhow::Result<()> {
            if let Some(commits) = &self.patch_commits {
                *commits.lock().unwrap() += 1;
            }
            Ok(())
        }
        fn patch_metadata(
            &self,
            _uid: &ProfileUid,
            _name: Option<String>,
            _desc: Option<Option<String>>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn patch_remote_options(
            &self,
            _uid: &ProfileUid,
            _user_agent: Option<Option<String>>,
            _with_proxy: Option<bool>,
            _self_proxy: Option<bool>,
            _update_interval_minutes: Option<u64>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn reorder(&self, _active_id: &ProfileUid, _over_id: &ProfileUid) -> anyhow::Result<()> {
            Ok(())
        }
        fn reorder_by_list(&self, _list: &[ProfileUid]) -> anyhow::Result<()> {
            Ok(())
        }
        fn set_current(&self, _uid: Option<&ProfileUid>) -> anyhow::Result<()> {
            Ok(())
        }
        fn set_valid_fields(&self, _fields: &[String]) -> anyhow::Result<()> {
            Ok(())
        }
        fn apply_remote_options(
            &self,
            _uid: &ProfileUid,
            _options: RemoteProfileOptionsBuilder,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn commit_refreshed(
            &self,
            _uid: &ProfileUid,
            _updated: RemoteProfile,
        ) -> anyhow::Result<bool> {
            if let Some(commits) = &self.refresh_commits {
                *commits.lock().unwrap() += 1;
            }
            if self.fail_refresh {
                anyhow::bail!("injected profile state failure");
            }
            Ok(false)
        }
        fn replace_remote_definition(
            &self,
            _uid: &ProfileUid,
            _file: &str,
            _updated_at: Option<usize>,
            _url: url::Url,
            _option: Option<RemoteProfileOptions>,
            _subscription: Option<SubscriptionInfo>,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
    }

    struct RecordingUi {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl UiEventSink for RecordingUi {
        fn refresh_clash(&self) {
            self.events.lock().unwrap().push("refresh-ui");
        }
        fn refresh_profiles(&self) {
            self.events.lock().unwrap().push("refresh-profiles");
        }
    }

    fn test_local_profile(uid: &str) -> Profile {
        Profile::Local(LocalProfile {
            shared: ProfileShared {
                uid: uid.into(),
                name: "Local Test".into(),
                file: format!("{uid}.yaml"),
                desc: None,
                updated: 7,
            },
            symlinks: None,
            chain: Vec::new(),
        })
    }

    fn test_remote_profile() -> RemoteProfile {
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

    fn recording_client_with_profiles(
        profiles: Profiles,
        fail_rebuild: bool,
    ) -> (NyanpasuClient, Arc<Mutex<Vec<&'static str>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = NyanpasuClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild,
            }),
            Arc::new(StaticProfilesRead { profiles }),
            Arc::new(NoopProfileFs),
            Arc::new(NoopProfilesWrite::default()),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );
        (client, events)
    }

    fn recording_client(fail_rebuild: bool) -> (NyanpasuClient, Arc<Mutex<Vec<&'static str>>>) {
        recording_client_with_profiles(Profiles::default(), fail_rebuild)
    }

    #[test]
    fn duplicate_profile_refresh_is_rejected_until_guard_drops() {
        let (client, _) = recording_client(false);
        let uid = "r-test".to_string();
        let first = client.begin_profile_refresh(&uid).unwrap();
        let error = match client.begin_profile_refresh(&uid) {
            Ok(_) => panic!("duplicate refresh should be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("already in progress"));
        drop(first);
        assert!(client.begin_profile_refresh(&uid).is_ok());
    }

    #[test]
    fn refresh_fingerprint_tracks_definition_but_not_display_metadata() {
        let profile = test_remote_profile();
        let fingerprint = NyanpasuClient::remote_profile_fingerprint(&profile).unwrap();
        let mut renamed = profile.clone();
        renamed.shared.name = "Renamed".into();
        assert!(NyanpasuClient::ensure_refresh_is_current(&fingerprint, &renamed).is_ok());
        let mut changed_url = profile.clone();
        changed_url.url = url::Url::parse("https://example.com/changed.yaml").unwrap();
        assert!(NyanpasuClient::ensure_refresh_is_current(&fingerprint, &changed_url).is_err());
        let mut refreshed = profile;
        refreshed.shared.updated += 1;
        assert!(NyanpasuClient::ensure_refresh_is_current(&fingerprint, &refreshed).is_err());
    }

    #[tokio::test]
    async fn refresh_file_write_failure_does_not_commit_profile_state() {
        let remote = test_remote_profile();
        let uid = remote.shared.uid.clone();
        let profiles = Profiles {
            items: vec![Profile::Remote(remote.clone())],
            ..Profiles::default()
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let reads = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let refresh_commits = Arc::new(Mutex::new(0));
        let previous_file = "mode: rule\n".to_string();
        let client = NyanpasuClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead { profiles }),
            Arc::new(RecordingProfileFs {
                previous_file: previous_file.clone(),
                reads: reads.clone(),
                writes: writes.clone(),
                fail_write: true,
            }),
            Arc::new(NoopProfilesWrite {
                refresh_commits: Some(refresh_commits.clone()),
                ..NoopProfilesWrite::default()
            }),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let (_, snapshot_file) = client.remote_profile_snapshot(&uid).await.unwrap();
        assert_eq!(snapshot_file, previous_file);

        let mut data = serde_yaml::Mapping::new();
        data.insert("mode".into(), "global".into());
        let prepared = PreparedSubscriptionUpdate::for_test(
            data,
            SubscriptionInfo {
                upload: 10,
                download: 20,
                total: 30,
                expire: 40,
            },
        );
        let fingerprint = NyanpasuClient::remote_profile_fingerprint(&remote).unwrap();

        let error = client
            .commit_refreshed_profile(uid, fingerprint, previous_file, prepared)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("injected profile file write failure")
        );
        assert_eq!(*refresh_commits.lock().unwrap(), 0);
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refresh_state_failure_restores_file_through_profile_fs_port() {
        let remote = test_remote_profile();
        let uid = remote.shared.uid.clone();
        let profiles = Profiles {
            items: vec![Profile::Remote(remote.clone())],
            ..Profiles::default()
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let reads = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let previous_file = "mode: rule\n".to_string();
        let client = NyanpasuClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead { profiles }),
            Arc::new(RecordingProfileFs {
                previous_file: previous_file.clone(),
                reads: reads.clone(),
                writes: writes.clone(),
                fail_write: false,
            }),
            Arc::new(NoopProfilesWrite {
                fail_refresh: true,
                ..NoopProfilesWrite::default()
            }),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let (_, snapshot_file) = client.remote_profile_snapshot(&uid).await.unwrap();
        assert_eq!(snapshot_file, previous_file);
        assert_eq!(reads.lock().unwrap().as_slice(), ["r-test.yaml"]);

        let mut data = serde_yaml::Mapping::new();
        data.insert("mode".into(), "global".into());
        let prepared = PreparedSubscriptionUpdate::for_test(
            data,
            SubscriptionInfo {
                upload: 10,
                download: 20,
                total: 30,
                expire: 40,
            },
        );
        let fingerprint = NyanpasuClient::remote_profile_fingerprint(&remote).unwrap();

        let error = client
            .commit_refreshed_profile(uid, fingerprint, previous_file.clone(), prepared)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("injected profile state failure"));
        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].0, "r-test.yaml");
        assert!(writes[0].1.contains("mode: global"));
        assert_eq!(writes[1], ("r-test.yaml".to_string(), previous_file));
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn profile_patch_uses_injected_write_port_before_runtime_rebuild() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let patch_commits = Arc::new(Mutex::new(0));
        let client = NyanpasuClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead {
                profiles: Profiles::default(),
            }),
            Arc::new(NoopProfileFs),
            Arc::new(NoopProfilesWrite {
                patch_commits: Some(patch_commits.clone()),
                ..NoopProfilesWrite::default()
            }),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let outcome = client
            .patch_profile("l-test".into(), ProfileBuilder::Local(Default::default()))
            .await
            .unwrap();

        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(*patch_commits.lock().unwrap(), 1);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "refresh-profiles",
                "begin",
                "rebuild",
                "refresh-ui",
                "profile-change"
            ]
        );
    }

    #[tokio::test]
    async fn core_status_is_read_through_the_injected_lifecycle_port() {
        let (client, _) = recording_client(false);
        let snapshot = client.core_status().await.unwrap();
        assert!(matches!(snapshot.state, CoreState::Stopped(None)));
        assert_eq!(snapshot.state_changed_at, 7);
        assert_eq!(snapshot.run_type, RunType::Normal);
    }

    #[tokio::test]
    async fn change_core_runs_through_the_injected_lifecycle_lease() {
        let (client, events) = recording_client(false);
        client.change_core(ClashCore::Mihomo).await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "change-core"]);
    }

    #[tokio::test]
    async fn rebuild_runs_runtime_then_ui_then_profile_side_effects() {
        let (client, events) = recording_client(false);
        client.rebuild_running_config().await.unwrap();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-ui", "profile-change"]
        );
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true);
        let error = client.rebuild_running_config().await.unwrap_err();
        assert!(error.to_string().contains("injected rebuild failure"));
        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "rebuild"]);
    }

    #[tokio::test]
    async fn active_local_profile_file_save_rebuilds_runtime() {
        let profiles = Profiles {
            current: vec!["l-active".into()],
            items: vec![test_local_profile("l-active")],
            ..Profiles::default()
        };
        let (client, events) = recording_client_with_profiles(profiles, false);
        let outcome = client
            .save_profile_file("l-active".into(), "proxies: []\n".into())
            .await
            .unwrap();
        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-ui", "profile-change"]
        );
    }

    #[tokio::test]
    async fn inactive_local_profile_file_save_does_not_rebuild_runtime() {
        let profiles = Profiles {
            current: vec!["l-active".into()],
            items: vec![test_local_profile("l-active"), test_local_profile("l-idle")],
            ..Profiles::default()
        };
        let (client, events) = recording_client_with_profiles(profiles, false);
        let outcome = client
            .save_profile_file("l-idle".into(), "proxies: []\n".into())
            .await
            .unwrap();
        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_commit_rebuild_failure_is_structured_degradation() {
        let (client, events) = recording_client(true);
        let outcome = client.after_profile_runtime_commit("test mutation").await;
        assert!(matches!(outcome, MutationOutcome::CommittedDegraded { .. }));
        assert_eq!(outcome.degradations().len(), 1);
        let degradation = &outcome.degradations()[0];
        assert_eq!(degradation.phase, DegradationPhase::RuntimeBuild);
        assert_eq!(degradation.code, "runtime_rebuild_failed");
        assert!(degradation.retryable);
        assert!(degradation.message.contains("injected rebuild failure"));
        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "rebuild"]);
    }

    #[test]
    fn mutation_outcome_serializes_ref_wire() {
        let applied = MutationOutcome::from_parts((), Vec::new());
        assert_eq!(
            serde_json::to_string(&applied).unwrap(),
            r#"{"status":"applied","value":null}"#
        );
        let degraded = MutationOutcome::from_parts(
            (),
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                code: "runtime_rebuild_failed".into(),
                message: "boom".into(),
                retryable: true,
            }],
        );
        let json = serde_json::to_string(&degraded).unwrap();
        assert!(json.contains(r#""status":"committed_degraded""#));
        assert!(json.contains(r#""phase":"runtime_build""#));
    }
}

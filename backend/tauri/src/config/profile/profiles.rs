use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use indexmap::IndexMap;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use sha2::{Digest, Sha256};

use crate::{
    config::profile::{
        item::{
            MAX_PROFILE_YAML_BYTES, Profile, ProfileMetaGetter, profile_materialized_target_in,
            read_file_bytes_with_limit,
            remote::{
                MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES, RemoteProfileOptions, SubscriptionInfo,
                is_valid_profile_update_interval_minutes,
            },
            shared::validate_profile_uid,
        },
        item_type::ProfileUid,
    },
    utils::dirs,
};

#[derive(Serialize, Deserialize, specta::Type, Clone, Builder, BuilderUpdate)]
#[builder(derive(Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
pub struct Profiles {
    pub current: Vec<ProfileUid>,
    #[serde(default)]
    /// profile list
    pub items: Vec<Profile>,
    #[serde(default)]
    /// record valid fields for clash
    pub valid: Vec<String>,
    /// same as PrfConfig.chain
    pub chain: Vec<ProfileUid>,
}

fn validate_profile_identity(profile: &Profile) -> Result<()> {
    validate_profile_uid(profile.uid())?;
    if profile.file().trim().is_empty() {
        bail!("profile materialized file must not be empty");
    }
    Ok(())
}

fn profile_chain(profile: &Profile) -> &[ProfileUid] {
    match profile {
        Profile::Remote(profile) => &profile.chain,
        Profile::Local(profile) => &profile.chain,
    }
}

fn profile_chain_mut(profile: &mut Profile) -> &mut Vec<ProfileUid> {
    match profile {
        Profile::Remote(profile) => &mut profile.chain,
        Profile::Local(profile) => &mut profile.chain,
    }
}

fn validate_profile_references(
    label: &str,
    owner_uid: Option<&str>,
    references: &[ProfileUid],
    identifiers: &HashSet<&str>,
) -> Result<()> {
    let mut seen = HashSet::with_capacity(references.len());
    for uid in references {
        if !identifiers.contains(uid.as_str()) {
            bail!("{label} profile identifier does not exist: {uid}");
        }
        if owner_uid == Some(uid.as_str()) {
            bail!("profile {uid} cannot reference itself in its chain");
        }
        if !seen.insert(uid.as_str()) {
            bail!("duplicate {label} profile identifier: {uid}");
        }
    }
    Ok(())
}

fn profile_materialized_target_key_in(directory: &Path, file: &str) -> Result<String> {
    let target = profile_materialized_target_in(directory, file)?;
    let key = target.to_string_lossy();

    #[cfg(target_os = "windows")]
    return Ok(key.to_ascii_lowercase());

    #[cfg(not(target_os = "windows"))]
    Ok(key.into_owned())
}

fn invalid_profiles_backup_path(path: &Path, data: &[u8]) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("profiles path has no file name"))?
        .to_string_lossy();
    let digest = hex::encode(Sha256::digest(data));
    Ok(path.with_file_name(format!("{file_name}.invalid-{digest}.bak")))
}

fn verify_existing_profiles_backup(path: &Path, expected: &[u8]) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect profiles backup {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!(
            "profiles backup path is not a regular file: {}",
            path.display()
        );
    }
    if metadata.len() != expected.len() as u64 {
        bail!(
            "profiles backup content does not match its hash: {}",
            path.display()
        );
    }
    let actual = read_file_bytes_with_limit(path, expected.len())
        .with_context(|| format!("failed to read profiles backup {}", path.display()))?;
    if actual != expected {
        bail!(
            "profiles backup content does not match its hash: {}",
            path.display()
        );
    }
    Ok(())
}

fn backup_invalid_profiles_file_with_limit(
    path: &Path,
    max_bytes: usize,
) -> Result<Option<PathBuf>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect profiles file {}", path.display()));
        }
    };
    if !metadata.file_type().is_file() {
        bail!("profiles path is not a regular file: {}", path.display());
    }

    let data = read_file_bytes_with_limit(path, max_bytes)
        .with_context(|| format!("failed to read invalid profiles file {}", path.display()))?;
    let backup = invalid_profiles_backup_path(path, &data)?;
    if backup.exists() {
        verify_existing_profiles_backup(&backup, &data)?;
        return Ok(Some(backup));
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("profiles path has no parent directory"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create profiles backup beside {}", path.display()))?;
    temporary
        .write_all(&data)
        .with_context(|| format!("failed to write profiles backup for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to sync profiles backup for {}", path.display()))?;

    match temporary.persist_noclobber(&backup) {
        Ok(_) => Ok(Some(backup)),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify_existing_profiles_backup(&backup, &data)?;
            Ok(Some(backup))
        }
        Err(error) => Err(error.error)
            .with_context(|| format!("failed to persist profiles backup {}", backup.display())),
    }
}

fn backup_invalid_profiles_file(path: &Path) -> Result<Option<PathBuf>> {
    backup_invalid_profiles_file_with_limit(path, MAX_PROFILE_YAML_BYTES)
}

fn serialize_profiles_with_limit(profiles: &Profiles, max_bytes: usize) -> Result<String> {
    let data = serde_yaml::to_string(profiles).context("failed to serialize profiles config")?;
    let yaml = format!("# Profiles Config for Clash Chimera\n\n{data}");
    if yaml.len() > max_bytes {
        bail!("profiles config exceeds the maximum size of {max_bytes} bytes");
    }
    Ok(yaml)
}

fn save_profiles_to_path_with_limit(
    profiles: &Profiles,
    path: &Path,
    directory: &Path,
    max_bytes: usize,
) -> Result<()> {
    profiles.validate_integrity_in(directory)?;
    let yaml = serialize_profiles_with_limit(profiles, max_bytes)?;
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(yaml.as_bytes()))
        .with_context(|| {
            format!(
                "failed to atomically save profiles config {}",
                path.display()
            )
        })
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            current: vec![],
            chain: vec![],
            valid: vec![
                "dns".into(),
                "unified-delay".into(),
                "tcp-concurrent".into(),
            ],
            items: vec![],
        }
    }
}

impl Profiles {
    fn load_from_path_with_limit(path: &Path, directory: &Path, max_bytes: usize) -> Result<Self> {
        let data = read_file_bytes_with_limit(path, max_bytes)
            .with_context(|| format!("failed to read profiles config {}", path.display()))?;
        let profiles = serde_yaml::from_slice::<Self>(&data)
            .with_context(|| format!("failed to parse profiles config {}", path.display()))?;
        profiles.validate_integrity_in(directory)?;
        Ok(profiles)
    }

    fn load_from_path(path: &Path, directory: &Path) -> Result<Self> {
        Self::load_from_path_with_limit(path, directory, MAX_PROFILE_YAML_BYTES)
    }

    pub fn new() -> Self {
        let path = match dirs::profiles_path() {
            Ok(path) => path,
            Err(error) => {
                log::error!(target: "app", "{error:?}\n - use the default profiles");
                return Self::default();
            }
        };
        let directory = match dirs::app_profiles_dir() {
            Ok(directory) => directory,
            Err(error) => {
                log::error!(target: "app", "{error:?}\n - use the default profiles");
                return Self::default();
            }
        };

        match Self::load_from_path(&path, &directory) {
            Ok(profiles) => profiles,
            Err(error) => {
                match backup_invalid_profiles_file(&path) {
                    Ok(Some(backup)) => log::error!(
                        target: "app",
                        "{error:?}\n - invalid profiles were preserved at {}\n - use the default profiles",
                        backup.display()
                    ),
                    Ok(None) => {
                        log::error!(target: "app", "{error:?}\n - use the default profiles")
                    }
                    Err(backup_error) => log::error!(
                        target: "app",
                        "{error:?}\n - failed to preserve invalid profiles: {backup_error:?}\n - use the default profiles"
                    ),
                }
                Self::default()
            }
        }
    }
    pub fn validate_integrity(&self) -> Result<()> {
        self.validate_integrity_in(&dirs::app_profiles_dir()?)
    }

    fn validate_integrity_in(&self, directory: &Path) -> Result<()> {
        let mut identifiers = HashSet::with_capacity(self.items.len());
        let mut targets = HashSet::with_capacity(self.items.len());
        for profile in &self.items {
            validate_profile_identity(profile)?;
            if !identifiers.insert(profile.uid()) {
                bail!("duplicate profile identifier: {}", profile.uid());
            }
            let target_key = profile_materialized_target_key_in(directory, profile.file())?;
            if !targets.insert(target_key) {
                bail!("duplicate profile materialized target: {}", profile.file());
            }
        }

        validate_profile_references("active", None, &self.current, &identifiers)?;
        validate_profile_references("global chain", None, &self.chain, &identifiers)?;
        for profile in &self.items {
            validate_profile_references(
                "scoped chain",
                Some(profile.uid()),
                profile_chain(profile),
                &identifiers,
            )?;
        }
        Ok(())
    }

    /// Append an item to the in-memory draft.
    /// The surrounding ManagedState transaction owns persistence.
    pub fn append_item(&mut self, item: Profile) -> Result<()> {
        self.append_item_in(item, &dirs::app_profiles_dir()?)
    }

    fn append_item_in(&mut self, item: Profile, directory: &Path) -> Result<()> {
        validate_profile_identity(&item)?;
        let uid = item.uid();
        if self.items.iter().any(|profile| profile.uid() == uid) {
            bail!("duplicate profile identifier: {uid}");
        }

        let target_key = profile_materialized_target_key_in(directory, item.file())?;
        for profile in &self.items {
            if profile_materialized_target_key_in(directory, profile.file())? == target_key {
                bail!("duplicate profile materialized target: {}", item.file());
            }
        }

        self.items.push(item);
        Ok(())
    }

    pub fn save_file(&self) -> Result<()> {
        save_profiles_to_path_with_limit(
            self,
            &dirs::profiles_path()?,
            &dirs::app_profiles_dir()?,
            MAX_PROFILE_YAML_BYTES,
        )
    }

    /// get items ref
    pub fn get_items(&self) -> &[Profile] {
        &self.items
    }

    /// find the item by the uid
    pub fn get_item(&self, uid: &str) -> Result<&Profile> {
        self.get_items()
            .iter()
            .find(|e| e.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))
    }

    pub fn get_current(&self) -> &[ProfileUid] {
        &self.current
    }

    pub fn materialization_affects_current(&self, uid: &str) -> bool {
        let active_profiles = self
            .current
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        active_profiles.contains(uid)
            || self.items.iter().any(|profile| {
                active_profiles.contains(profile.uid())
                    && profile_chain(profile)
                        .iter()
                        .any(|chain_uid| chain_uid == uid)
            })
    }

    pub fn replace_item(&mut self, uid: &str, item: Profile) -> Result<()> {
        self.replace_item_in(uid, item, &dirs::app_profiles_dir()?)
    }

    fn replace_item_in(&mut self, uid: &str, item: Profile, directory: &Path) -> Result<()> {
        validate_profile_identity(&item)?;
        if item.uid() != uid {
            bail!("replacement profile identifier must remain {uid}");
        }

        let target_index = self
            .items
            .iter()
            .position(|profile| profile.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))?;
        let target_key = profile_materialized_target_key_in(directory, item.file())?;
        for (index, profile) in self.items.iter().enumerate() {
            if index != target_index
                && profile_materialized_target_key_in(directory, profile.file())? == target_key
            {
                bail!("duplicate profile materialized target: {}", item.file());
            }
        }

        self.items[target_index] = item;
        Ok(())
    }

    pub fn reorder(&mut self, active_id: &str, over_id: &str) -> Result<()> {
        if active_id == over_id {
            self.get_item(active_id)?;
            return Ok(());
        }

        let active_index = self
            .items
            .iter()
            .position(|profile| profile.uid() == active_id)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{active_id}\""))?;
        let over_index = self
            .items
            .iter()
            .position(|profile| profile.uid() == over_id)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{over_id}\""))?;

        let active = self.items.remove(active_index);
        let target_index = if active_index < over_index {
            over_index - 1
        } else {
            over_index
        };
        self.items.insert(target_index, active);
        Ok(())
    }

    pub fn reorder_by_list(&mut self, list: &[ProfileUid]) -> Result<()> {
        if list.len() != self.items.len() {
            bail!(
                "expected {} profile identifiers, got {}",
                self.items.len(),
                list.len()
            );
        }

        let mut seen = HashSet::with_capacity(list.len());
        for uid in list {
            if !seen.insert(uid.as_str()) {
                bail!("duplicate profile identifier: {uid}");
            }
            self.get_item(uid)?;
        }

        let mut remaining = std::mem::take(&mut self.items);
        self.items = list
            .iter()
            .map(|uid| {
                let index = remaining
                    .iter()
                    .position(|profile| profile.uid() == uid)
                    .expect("profile reorder list was validated");
                remaining.remove(index)
            })
            .collect();
        Ok(())
    }

    pub fn activate(&mut self, uid: Option<&str>) -> Result<()> {
        self.current = match uid {
            Some(uid) => {
                self.get_item(uid)?;
                vec![uid.to_string()]
            }
            None => vec![],
        };
        Ok(())
    }

    pub fn patch_metadata(
        &mut self,
        uid: &str,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> Result<()> {
        let profile = self
            .items
            .iter_mut()
            .find(|profile| profile.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))?;
        let shared = match profile {
            Profile::Remote(profile) => &mut profile.shared,
            Profile::Local(profile) => &mut profile.shared,
        };

        if let Some(name) = name {
            if name.trim().is_empty() {
                bail!("profile name cannot be empty");
            }
            shared.name = name;
        }
        if let Some(desc) = desc {
            shared.desc = desc;
        }
        Ok(())
    }

    pub fn replace_remote_definition(
        &mut self,
        uid: &str,
        file: &str,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
    ) -> Result<()> {
        let profile = self
            .items
            .iter_mut()
            .find(|profile| profile.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))?;
        let Profile::Remote(profile) = profile else {
            bail!("profile \"uid:{uid}\" is not remote");
        };
        if profile.shared.file != file {
            bail!("profile materialized file cannot be changed");
        }

        profile.url = url;
        if let Some(updated_at) = updated_at {
            profile.shared.updated = updated_at;
        }
        if let Some(option) = option {
            profile.option = option;
        }
        if let Some(subscription) = subscription {
            profile.extra = subscription;
        }
        Ok(())
    }

    pub fn patch_remote_options(
        &mut self,
        uid: &str,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> Result<()> {
        let profile = self
            .items
            .iter_mut()
            .find(|profile| profile.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))?;
        let Profile::Remote(profile) = profile else {
            bail!("profile \"uid:{uid}\" is not remote");
        };

        if update_interval_minutes
            .is_some_and(|value| !is_valid_profile_update_interval_minutes(value))
        {
            bail!(
                "profile update interval must be between 1 and {MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES} minutes"
            );
        }

        if let Some(user_agent) = user_agent {
            profile.option.user_agent = user_agent;
        }
        if let Some(with_proxy) = with_proxy {
            profile.option.with_proxy = with_proxy;
        }
        if let Some(self_proxy) = self_proxy {
            profile.option.self_proxy = self_proxy;
        }
        if let Some(update_interval_minutes) = update_interval_minutes {
            profile.option.update_interval_minutes = update_interval_minutes;
        }
        Ok(())
    }

    pub fn delete_item(&mut self, uid: &str) -> Result<(bool, String)> {
        let Some(index) = self.items.iter().position(|profile| profile.uid() == uid) else {
            bail!("failed to get the profile item \"uid:{uid}\"");
        };

        let should_update = self.materialization_affects_current(uid);
        let item = self.items.remove(index);
        let file = item.file().to_string();

        self.current.retain(|current| current != uid);
        self.chain.retain(|chain_uid| chain_uid != uid);
        for profile in &mut self.items {
            profile_chain_mut(profile).retain(|chain_uid| chain_uid != uid);
        }

        Ok((should_update, file))
    }

    /// 获取current指向的配置内容
    pub fn current_mappings(&self) -> Result<IndexMap<&str, Mapping>> {
        let current = self
            .items
            .iter()
            .filter(|e| self.current.iter().any(|uid| uid == e.uid()))
            .collect::<Vec<_>>();
        let (successes, failures): (Vec<(&str, Mapping)>, Vec<anyhow::Error>) = current
            .par_iter()
            .map(|item| item.read_mapping().map(|mapping| (item.uid(), mapping)))
            .partition_map(|item| match item {
                Ok(item) => itertools::Either::Left(item),
                Err(err) => itertools::Either::Right(err),
            });
        if !failures.is_empty() {
            bail!("failed to read the file: {:#?}", failures);
        }
        let map = IndexMap::from_iter(successes);
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        Profiles, backup_invalid_profiles_file, backup_invalid_profiles_file_with_limit,
        invalid_profiles_backup_path, profile_chain, save_profiles_to_path_with_limit,
        serialize_profiles_with_limit,
    };
    use crate::config::profile::item::{
        MAX_PROFILE_FILE_NAME_BYTES, Profile, ProfileMetaGetter,
        local::LocalProfile,
        remote::{
            MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES, RemoteProfile, RemoteProfileOptions,
            SubscriptionInfo,
        },
        shared::{MAX_PROFILE_UID_BYTES, ProfileShared},
    };

    fn local_profile() -> Profile {
        Profile::Local(
            LocalProfile::builder()
                .build()
                .expect("failed to build local profile fixture"),
        )
    }

    fn local_profile_with_uid(uid: &str) -> Profile {
        local_profile_with_uid_and_file(uid, &format!("{uid}.yaml"))
    }

    fn local_profile_with_uid_and_file(uid: &str, file: &str) -> Profile {
        let mut profile = LocalProfile::builder()
            .build()
            .expect("failed to build named local profile fixture");
        profile.shared.uid = uid.to_string();
        profile.shared.file = file.to_string();
        Profile::Local(profile)
    }

    fn with_chain(mut profile: Profile, chain: &[&str]) -> Profile {
        let chain = chain.iter().map(|uid| (*uid).to_string()).collect();
        match &mut profile {
            Profile::Remote(profile) => profile.chain = chain,
            Profile::Local(profile) => profile.chain = chain,
        }
        profile
    }

    fn remote_profile_with_uid(uid: &str) -> Profile {
        Profile::Remote(RemoteProfile {
            url: url::Url::parse("https://example.com/profile.yaml")
                .expect("valid remote profile fixture URL"),
            option: RemoteProfileOptions::default(),
            shared: ProfileShared {
                uid: uid.to_string(),
                name: "Remote Profile".into(),
                file: format!("{uid}.yaml"),
                desc: None,
                updated: 0,
            },
            chain: vec![],
            extra: SubscriptionInfo::default(),
        })
    }

    fn snapshot(profiles: &Profiles) -> String {
        serde_yaml::to_string(profiles).expect("failed to serialize profiles fixture")
    }

    #[test]
    fn profiles_save_atomically_replaces_existing_content() {
        let root = tempdir().expect("failed to create profiles save fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        std::fs::write(&path, "old-content").expect("failed to write existing profiles fixture");
        let profiles = Profiles {
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        save_profiles_to_path_with_limit(&profiles, &path, &directory, 4096)
            .expect("valid profiles config must be saved atomically");

        let saved = std::fs::read_to_string(&path).expect("failed to read saved profiles fixture");
        assert!(saved.starts_with("# Profiles Config for Clash Chimera\n\n"));
        let loaded = Profiles::load_from_path_with_limit(&path, &directory, 4096)
            .expect("atomically saved profiles config must be readable");
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].uid(), "profile-a");
    }

    #[test]
    fn invalid_profiles_save_preserves_existing_content() {
        let root = tempdir().expect("failed to create profiles save fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        std::fs::write(&path, "old-content").expect("failed to write existing profiles fixture");
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("duplicate", "first.yaml"),
                local_profile_with_uid_and_file("duplicate", "second.yaml"),
            ],
            ..Profiles::default()
        };

        save_profiles_to_path_with_limit(&profiles, &path, &directory, 4096)
            .expect_err("invalid profiles config must not replace existing content");

        assert_eq!(
            std::fs::read_to_string(&path).expect("existing profiles fixture must remain readable"),
            "old-content"
        );
    }

    #[test]
    fn oversized_profiles_save_preserves_existing_content() {
        let root = tempdir().expect("failed to create profiles save fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        std::fs::write(&path, "old-content").expect("failed to write existing profiles fixture");
        let profiles = Profiles::default();
        let serialized = serialize_profiles_with_limit(&profiles, usize::MAX)
            .expect("default profiles config must serialize");

        let error =
            save_profiles_to_path_with_limit(&profiles, &path, &directory, serialized.len() - 1)
                .expect_err("oversized profiles config must not replace existing content");

        assert!(error.to_string().contains("exceeds the maximum size"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("existing profiles fixture must remain readable"),
            "old-content"
        );
    }

    #[test]
    fn failed_profiles_atomic_replace_cleans_temporary_files() {
        let root = tempdir().expect("failed to create profiles save fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        std::fs::create_dir(&path).expect("failed to create colliding profiles directory fixture");

        save_profiles_to_path_with_limit(&Profiles::default(), &path, &directory, 4096)
            .expect_err("atomic profiles save over a directory must fail");

        let entries = std::fs::read_dir(root.path())
            .expect("failed to list profiles save fixture directory")
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&std::ffi::OsString::from("profiles")));
        assert!(entries.contains(&std::ffi::OsString::from("profiles.yaml")));
        assert!(path.is_dir());
    }

    #[test]
    fn oversized_profiles_config_is_rejected_before_parsing() {
        let root = tempdir().expect("failed to create profiles size fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        std::fs::write(&path, "12345").expect("failed to write oversized profiles fixture");

        let error = match Profiles::load_from_path_with_limit(&path, &directory, 4) {
            Ok(_) => panic!("oversized profiles config must be rejected before parsing"),
            Err(error) => error,
        };

        assert!(format!("{error:#}").contains("maximum size of 4 bytes"));
    }

    #[test]
    fn oversized_invalid_profiles_file_is_not_loaded_for_backup() {
        let root = tempdir().expect("failed to create profiles size fixture directory");
        let path = root.path().join("profiles.yaml");
        let original = b"12345";
        std::fs::write(&path, original).expect("failed to write oversized profiles fixture");

        let error = backup_invalid_profiles_file_with_limit(&path, 4)
            .expect_err("oversized invalid profiles file must not be loaded for backup");

        assert!(format!("{error:#}").contains("maximum size of 4 bytes"));
        assert_eq!(
            std::fs::read(&path).expect("oversized profiles source must remain readable"),
            original
        );
        assert_eq!(
            std::fs::read_dir(root.path())
                .expect("failed to list oversized profiles fixture directory")
                .filter_map(std::result::Result::ok)
                .count(),
            1
        );
    }

    #[test]
    fn profiles_config_at_exact_limit_reaches_yaml_validation() {
        let root = tempdir().expect("failed to create profiles size fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        let invalid = b"1234";
        std::fs::write(&path, invalid).expect("failed to write exact-limit profiles fixture");

        let error = match Profiles::load_from_path_with_limit(&path, &directory, 4) {
            Ok(_) => panic!("invalid exact-limit profiles config must reach YAML parsing"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("failed to parse profiles config")
        );
        assert!(!error.to_string().contains("maximum size"));
    }

    #[test]
    fn invalid_profiles_yaml_is_backed_up_idempotently() {
        let root = tempdir().expect("failed to create profiles recovery fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        let invalid = b"items: [not-valid";
        std::fs::write(&path, invalid).expect("failed to write invalid profiles fixture");

        assert!(Profiles::load_from_path(&path, &directory).is_err());
        let first = backup_invalid_profiles_file(&path)
            .expect("failed to preserve invalid profiles fixture")
            .expect("invalid profiles fixture must create a backup");
        let second = backup_invalid_profiles_file(&path)
            .expect("failed to reuse invalid profiles backup")
            .expect("invalid profiles fixture backup must remain available");

        assert_eq!(first, second);
        assert_eq!(
            std::fs::read(&first).expect("failed to read invalid profiles backup"),
            invalid
        );
        assert_eq!(
            std::fs::read(&path).expect("invalid profiles source must remain readable"),
            invalid
        );
        let backup_count = std::fs::read_dir(root.path())
            .expect("failed to list profiles recovery fixtures")
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("profiles.yaml.invalid-")
            })
            .count();
        assert_eq!(backup_count, 1);
    }

    #[test]
    fn integrity_invalid_profiles_are_preserved_before_default_fallback() {
        let root = tempdir().expect("failed to create profiles recovery fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("duplicate", "first.yaml"),
                local_profile_with_uid_and_file("duplicate", "second.yaml"),
            ],
            ..Profiles::default()
        };
        let serialized = snapshot(&profiles);
        std::fs::write(&path, &serialized).expect("failed to write integrity-invalid fixture");

        let error = match Profiles::load_from_path(&path, &directory) {
            Ok(_) => panic!("duplicate profile identifiers must fail loading"),
            Err(error) => error,
        };
        let backup = backup_invalid_profiles_file(&path)
            .expect("failed to preserve integrity-invalid profiles")
            .expect("integrity-invalid profiles must create a backup");

        assert!(error.to_string().contains("duplicate profile identifier"));
        assert_eq!(
            std::fs::read_to_string(backup)
                .expect("failed to read integrity-invalid profiles backup"),
            serialized
        );
    }

    #[test]
    fn oversized_historical_profile_uid_is_preserved_before_default_fallback() {
        let root = tempdir().expect("failed to create oversized UID recovery fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        let oversized_uid = "u".repeat(MAX_PROFILE_UID_BYTES + 1);
        let profiles = Profiles {
            items: vec![local_profile_with_uid_and_file(
                &oversized_uid,
                "oversized-uid.yaml",
            )],
            ..Profiles::default()
        };
        let serialized = snapshot(&profiles);
        std::fs::write(&path, &serialized).expect("failed to write oversized UID fixture");

        let error = match Profiles::load_from_path(&path, &directory) {
            Ok(_) => panic!("oversized historical profile UID must fail loading"),
            Err(error) => error,
        };
        let backup = backup_invalid_profiles_file(&path)
            .expect("failed to preserve oversized UID profiles")
            .expect("oversized UID profiles must create a backup");

        assert!(format!("{error:#}").contains("maximum size"));
        assert_eq!(
            std::fs::read_to_string(backup).expect("failed to read oversized UID profiles backup"),
            serialized
        );
        assert_eq!(
            std::fs::read_to_string(&path)
                .expect("oversized UID profiles source must remain readable"),
            serialized
        );
    }

    #[test]
    fn oversized_historical_profile_file_name_is_preserved_before_default_fallback() {
        let root = tempdir().expect("failed to create oversized file recovery fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        let oversized_file = format!("{}.yaml", "f".repeat(MAX_PROFILE_FILE_NAME_BYTES));
        let profiles = Profiles {
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                &oversized_file,
            )],
            ..Profiles::default()
        };
        let serialized = snapshot(&profiles);
        std::fs::write(&path, &serialized).expect("failed to write oversized file fixture");

        let error = match Profiles::load_from_path(&path, &directory) {
            Ok(_) => panic!("oversized historical profile file name must fail loading"),
            Err(error) => error,
        };
        let backup = backup_invalid_profiles_file(&path)
            .expect("failed to preserve oversized file profiles")
            .expect("oversized file profiles must create a backup");

        assert!(format!("{error:#}").contains("profile file name exceeds"));
        assert_eq!(
            std::fs::read_to_string(backup).expect("failed to read oversized file profiles backup"),
            serialized
        );
        assert_eq!(
            std::fs::read_to_string(&path)
                .expect("oversized file profiles source must remain readable"),
            serialized
        );
    }

    #[test]
    fn existing_mismatched_profiles_backup_is_never_overwritten() {
        let root = tempdir().expect("failed to create profiles recovery fixture directory");
        let path = root.path().join("profiles.yaml");
        let invalid = b"invalid: [yaml";
        std::fs::write(&path, invalid).expect("failed to write invalid profiles fixture");
        let backup = invalid_profiles_backup_path(&path, invalid)
            .expect("failed to derive invalid profiles backup path");
        std::fs::write(&backup, b"different-content")
            .expect("failed to write mismatched profiles backup fixture");

        let error = backup_invalid_profiles_file(&path)
            .expect_err("mismatched existing backup must be rejected");

        assert!(error.to_string().contains("does not match its hash"));
        assert_eq!(
            std::fs::read(&backup).expect("mismatched backup must remain readable"),
            b"different-content"
        );
        assert_eq!(
            std::fs::read(&path).expect("invalid source must not be modified"),
            invalid
        );
    }

    #[test]
    fn missing_profiles_file_does_not_create_a_backup() {
        let root = tempdir().expect("failed to create profiles recovery fixture directory");
        let path = root.path().join("missing-profiles.yaml");

        assert_eq!(
            backup_invalid_profiles_file(&path)
                .expect("missing profiles backup check must succeed"),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profiles_backup_rejects_a_symlink_source_without_touching_its_target() {
        use std::os::windows::fs::symlink_file;

        let root = tempdir().expect("failed to create profiles recovery fixture directory");
        let external = root.path().join("external.yaml");
        std::fs::write(&external, "external: true")
            .expect("failed to write external profiles fixture");
        let path = root.path().join("profiles.yaml");
        symlink_file(&external, &path).expect("failed to create profiles symlink fixture");

        let error = backup_invalid_profiles_file(&path)
            .expect_err("profiles symlink source must not be backed up");

        assert!(error.to_string().contains("not a regular file"));
        assert_eq!(
            std::fs::read_to_string(&external)
                .expect("external profiles target must remain readable"),
            "external: true"
        );
    }

    #[test]
    fn append_item_only_mutates_the_draft_collection() {
        let mut profiles = Profiles::default();

        profiles
            .append_item(local_profile())
            .expect("unique profile fixture must be appended");

        assert_eq!(profiles.items.len(), 1);
    }

    #[test]
    fn profile_integrity_accepts_unique_non_empty_identity_data() {
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        profiles
            .validate_integrity()
            .expect("unique profile identity data must be valid");
    }

    #[test]
    fn direct_active_profile_materialization_affects_current_config() {
        let profiles = Profiles {
            current: vec!["profile-a".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        assert!(profiles.materialization_affects_current("profile-a"));
    }

    #[test]
    fn active_scoped_chain_materialization_affects_current_config() {
        let profiles = Profiles {
            current: vec!["profile-a".into()],
            items: vec![
                with_chain(
                    local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                    &["profile-b"],
                ),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        assert!(profiles.materialization_affects_current("profile-b"));
    }

    #[test]
    fn inactive_or_missing_materialization_does_not_affect_current_config() {
        let profiles = Profiles {
            current: vec!["profile-a".into()],
            items: vec![
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                with_chain(
                    local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
                    &["profile-c"],
                ),
                local_profile_with_uid_and_file("profile-c", "profile-c.yaml"),
            ],
            ..Profiles::default()
        };

        assert!(!profiles.materialization_affects_current("profile-c"));
        assert!(!profiles.materialization_affects_current("missing"));
    }

    #[test]
    fn profile_integrity_accepts_multiple_unique_active_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            current: vec!["profile-a".into(), "profile-b".into()],
            items: vec![
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        profiles
            .validate_integrity_in(directory.path())
            .expect("unique active references must be valid");
    }

    #[test]
    fn profile_integrity_rejects_missing_active_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            current: vec!["missing".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("missing active reference must be rejected");

        assert!(error.to_string().contains("does not exist: missing"));
    }

    #[test]
    fn profile_integrity_rejects_duplicate_active_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            current: vec!["profile-a".into(), "profile-a".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("duplicate active references must be rejected");

        assert!(
            error
                .to_string()
                .contains("duplicate active profile identifier")
        );
    }

    #[test]
    fn missing_active_reference_is_preserved_in_recovery_backup() {
        let root = tempdir().expect("failed to create profiles recovery fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profile materialization fixture");
        let path = root.path().join("profiles.yaml");
        let profiles = Profiles {
            current: vec!["missing".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };
        let serialized = snapshot(&profiles);
        std::fs::write(&path, &serialized)
            .expect("failed to write invalid active reference fixture");

        let error = match Profiles::load_from_path(&path, &directory) {
            Ok(_) => panic!("missing active reference must fail loading"),
            Err(error) => error,
        };
        let backup = backup_invalid_profiles_file(&path)
            .expect("failed to preserve profiles with missing active reference")
            .expect("missing active reference must create a backup");

        assert!(error.to_string().contains("does not exist: missing"));
        assert_eq!(
            std::fs::read_to_string(backup)
                .expect("failed to read missing active reference backup"),
            serialized
        );
    }

    #[test]
    fn profile_integrity_accepts_valid_global_and_scoped_chains() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            chain: vec!["profile-b".into()],
            items: vec![
                with_chain(
                    local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                    &["profile-b"],
                ),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        profiles
            .validate_integrity_in(directory.path())
            .expect("valid global and scoped chain references must be accepted");
    }

    #[test]
    fn profile_integrity_rejects_missing_global_chain_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            chain: vec!["missing".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("missing global chain reference must be rejected");

        assert!(error.to_string().contains("global chain"));
        assert!(error.to_string().contains("does not exist: missing"));
    }

    #[test]
    fn profile_integrity_rejects_duplicate_global_chain_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            chain: vec!["profile-a".into(), "profile-a".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("duplicate global chain references must be rejected");

        assert!(error.to_string().contains("duplicate global chain"));
    }

    #[test]
    fn profile_integrity_rejects_missing_scoped_chain_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            items: vec![with_chain(
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                &["missing"],
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("missing scoped chain reference must be rejected");

        assert!(error.to_string().contains("scoped chain"));
        assert!(error.to_string().contains("does not exist: missing"));
    }

    #[test]
    fn profile_integrity_rejects_duplicate_scoped_chain_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            items: vec![
                with_chain(
                    local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                    &["profile-b", "profile-b"],
                ),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("duplicate scoped chain references must be rejected");

        assert!(error.to_string().contains("duplicate scoped chain"));
    }

    #[test]
    fn profile_integrity_rejects_scoped_chain_self_references() {
        let directory = tempdir().expect("failed to create profile integrity fixture directory");
        let profiles = Profiles {
            items: vec![with_chain(
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                &["profile-a"],
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("scoped chain self-reference must be rejected");

        assert!(error.to_string().contains("cannot reference itself"));
    }

    #[test]
    fn profile_integrity_rejects_duplicate_identifiers() {
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                local_profile_with_uid_and_file("profile-a", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity()
            .expect_err("duplicate profile identifiers must be rejected");
        assert!(error.to_string().contains("duplicate profile identifier"));
    }

    #[test]
    fn profile_integrity_rejects_duplicate_materialized_files() {
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "shared.yaml"),
                local_profile_with_uid_and_file("profile-b", "shared.yaml"),
            ],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity()
            .expect_err("duplicate materialized files must be rejected");
        assert!(
            error
                .to_string()
                .contains("duplicate profile materialized target")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_integrity_rejects_distinct_links_to_the_same_target() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("failed to create profiles fixture directory");
        let target = directory.path().join("shared.yaml");
        std::fs::write(&target, "proxies: []")
            .expect("failed to create shared profile target fixture");
        symlink_file(&target, directory.path().join("first.yaml"))
            .expect("failed to create first profile link fixture");
        symlink_file(&target, directory.path().join("second.yaml"))
            .expect("failed to create second profile link fixture");
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "first.yaml"),
                local_profile_with_uid_and_file("profile-b", "second.yaml"),
            ],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(directory.path())
            .expect_err("links resolving to the same target must be rejected");

        assert!(
            error
                .to_string()
                .contains("duplicate profile materialized target")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_integrity_accepts_links_to_distinct_internal_targets() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("failed to create profiles fixture directory");
        for name in ["first-target.yaml", "second-target.yaml"] {
            std::fs::write(directory.path().join(name), "proxies: []")
                .expect("failed to create distinct profile target fixture");
        }
        symlink_file(
            directory.path().join("first-target.yaml"),
            directory.path().join("first.yaml"),
        )
        .expect("failed to create first distinct profile link fixture");
        symlink_file(
            directory.path().join("second-target.yaml"),
            directory.path().join("second.yaml"),
        )
        .expect("failed to create second distinct profile link fixture");
        let profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "first.yaml"),
                local_profile_with_uid_and_file("profile-b", "second.yaml"),
            ],
            ..Profiles::default()
        };

        profiles
            .validate_integrity_in(directory.path())
            .expect("links to distinct internal targets must remain valid");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_integrity_rejects_a_link_to_an_external_target() {
        use std::os::windows::fs::symlink_file;

        let root = tempdir().expect("failed to create profile root fixture directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("failed to create profiles fixture directory");
        let external = root.path().join("outside.yaml");
        std::fs::write(&external, "proxies: []")
            .expect("failed to create external profile target fixture");
        symlink_file(&external, directory.join("external.yaml"))
            .expect("failed to create external profile link fixture");
        let profiles = Profiles {
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "external.yaml",
            )],
            ..Profiles::default()
        };

        let error = profiles
            .validate_integrity_in(&directory)
            .expect_err("external profile target must be rejected");

        assert!(error.to_string().contains("escapes the profiles directory"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn append_item_rejects_a_link_alias_without_mutating_state() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("failed to create profiles fixture directory");
        let target = directory.path().join("shared.yaml");
        std::fs::write(&target, "proxies: []").expect("failed to create append target fixture");
        symlink_file(&target, directory.path().join("alias.yaml"))
            .expect("failed to create append alias fixture");
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid_and_file("profile-a", "shared.yaml")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        profiles
            .append_item_in(
                local_profile_with_uid_and_file("profile-b", "alias.yaml"),
                directory.path(),
            )
            .expect_err("alias to an existing profile target must be rejected");

        assert_eq!(snapshot(&profiles), before);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn replace_item_rejects_a_link_alias_without_mutating_state() {
        use std::os::windows::fs::symlink_file;

        let directory = tempdir().expect("failed to create profiles fixture directory");
        let shared = directory.path().join("shared.yaml");
        std::fs::write(&shared, "proxies: []")
            .expect("failed to create replacement target fixture");
        std::fs::write(directory.path().join("second.yaml"), "proxies: []")
            .expect("failed to create second replacement target fixture");
        symlink_file(&shared, directory.path().join("alias.yaml"))
            .expect("failed to create replacement alias fixture");
        let mut profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "shared.yaml"),
                local_profile_with_uid_and_file("profile-b", "second.yaml"),
            ],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        profiles
            .replace_item_in(
                "profile-b",
                local_profile_with_uid_and_file("profile-b", "alias.yaml"),
                directory.path(),
            )
            .expect_err("replacement alias to another profile target must be rejected");

        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn profile_integrity_rejects_invalid_identifiers_and_empty_files() {
        for profile in [
            local_profile_with_uid_and_file("", "profile.yaml"),
            local_profile_with_uid_and_file("   ", "profile.yaml"),
            local_profile_with_uid_and_file(" profile-a", "profile.yaml"),
            local_profile_with_uid_and_file("profile-a ", "profile.yaml"),
            local_profile_with_uid_and_file("profile\nnext", "profile.yaml"),
            local_profile_with_uid_and_file("profile\tnext", "profile.yaml"),
            local_profile_with_uid_and_file("profile\0next", "profile.yaml"),
            local_profile_with_uid_and_file("profile-a", ""),
            local_profile_with_uid_and_file("profile-a", " \t"),
        ] {
            let profiles = Profiles {
                items: vec![profile],
                ..Profiles::default()
            };
            profiles
                .validate_integrity()
                .expect_err("invalid profile identity fields must be rejected");
        }
    }

    #[test]
    fn replace_item_rejects_identifier_changes_without_mutating_state() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid("profile-a")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        profiles
            .replace_item("profile-a", local_profile_with_uid("profile-b"))
            .expect_err("replacement must retain the target identifier");

        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn replace_item_rejects_materialized_file_collisions_without_mutation() {
        let mut profiles = Profiles {
            items: vec![
                local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        profiles
            .replace_item(
                "profile-a",
                local_profile_with_uid_and_file("profile-a", "profile-b.yaml"),
            )
            .expect_err("replacement materialized file collision must be rejected");

        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn replace_item_accepts_the_same_identity_with_distinct_materialization() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };
        let mut replacement = local_profile_with_uid_and_file("profile-a", "profile-a.yaml");
        let Profile::Local(profile) = &mut replacement else {
            panic!("local replacement fixture changed type");
        };
        profile.shared.name = "Changed".into();

        profiles
            .replace_item("profile-a", replacement)
            .expect("same profile identity replacement must succeed");

        let Profile::Local(profile) = &profiles.items[0] else {
            panic!("stored replacement changed type");
        };
        assert_eq!(profile.shared.name, "Changed");
    }

    #[test]
    fn append_item_rejects_a_duplicate_identifier_without_mutating_state() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid("profile-a")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        let error = profiles
            .append_item(local_profile_with_uid_and_file(
                "profile-a",
                "different-file.yaml",
            ))
            .expect_err("duplicate profile identifier must be rejected");

        assert!(error.to_string().contains("duplicate profile identifier"));
        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn append_item_rejects_a_duplicate_materialized_file_without_mutating_state() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid_and_file("profile-a", "shared.yaml")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        let error = profiles
            .append_item(local_profile_with_uid_and_file("profile-b", "shared.yaml"))
            .expect_err("duplicate materialized file must be rejected");

        assert!(
            error
                .to_string()
                .contains("duplicate profile materialized target")
        );
        assert_eq!(snapshot(&profiles), before);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn append_item_rejects_case_only_materialized_file_collisions_on_windows() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid_and_file("profile-a", "Profile.YAML")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        profiles
            .append_item(local_profile_with_uid_and_file("profile-b", "profile.yaml"))
            .expect_err("Windows materialized file comparison must be case-insensitive");

        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn append_item_accepts_distinct_identifiers_and_materialized_files() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };

        profiles
            .append_item(local_profile_with_uid_and_file(
                "profile-b",
                "profile-b.yaml",
            ))
            .expect("distinct profile identity and file must be accepted");

        assert_eq!(profiles.items.len(), 2);
    }

    #[test]
    fn reorder_same_existing_profile_is_a_valid_noop() {
        let profile = local_profile();
        let uid = profile.uid().to_string();
        let mut profiles = Profiles {
            items: vec![profile],
            ..Profiles::default()
        };

        profiles
            .reorder(&uid, &uid)
            .expect("same existing profile reorder must be accepted");

        assert_eq!(profiles.items[0].uid(), uid);
    }

    #[test]
    fn reorder_same_missing_profile_is_rejected() {
        let mut profiles = Profiles::default();

        let error = profiles
            .reorder("missing", "missing")
            .expect_err("same missing profile reorder must not silently succeed");

        assert!(error.to_string().contains("uid:missing"));
        assert!(profiles.items.is_empty());
    }

    #[test]
    fn failed_reorder_preserves_the_original_order() {
        let first = local_profile();
        let second = local_profile();
        let expected = vec![first.uid().to_string(), second.uid().to_string()];
        let mut profiles = Profiles {
            items: vec![first, second],
            ..Profiles::default()
        };

        profiles
            .reorder(&expected[0], "missing")
            .expect_err("missing reorder target must be rejected");

        assert_eq!(
            profiles
                .items
                .iter()
                .map(|profile| profile.uid().to_string())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn failed_activation_preserves_the_previous_current_profile() {
        let profile = local_profile();
        let uid = profile.uid().to_string();
        let mut profiles = Profiles {
            current: vec![uid.clone()],
            items: vec![profile],
            ..Profiles::default()
        };

        profiles
            .activate(Some("missing"))
            .expect_err("missing activation target must be rejected");

        assert_eq!(profiles.current, vec![uid]);
    }

    #[test]
    fn invalid_reorder_list_preserves_profile_order() {
        let mut profiles = Profiles {
            items: vec![
                local_profile_with_uid("profile-a"),
                local_profile_with_uid("profile-b"),
            ],
            ..Profiles::default()
        };

        for invalid_order in [
            vec!["profile-a".into(), "profile-a".into()],
            vec!["profile-a".into(), "missing-profile".into()],
            vec!["profile-a".into()],
        ] {
            let before = snapshot(&profiles);
            profiles
                .reorder_by_list(&invalid_order)
                .expect_err("invalid complete order must be rejected");
            assert_eq!(snapshot(&profiles), before);
        }
    }

    #[test]
    fn empty_metadata_name_patch_preserves_profile_state() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid("profile-a")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        let error = profiles
            .patch_metadata(
                "profile-a",
                Some("   ".into()),
                Some(Some("changed".into())),
            )
            .expect_err("blank profile name must reject the complete metadata patch");

        assert!(error.to_string().contains("name cannot be empty"));
        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn remote_definition_replacement_on_local_profile_preserves_state() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid("profile-a")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        let error = profiles
            .replace_remote_definition(
                "profile-a",
                "profile-a.yaml",
                Some(42),
                url::Url::parse("https://example.com/profile.yaml")
                    .expect("valid remote profile fixture URL"),
                None,
                None,
            )
            .expect_err("remote definition must not replace a local profile");

        assert!(error.to_string().contains("is not remote"));
        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn invalid_remote_update_interval_preserves_all_remote_options() {
        for interval in [0, MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES + 1] {
            let mut profiles = Profiles {
                items: vec![remote_profile_with_uid("profile-a")],
                ..Profiles::default()
            };
            let before = snapshot(&profiles);

            let error = profiles
                .patch_remote_options(
                    "profile-a",
                    Some(Some("changed-agent".into())),
                    Some(true),
                    Some(true),
                    Some(interval),
                )
                .expect_err("unsafe update interval must reject the complete option patch");

            assert!(error.to_string().contains("must be between 1"));
            assert_eq!(snapshot(&profiles), before);
        }
    }

    #[test]
    fn valid_remote_option_patch_updates_all_requested_fields() {
        let mut profiles = Profiles {
            items: vec![remote_profile_with_uid("profile-a")],
            ..Profiles::default()
        };

        profiles
            .patch_remote_options(
                "profile-a",
                Some(Some("changed-agent".into())),
                Some(true),
                Some(true),
                Some(45),
            )
            .expect("valid remote option patch must succeed");

        let Profile::Remote(profile) = profiles.get_item("profile-a").unwrap() else {
            panic!("remote fixture changed profile type");
        };
        assert_eq!(profile.option.user_agent.as_deref(), Some("changed-agent"));
        assert!(profile.option.with_proxy);
        assert!(profile.option.self_proxy);
        assert_eq!(profile.option.update_interval_minutes, 45);
    }

    #[test]
    fn remote_option_patch_on_local_profile_preserves_state() {
        let mut profiles = Profiles {
            items: vec![local_profile_with_uid("profile-a")],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        let error = profiles
            .patch_remote_options(
                "profile-a",
                Some(Some("test-agent".into())),
                Some(true),
                Some(true),
                Some(30),
            )
            .expect_err("remote options must not be applied to a local profile");

        assert!(error.to_string().contains("is not remote"));
        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn deleting_a_profile_used_by_an_active_scoped_chain_requires_rebuild() {
        let mut profiles = Profiles {
            current: vec!["profile-a".into()],
            items: vec![
                with_chain(
                    local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                    &["profile-b"],
                ),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
            ],
            ..Profiles::default()
        };

        let (should_update, deferred_file) = profiles
            .delete_item("profile-b")
            .expect("active scoped chain target deletion must succeed");

        assert!(should_update);
        assert_eq!(deferred_file, "profile-b.yaml");
        assert_eq!(profiles.items.len(), 1);
        assert!(profile_chain(&profiles.items[0]).is_empty());
        assert_eq!(profiles.current, vec!["profile-a"]);
    }

    #[test]
    fn deleting_a_profile_used_only_by_an_inactive_chain_cleans_reference_without_rebuild() {
        let mut profiles = Profiles {
            current: vec!["profile-c".into()],
            chain: vec!["profile-b".into()],
            items: vec![
                with_chain(
                    local_profile_with_uid_and_file("profile-a", "profile-a.yaml"),
                    &["profile-b"],
                ),
                local_profile_with_uid_and_file("profile-b", "profile-b.yaml"),
                local_profile_with_uid_and_file("profile-c", "profile-c.yaml"),
            ],
            ..Profiles::default()
        };

        let (should_update, _) = profiles
            .delete_item("profile-b")
            .expect("inactive scoped chain target deletion must succeed");

        assert!(!should_update);
        assert!(profiles.chain.is_empty());
        let profile_a = profiles
            .get_item("profile-a")
            .expect("referencing inactive profile must remain");
        assert!(profile_chain(profile_a).is_empty());
        assert_eq!(profiles.current, vec!["profile-c"]);
    }

    #[test]
    fn deleting_a_missing_profile_preserves_all_state() {
        let mut profiles = Profiles {
            current: vec!["profile-a".into()],
            chain: vec!["profile-a".into()],
            items: vec![local_profile_with_uid_and_file(
                "profile-a",
                "profile-a.yaml",
            )],
            ..Profiles::default()
        };
        let before = snapshot(&profiles);

        let error = profiles
            .delete_item("missing")
            .expect_err("missing profile deletion must be rejected");

        assert!(error.to_string().contains("uid:missing"));
        assert_eq!(snapshot(&profiles), before);
    }

    #[test]
    fn delete_item_only_mutates_state_and_returns_deferred_cleanup() {
        let profile = local_profile();
        let uid = profile.uid().to_string();
        let file = profile.file().to_string();
        let mut profiles = Profiles {
            current: vec![uid.clone()],
            chain: vec![uid.clone()],
            items: vec![profile],
            ..Profiles::default()
        };

        let (should_update, deferred_file) = profiles
            .delete_item(&uid)
            .expect("failed to delete profile fixture from draft");

        assert!(should_update);
        assert_eq!(deferred_file, file);
        assert!(profiles.items.is_empty());
        assert!(profiles.current.is_empty());
        assert!(profiles.chain.is_empty());
    }
}

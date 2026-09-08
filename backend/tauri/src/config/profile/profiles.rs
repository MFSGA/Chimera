use std::collections::HashSet;

use anyhow::{Result, bail};
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use indexmap::IndexMap;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::{
    config::profile::{
        item::{
            Profile, ProfileKindGetter, ProfileMetaGetter,
            remote::{RemoteProfileOptions, SubscriptionInfo},
            utils::resolve_managed_profile_path,
        },
        item_type::ProfileUid,
    },
    utils::{dirs, help},
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
    pub fn new() -> Self {
        match dirs::profiles_path().and_then(|path| help::read_yaml::<Self, _>(&path)) {
            Ok(profiles) => {
                profiles.reconcile_reservations();
                profiles
            }
            Err(err) => {
                log::error!(target: "app", "{err:?}\n - use the default profiles");
                Self::default()
            }
        }
    }

    fn reconcile_reservations(&self) {
        let committed_files = self
            .items
            .iter()
            .map(|profile| profile.file().to_string())
            .collect::<HashSet<_>>();
        let result = dirs::app_profiles_dir().and_then(|root| {
            super::reservation_reconcile::reconcile_reservations(&root, &committed_files)
        });
        match result {
            Ok(report) => {
                if report.removed_reservations > 0 || report.removed_materializations > 0 {
                    log::info!(
                        target: "app",
                        "reconciled profile reservations: removed {} reservations and {} materializations",
                        report.removed_reservations,
                        report.removed_materializations
                    );
                }
                for degradation in report.degradations {
                    log::warn!(
                        target: "app",
                        "profile reservation reconciliation deferred for {}: {}",
                        degradation.path.display(),
                        degradation.reason
                    );
                }
            }
            Err(error) => {
                log::warn!(target: "app", "profile reservation reconciliation skipped: {error:#}");
            }
        }
    }

    /// append new item
    pub fn append_item(&mut self, item: Profile) -> Result<()> {
        self.items.push(item);
        Ok(())
    }

    pub fn save_file(&self) -> Result<()> {
        help::save_yaml(
            &dirs::profiles_path()?,
            self,
            Some("# Profiles Config for Clash Chimera"),
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

    pub fn replace_item(&mut self, uid: &str, item: Profile) -> Result<()> {
        let target = self
            .items
            .iter_mut()
            .find(|profile| profile.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))?;

        *target = item;
        Ok(())
    }

    pub fn reorder(&mut self, active_id: &str, over_id: &str) -> Result<()> {
        if active_id == over_id {
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
                let profile = self.get_item(uid)?;
                if !profile.kind().is_config() {
                    bail!("profile \"uid:{uid}\" is not activatable");
                }
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
            Profile::Merge(profile) => &mut profile.shared,
            Profile::Script(profile) => &mut profile.shared,
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

    pub fn replace_remote_definition_with_transforms(
        &mut self,
        uid: &str,
        file: &str,
        updated_at: Option<usize>,
        url: url::Url,
        option: Option<RemoteProfileOptions>,
        subscription: Option<SubscriptionInfo>,
        transforms: Vec<ProfileUid>,
    ) -> Result<bool> {
        self.validate_transform_chain(&transforms)?;
        let affects_current = self.current.iter().any(|current| current == uid);
        self.replace_remote_definition(uid, file, updated_at, url, option, subscription)?;
        self.set_profile_transform_chain(uid, transforms)?;
        Ok(affects_current)
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
            if update_interval_minutes < 1 {
                bail!("profile update interval must be at least one minute");
            }
            profile.option.update_interval_minutes = update_interval_minutes;
        }
        Ok(())
    }

    fn validate_transform_chain(&self, transforms: &[ProfileUid]) -> Result<()> {
        let mut seen = HashSet::with_capacity(transforms.len());
        for transform_uid in transforms {
            if !seen.insert(transform_uid) {
                bail!("duplicate transform profile `{transform_uid}` in chain");
            }
            let transform = self.get_item(transform_uid)?;
            let kind = transform.kind();
            if !kind.is_transform() {
                bail!("profile `{transform_uid}` is not a transform profile");
            }
            if !kind.is_runtime_transform_supported() {
                bail!(
                    "transform profile `{transform_uid}` cannot run in this build because its runtime is unavailable"
                );
            }
        }
        Ok(())
    }

    pub fn set_profile_transform_chain(
        &mut self,
        uid: &str,
        transforms: Vec<ProfileUid>,
    ) -> Result<bool> {
        self.validate_transform_chain(&transforms)?;
        let affects_current = self.current.iter().any(|current| current == uid);
        let profile = self
            .items
            .iter_mut()
            .find(|profile| profile.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))?;
        match profile {
            Profile::Local(profile) => profile.chain = transforms,
            Profile::Remote(profile) => profile.chain = transforms,
            Profile::Merge(_) | Profile::Script(_) => {
                bail!("transform profile `{uid}` cannot own a transform chain")
            }
        }
        Ok(affects_current)
    }

    pub fn set_global_transform_chain(&mut self, transforms: Vec<ProfileUid>) -> Result<bool> {
        self.validate_transform_chain(&transforms)?;
        if self.chain == transforms {
            return Ok(false);
        }
        self.chain = transforms;
        Ok(true)
    }

    pub fn delete_item(&mut self, uid: &str) -> Result<bool> {
        let Some(index) = self.items.iter().position(|profile| profile.uid() == uid) else {
            bail!("failed to get the profile item \"uid:{uid}\"");
        };

        let should_update = self.is_runtime_relevant(uid);
        self.items.remove(index);

        self.current.retain(|current| current != uid);
        self.chain.retain(|chain_uid| chain_uid != uid);
        for profile in &mut self.items {
            match profile {
                Profile::Local(profile) => profile.chain.retain(|chain_uid| chain_uid != uid),
                Profile::Remote(profile) => profile.chain.retain(|chain_uid| chain_uid != uid),
                Profile::Merge(_) | Profile::Script(_) => {}
            }
        }

        Ok(should_update)
    }

    pub fn is_runtime_relevant(&self, uid: &str) -> bool {
        if self.current.iter().any(|current| current == uid)
            || self.chain.iter().any(|transform| transform == uid)
        {
            return true;
        }

        self.items.iter().any(|profile| {
            if !self.current.iter().any(|current| current == profile.uid()) {
                return false;
            }
            match profile {
                Profile::Local(profile) => profile.chain.iter().any(|transform| transform == uid),
                Profile::Remote(profile) => profile.chain.iter().any(|transform| transform == uid),
                Profile::Merge(_) | Profile::Script(_) => false,
            }
        })
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
            .map(|item| {
                let file_path = resolve_managed_profile_path(item.file())?;
                if !file_path.exists() {
                    return Err(anyhow::anyhow!("failed to find the file: {:?}", file_path));
                }
                help::read_merge_mapping(&file_path).map(|mapping| (item.uid(), mapping))
            })
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

#[cfg(all(test, feature = "e2e"))]
mod tests {
    use std::sync::Mutex;

    use crate::config::profile::{
        item::{
            local::LocalProfile, merge::MergeProfile, remote::RemoteProfile, script::ScriptProfile,
            shared::ProfileShared,
        },
        item_type::ScriptType,
    };

    use super::*;

    static CONFIG_DIR_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn delete_item_preserves_materialized_file_before_transaction_commit() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let profile_file = config_dir.path().join("profiles").join("l-delete.yaml");
        std::fs::create_dir_all(profile_file.parent().expect("profile parent")).unwrap();
        std::fs::write(&profile_file, "mode: rule\n").unwrap();

        let mut profiles = Profiles {
            current: vec!["l-delete".to_string()],
            items: vec![Profile::Local(LocalProfile {
                shared: ProfileShared {
                    uid: "l-delete".to_string(),
                    name: "Delete me".to_string(),
                    file: "l-delete.yaml".to_string(),
                    desc: None,
                    updated: 1,
                },
                symlinks: None,
                chain: Vec::new(),
            })],
            valid: Vec::new(),
            chain: vec!["l-delete".to_string()],
        };

        profiles.delete_item("l-delete").unwrap();

        assert!(
            profile_file.exists(),
            "the materialized file must remain until the transaction commits"
        );
        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
    }

    #[test]
    fn transform_profiles_cannot_be_activated() {
        let mut profiles = Profiles {
            items: vec![Profile::Merge(MergeProfile {
                shared: ProfileShared {
                    uid: "m-transform".to_string(),
                    name: "Transform".to_string(),
                    file: "m-transform.yaml".to_string(),
                    desc: None,
                    updated: 1,
                },
            })],
            ..Profiles::default()
        };

        let error = profiles.activate(Some("m-transform")).unwrap_err();
        assert!(error.to_string().contains("not activatable"));
        assert!(profiles.current.is_empty());
    }

    #[test]
    fn transform_chains_validate_targets_and_report_runtime_relevance() {
        let local_uid = "l-source".to_string();
        let merge_uid = "m-transform".to_string();
        let script_uid = "sj-transform".to_string();
        let lua_uid = "sl-transform".to_string();
        let mut profiles = Profiles {
            current: vec![local_uid.clone()],
            items: vec![
                Profile::Local(LocalProfile {
                    shared: ProfileShared {
                        uid: local_uid.clone(),
                        name: "Source".to_string(),
                        file: "l-source.yaml".to_string(),
                        desc: None,
                        updated: 1,
                    },
                    symlinks: None,
                    chain: Vec::new(),
                }),
                Profile::Merge(MergeProfile {
                    shared: ProfileShared {
                        uid: merge_uid.clone(),
                        name: "Transform".to_string(),
                        file: "m-transform.yaml".to_string(),
                        desc: None,
                        updated: 1,
                    },
                }),
                Profile::Script(ScriptProfile {
                    shared: ProfileShared {
                        uid: script_uid.clone(),
                        name: "Script".to_string(),
                        file: "sj-transform.js".to_string(),
                        desc: None,
                        updated: 1,
                    },
                    script_type: ScriptType::JavaScript,
                }),
                Profile::Script(ScriptProfile {
                    shared: ProfileShared {
                        uid: lua_uid.clone(),
                        name: "Lua".to_string(),
                        file: "sl-transform.lua".to_string(),
                        desc: None,
                        updated: 1,
                    },
                    script_type: ScriptType::Lua,
                }),
            ],
            ..Profiles::default()
        };

        assert!(
            profiles
                .set_profile_transform_chain(&local_uid, vec![merge_uid.clone()])
                .unwrap()
        );
        let Profile::Local(local) = &profiles.items[0] else {
            panic!("expected local profile");
        };
        assert_eq!(local.chain, vec![merge_uid.clone()]);
        assert!(
            profiles
                .set_global_transform_chain(vec![merge_uid.clone()])
                .unwrap()
        );
        assert_eq!(profiles.chain, vec![merge_uid.clone()]);
        assert!(
            !profiles
                .set_global_transform_chain(vec![merge_uid.clone()])
                .unwrap()
        );

        let duplicate = profiles
            .set_global_transform_chain(vec![merge_uid.clone(), merge_uid.clone()])
            .unwrap_err();
        assert!(duplicate.to_string().contains("duplicate transform"));
        let invalid = profiles
            .set_global_transform_chain(vec![local_uid.clone()])
            .unwrap_err();
        assert!(invalid.to_string().contains("not a transform profile"));
        assert!(
            profiles
                .set_global_transform_chain(vec![script_uid.clone()])
                .unwrap()
        );
        assert_eq!(profiles.chain, vec![script_uid]);
        assert!(
            profiles
                .set_global_transform_chain(vec![lua_uid.clone()])
                .unwrap()
        );
        assert_eq!(profiles.chain, vec![lua_uid]);
        let missing = profiles
            .set_global_transform_chain(vec!["m-missing".to_string()])
            .unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("failed to get the profile item")
        );
    }

    #[test]
    fn remote_definition_replace_validates_and_applies_scoped_transforms_together() {
        let remote_uid = "r-source".to_string();
        let merge_uid = "m-transform".to_string();
        let original_url = url::Url::parse("https://example.com/old.yaml").unwrap();
        let replacement_url = url::Url::parse("https://example.com/new.yaml").unwrap();
        let mut profiles = Profiles {
            current: vec![remote_uid.clone()],
            items: vec![
                Profile::Remote(RemoteProfile {
                    url: original_url.clone(),
                    option: RemoteProfileOptions::default(),
                    shared: ProfileShared {
                        uid: remote_uid.clone(),
                        name: "Remote".to_string(),
                        file: "r-source.yaml".to_string(),
                        desc: None,
                        updated: 1,
                    },
                    chain: Vec::new(),
                    extra: SubscriptionInfo::default(),
                }),
                Profile::Merge(MergeProfile {
                    shared: ProfileShared {
                        uid: merge_uid.clone(),
                        name: "Transform".to_string(),
                        file: "m-transform.yaml".to_string(),
                        desc: None,
                        updated: 1,
                    },
                }),
            ],
            ..Profiles::default()
        };

        let invalid = profiles
            .replace_remote_definition_with_transforms(
                &remote_uid,
                "r-source.yaml",
                Some(2),
                replacement_url.clone(),
                None,
                None,
                vec!["m-missing".to_string()],
            )
            .unwrap_err();
        assert!(
            invalid
                .to_string()
                .contains("failed to get the profile item")
        );
        let Profile::Remote(remote) = &profiles.items[0] else {
            panic!("expected remote profile");
        };
        assert_eq!(remote.url, original_url);
        assert_eq!(remote.shared.updated, 1);
        assert!(remote.chain.is_empty());

        assert!(
            profiles
                .replace_remote_definition_with_transforms(
                    &remote_uid,
                    "r-source.yaml",
                    Some(2),
                    replacement_url.clone(),
                    None,
                    None,
                    vec![merge_uid.clone()],
                )
                .unwrap()
        );
        let Profile::Remote(remote) = &profiles.items[0] else {
            panic!("expected remote profile");
        };
        assert_eq!(remote.url, replacement_url);
        assert_eq!(remote.shared.updated, 2);
        assert_eq!(remote.chain, vec![merge_uid]);
    }

    #[test]
    fn delete_item_removes_dangling_scoped_transform_references() {
        let mut profiles = Profiles {
            current: vec!["l-keep".to_string()],
            items: vec![
                Profile::Local(LocalProfile {
                    shared: ProfileShared {
                        uid: "l-keep".to_string(),
                        name: "Keep".to_string(),
                        file: "l-keep.yaml".to_string(),
                        desc: None,
                        updated: 1,
                    },
                    symlinks: None,
                    chain: vec!["m-delete".to_string(), "t-keep".to_string()],
                }),
                Profile::Merge(MergeProfile {
                    shared: ProfileShared {
                        uid: "m-delete".to_string(),
                        name: "Delete".to_string(),
                        file: "m-delete.yaml".to_string(),
                        desc: None,
                        updated: 1,
                    },
                }),
            ],
            valid: Vec::new(),
            chain: Vec::new(),
        };

        let affects_runtime = profiles.delete_item("m-delete").unwrap();

        assert!(affects_runtime);
        let Profile::Local(kept) = &profiles.items[0] else {
            panic!("expected local profile");
        };
        assert_eq!(kept.chain, vec!["t-keep"]);
    }

    #[test]
    fn deleting_global_transform_marks_runtime_rebuild_required() {
        let mut profiles = Profiles {
            items: vec![Profile::Merge(MergeProfile {
                shared: ProfileShared {
                    uid: "m-global".to_string(),
                    name: "Global".to_string(),
                    file: "m-global.yaml".to_string(),
                    desc: None,
                    updated: 1,
                },
            })],
            chain: vec!["m-global".to_string()],
            ..Profiles::default()
        };

        let affects_runtime = profiles.delete_item("m-global").unwrap();

        assert!(affects_runtime);
        assert!(profiles.chain.is_empty());
    }

    #[test]
    fn append_item_only_mutates_draft_before_transaction_commit() {
        let _guard = CONFIG_DIR_LOCK.lock().expect("config dir lock");
        let config_dir = tempfile::tempdir().expect("isolated config dir");
        unsafe {
            std::env::set_var("CHIMERA_E2E_CONFIG_DIR", config_dir.path());
        }

        let mut profiles = Profiles::default();
        profiles
            .append_item(Profile::Local(LocalProfile {
                shared: ProfileShared {
                    uid: "l-create".to_string(),
                    name: "Create me".to_string(),
                    file: "l-create.yaml".to_string(),
                    desc: None,
                    updated: 1,
                },
                symlinks: None,
                chain: Vec::new(),
            }))
            .unwrap();

        let profiles_path = config_dir.path().join("profiles.yaml");
        unsafe {
            std::env::remove_var("CHIMERA_E2E_CONFIG_DIR");
        }
        assert!(
            !profiles_path.exists(),
            "profile state must remain unpersisted until the transaction commits"
        );
    }
}

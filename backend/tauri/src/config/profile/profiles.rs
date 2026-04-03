use anyhow::{Result, bail};
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use indexmap::IndexMap;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::{
    config::profile::{
        item::{Profile, ProfileMetaGetter},
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
            Ok(profiles) => profiles,
            Err(err) => {
                log::error!(target: "app", "{err:?}\n - use the default profiles");
                Self::default()
            }
        }
    }
    /// append new item
    pub fn append_item(&mut self, item: Profile) -> Result<()> {
        self.items.push(item);
        self.save_file()
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

    pub fn delete_item(&mut self, uid: &str) -> Result<bool> {
        let Some(index) = self.items.iter().position(|profile| profile.uid() == uid) else {
            bail!("failed to get the profile item \"uid:{uid}\"");
        };

        let should_update = self.current.iter().any(|current| current == uid);
        let item = self.items.remove(index);
        let file_path = dirs::app_profiles_dir()?.join(item.file());
        if file_path.exists() {
            std::fs::remove_file(file_path)?;
        }

        self.current.retain(|current| current != uid);
        self.chain.retain(|chain_uid| chain_uid != uid);

        Ok(should_update)
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
                let file_path = dirs::app_profiles_dir()?.join(item.file());
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

use anyhow::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::{
    config::profile::{
        item::{Profile, ProfileMetaGetter},
        item_type::ProfileUid,
    },
    utils::{dirs, help},
};

#[derive(Serialize, Deserialize, specta::Type, Clone)]
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
        todo!()
        /* help::save_yaml(
            &dirs::profiles_path()?,
            self,
            Some("# Profiles Config for Clash Chimera"),
        ) */
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

    /// 获取current指向的配置内容
    pub fn current_mappings(&self) -> Result<IndexMap<&str, Mapping>> {
        todo!()
    }
}

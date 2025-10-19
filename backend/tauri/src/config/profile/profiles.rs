use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    config::profile::{item::Profile, item_type::ProfileUid},
    utils::{dirs, help},
};

#[derive(Serialize, Deserialize, specta::Type, Clone)]
pub struct Profiles {
    pub current: Vec<ProfileUid>,
    #[serde(default)]
    /// profile list
    pub items: Vec<Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            current: vec![],
            /* chain: vec![],
            valid: vec![
                "dns".into(),
                "unified-delay".into(),
                "tcp-concurrent".into(),
            ], */
            items: vec![],
        }
    }
}

impl Profiles {
    pub fn new() -> Self {
        match dirs::profiles_path().and_then(|path| help::read_yaml::<Self, _>(&path)) {
            Ok(profiles) => profiles,
            Err(err) => {
                // todo: import_profile log::error!(target: "app", "{err:?}\n - use the default profiles");
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
            Some("# Profiles Config for Clash Nyanpasu"),
        ) */
    }
}

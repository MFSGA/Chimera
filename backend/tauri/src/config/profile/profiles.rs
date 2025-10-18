use anyhow::Result;
use serde::Serialize;

use crate::config::profile::{item::Profile, item_type::ProfileUid};

#[derive(Serialize, specta::Type, Clone)]
pub struct Profiles {
    pub current: Vec<ProfileUid>,
    #[serde(default)]
    /// profile list
    pub items: Vec<Profile>,
}

impl Profiles {
    pub fn new() -> Self {
        todo!()
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

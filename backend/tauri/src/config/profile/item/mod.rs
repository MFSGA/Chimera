use std::{borrow::Borrow, io::Write};

use ambassador::{Delegate, delegatable_trait};
use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use chimera_macro::EnumWrapperCombined;

use crate::config::profile::{
    item::{local::LocalProfile, remote::RemoteProfile, utils::resolve_managed_profile_path},
    item_type::ProfileItemType,
};

/// 0
pub mod local;
/// 1
pub mod remote;
/// 2
pub mod shared;
/// 3
pub mod utils;

/// Some getter is provided due to `Profile` is a enum type, and could not be used directly.
/// If access to inner data is needed, you should use the `as_xxx` or `as_mut_xxx` method to get the inner specific profile item.
#[delegatable_trait]
pub trait ProfileMetaGetter {
    fn uid(&self) -> &str;
}

#[derive(
    serde::Deserialize, serde::Serialize, Debug, Clone, EnumWrapperCombined, specta::Type, Delegate,
)]
#[delegate(ProfileMetaGetter)]
#[delegate(ProfileKindGetter)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Profile {
    Remote(RemoteProfile),
    Local(LocalProfile),
}

impl Profile {
    pub fn file(&self) -> &str {
        match self {
            Profile::Remote(profile) => &profile.shared.file,
            Profile::Local(profile) => &profile.shared.file,
        }
    }

    /// get the file data
    pub fn read_file(&self) -> Result<String> {
        let path = resolve_managed_profile_path(self.file())?;
        if !path.exists() {
            bail!("file does not exist");
        }
        std::fs::read_to_string(path).context("failed to read the file")
    }

    /// save the file data atomically
    pub fn save_file<T: Borrow<String>>(&self, data: T) -> Result<()> {
        let path = resolve_managed_profile_path(self.file())?;
        AtomicFile::new(&path, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(data.borrow().as_bytes()))
            .with_context(|| format!("failed to atomically save profile file {}", path.display()))
    }
}

#[delegatable_trait]
pub trait ProfileKindGetter {
    fn kind(&self) -> ProfileItemType;
}

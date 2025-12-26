use std::{borrow::Borrow, io::Write};

use ambassador::{Delegate, delegatable_trait};
use anyhow::{Context, Result, bail};
use chimera_macro::EnumWrapperCombined;

use crate::{
    config::profile::{item::remote::RemoteProfile, item_type::ProfileItemType},
    utils::dirs,
};

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
}

impl Profile {
    pub fn file(&self) -> &str {
        match self {
            Profile::Remote(profile) => &profile.shared.file,
        }
    }

    /// get the file data
    pub fn read_file(&self) -> Result<String> {
        let file = self.file();
        let path = dirs::app_profiles_dir()?.join(file);
        if !path.exists() {
            bail!("file does not exist");
        }
        std::fs::read_to_string(path).context("failed to read the file")
    }

    /// save the file data
    pub fn save_file<T: Borrow<String>>(&self, data: T) -> Result<()> {
        let file = self.file();
        let path = dirs::app_profiles_dir()?.join(file);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)
            .context("failed to open the file")?;
        file.write_all(data.borrow().as_bytes())
            .context("failed to save the file")
    }
}

/// Profile Setter Helper
/// It is intended to be used in the default trait implementation, so it is PRIVATE.
/// NOTE: this just a setter for fields, NOT do any file operation.
#[delegatable_trait]
trait ProfileMetaSetter {
    fn set_uid(&mut self, uid: String);
    fn set_name(&mut self, name: String);
    fn set_desc(&mut self, desc: Option<String>);
    fn set_file(&mut self, file: String);
    fn set_updated(&mut self, updated: usize);
}

#[delegatable_trait]
pub trait ProfileKindGetter {
    fn kind(&self) -> ProfileItemType;
}

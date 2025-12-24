use ambassador::{Delegate, delegatable_trait};
use chimera_macro::EnumWrapperCombined;

use crate::config::profile::item::remote::RemoteProfile;

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

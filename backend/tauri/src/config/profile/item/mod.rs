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

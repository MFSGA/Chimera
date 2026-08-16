use std::{borrow::Borrow, io::Write};

use ambassador::{Delegate, delegatable_trait};
use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use chimera_macro::EnumWrapperCombined;

use crate::config::profile::{
    item::{
        local::LocalProfile, merge::MergeProfile, remote::RemoteProfile, script::ScriptProfile,
        utils::resolve_managed_profile_path,
    },
    item_type::ProfileItemType,
};

/// 0
pub mod local;
/// 1
pub mod merge;
/// 2
pub mod remote;
/// 3
pub mod script;
/// 4
pub mod shared;
/// 5
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
    Merge(MergeProfile),
    Script(ScriptProfile),
}

impl Profile {
    pub fn file(&self) -> &str {
        match self {
            Profile::Remote(profile) => &profile.shared.file,
            Profile::Local(profile) => &profile.shared.file,
            Profile::Merge(profile) => &profile.shared.file,
            Profile::Script(profile) => &profile.shared.file,
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

#[cfg(test)]
mod tests {
    use crate::config::profile::{
        item::{merge::MergeProfile, script::ScriptProfile, shared::ProfileShared},
        item_type::ScriptType,
    };

    use super::*;

    fn shared(uid: &str, file: &str) -> ProfileShared {
        ProfileShared {
            uid: uid.to_string(),
            name: uid.to_string(),
            file: file.to_string(),
            desc: None,
            updated: 1,
        }
    }

    #[test]
    fn legacy_transform_profile_schema_round_trips() {
        let merge = Profile::Merge(MergeProfile {
            shared: shared("m-test", "m-test.yaml"),
        });
        let javascript = Profile::Script(ScriptProfile {
            shared: shared("s-test", "s-test.js"),
            script_type: ScriptType::JavaScript,
        });

        let merge_yaml = serde_yaml::to_string(&merge).unwrap();
        let script_yaml = serde_yaml::to_string(&javascript).unwrap();

        assert!(merge_yaml.contains("type: merge"));
        assert!(script_yaml.contains("type: script"));
        assert!(script_yaml.contains("script_type: javascript"));
        assert!(matches!(
            serde_yaml::from_str::<Profile>(&merge_yaml).unwrap(),
            Profile::Merge(_)
        ));
        assert!(matches!(
            serde_yaml::from_str::<Profile>(&script_yaml).unwrap(),
            Profile::Script(ScriptProfile {
                script_type: ScriptType::JavaScript,
                ..
            })
        ));
    }
}

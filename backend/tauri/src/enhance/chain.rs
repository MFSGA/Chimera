use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::config::profile::{
    item::{Profile, ProfileMetaGetter},
    item_type::{ProfileUid, ScriptType},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LogSpan {
    Log,
    Info,
    Warn,
    Error,
}

pub type Logs = Vec<(LogSpan, String)>;

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
/// 后处理输出
pub struct PostProcessingOutput {
    /// Per-source transform chain output, keyed by source profile UID and transform UID.
    pub scopes: IndexMap<ProfileUid, IndexMap<ProfileUid, Logs>>,
    /// Global transform chain output, keyed by transform UID.
    pub global: IndexMap<ProfileUid, Logs>,
}

#[derive(Debug, Clone)]
pub enum ChainTypeWrapper {
    Merge(Mapping),
    Script {
        script_type: ScriptType,
        source: String,
    },
}

#[derive(Debug, Clone)]
pub struct ChainItem {
    pub uid: ProfileUid,
    pub data: ChainTypeWrapper,
}

fn parse_merge_mapping(source: &str, uid: &str) -> Result<Mapping> {
    serde_yaml::from_str::<Option<Mapping>>(source)
        .with_context(|| format!("merge profile {uid} must contain a YAML mapping"))
        .map(Option::unwrap_or_default)
}

impl TryFrom<&Profile> for ChainItem {
    type Error = anyhow::Error;

    fn try_from(item: &Profile) -> Result<Self, Self::Error> {
        let uid = item.uid().to_string();
        let data = match item {
            Profile::Merge(_) => {
                let source = item
                    .read_file()
                    .with_context(|| format!("failed to read merge profile {uid}"))?;
                let mapping = parse_merge_mapping(&source, &uid)?;
                ChainTypeWrapper::Merge(mapping)
            }
            Profile::Script(profile) => ChainTypeWrapper::Script {
                script_type: profile.script_type,
                source: item
                    .read_file()
                    .with_context(|| format!("failed to read script profile {uid}"))?,
            },
            Profile::Local(_) | Profile::Remote(_) => {
                bail!("profile {uid} is not a transform profile")
            }
        };

        Ok(Self { uid, data })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_merge_mapping;

    #[test]
    fn empty_merge_template_is_a_noop_mapping() {
        let mapping = parse_merge_mapping(
            "# Clash Chimera Merge Template (YAML)\n# No overrides yet.\n",
            "m-empty",
        )
        .unwrap();

        assert!(mapping.is_empty());
    }

    #[test]
    fn merge_source_must_be_a_mapping() {
        let error = parse_merge_mapping("- invalid\n- for-a-merge-profile\n", "m-invalid")
            .unwrap_err()
            .to_string();

        assert!(error.contains("m-invalid"));
        assert!(error.contains("YAML mapping"));
    }
}

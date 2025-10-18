use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::config::profile::item_type::ProfileItemType;

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type)]
#[builder(
    derive(Debug, serde::Serialize, serde::Deserialize, specta::Type),
    build_fn(skip)
)]
#[builder_update(patch_fn = "apply", getter)]
pub struct ProfileShared {
    /// Profile ID
    pub uid: String,
    /// profile name
    pub name: String,
    /// profile holds the file
    // #[serde(alias = "file", deserialize_with = "deserialize_option_single_or_vec")]
    #[builder(default = "self.default_files()?")]
    pub file: String,
    /// profile description
    #[builder(default, setter(strip_option))]
    pub desc: Option<String>,
}

impl ProfileSharedBuilder {
    fn default_uid(kind: &ProfileItemType) -> String {
        super::utils::generate_uid(kind)
    }

    pub fn default_name(kind: &ProfileItemType) -> &'static str {
        match kind {
            ProfileItemType::Remote => "Remote Profile",
            // ProfileItemType::Local => "Local Profile",
            // ProfileItemType::Merge => "Merge Profile",
            // ProfileItemType::Script(_) => "Script Profile",
        }
    }

    pub fn default_file_name(kind: &ProfileItemType, uid: &str) -> String {
        match kind {
            ProfileItemType::Remote => format!("{uid}.yaml"),
            // ProfileItemType::Local => format!("{uid}.yaml"),
            // ProfileItemType::Merge => format!("{uid}.yaml"),
            // ProfileItemType::Script(ScriptType::JavaScript) => format!("{uid}.js"),
            // ProfileItemType::Script(ScriptType::Lua) => format!("{uid}.lua"),
        }
    }

    pub fn build(
        &self,
        kind: &ProfileItemType,
    ) -> Result<ProfileShared, ProfileSharedBuilderError> {
        let mut builder = self.clone();
        if self.uid.is_none() {
            builder.uid = Some(Self::default_uid(kind));
        }
        if self.name.is_none() {
            builder.name = Some(Self::default_name(kind).to_string());
        }
        if self.file.is_none() {
            builder.file = Some(Self::default_file_name(kind, builder.uid.as_ref().unwrap()));
        }

        Ok(ProfileShared {
            uid: builder.uid.unwrap(),
            name: builder.name.unwrap(),
            file: builder.file.unwrap(),
            desc: builder.desc.clone().unwrap_or_default(),
            /* todo
            updated: builder
                .updated
                .unwrap_or_else(|| chrono::Local::now().timestamp() as usize), */
        })
    }
}

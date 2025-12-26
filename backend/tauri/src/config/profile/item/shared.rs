use ambassador::delegatable_trait;
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::{
    config::profile::{item::ProfileMetaGetter, item_type::ProfileItemType},
    utils::dirs::app_profiles_dir,
};

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
    #[builder(default = "chrono::Local::now().timestamp() as usize")]
    /// update time
    pub updated: usize,
}

impl ProfileSharedBuilder {
    fn default_uid(kind: &ProfileItemType) -> String {
        super::utils::generate_uid(kind)
    }

    pub fn default_name(kind: &ProfileItemType) -> &'static str {
        match kind {
            ProfileItemType::Remote => "Remote Profile",
            ProfileItemType::Local => "Local Profile",
            // ProfileItemType::Merge => "Merge Profile",
            // ProfileItemType::Script(_) => "Script Profile",
        }
    }

    pub fn default_file_name(kind: &ProfileItemType, uid: &str) -> String {
        match kind {
            ProfileItemType::Remote => format!("{uid}.yaml"),
            ProfileItemType::Local => format!("{uid}.yaml"),
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
            updated: builder.updated.unwrap(),
        })
    }
}

#[delegatable_trait]
pub trait ProfileFileIo {
    // async fn read_file(&self) -> std::io::Result<String>;
    async fn write_file(&self, content: String) -> std::io::Result<()>;
}

impl ProfileFileIo for ProfileShared {
    async fn write_file(&self, content: String) -> std::io::Result<()> {
        let path = app_profiles_dir().map_err(std::io::Error::other)?;
        let file = path.join(&self.file);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file)
            .await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, content.as_bytes()).await
    }
}

impl ProfileMetaGetter for ProfileShared {
    fn uid(&self) -> &str {
        &self.uid
    }
}

impl super::ProfileMetaSetter for ProfileShared {
    fn set_uid(&mut self, uid: String) {
        self.uid = uid;
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn set_desc(&mut self, desc: Option<String>) {
        self.desc = desc;
    }

    fn set_file(&mut self, file: String) {
        self.file = file;
    }

    fn set_updated(&mut self, updated: usize) {
        self.updated = updated;
    }
}

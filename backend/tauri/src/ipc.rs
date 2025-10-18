use std::result::Result as StdResult;

use anyhow::Context;

use crate::config::{
    core::Config,
    profile::{
        item::{
            ProfileMetaGetter,
            remote::{RemoteProfileBuilder, RemoteProfileOptionsBuilder},
        },
        profiles::Profiles,
    },
};

type Result<T = ()> = StdResult<T, IpcError>;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl serde::Serialize for IpcError {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(format!("{self:#?}").as_str())
    }
}

impl specta::Type for IpcError {
    fn inline(
        type_map: &mut specta::TypeMap,
        generics: specta::Generics,
    ) -> specta::datatype::DataType {
        specta::datatype::DataType::Primitive(specta::datatype::PrimitiveType::String)
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_profiles() -> Result<Profiles> {
    Ok(Config::profiles().data().clone())
}

#[tauri::command]
#[specta::specta]
/// later: check in the frontend
pub async fn import_profile(url: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    let mut builder = RemoteProfileBuilder::default();
    builder.url(url);
    if let Some(option) = option {
        builder.option(option.clone());
    }

    let profile = builder
        .build_no_blocking()
        .await
        .context("failed to build a remote profile")?;

    // 根据是否为 Some(uid) 来判断是否要激活配置
    let profile_id = {
        if Config::profiles().draft().current.is_empty() {
            Some(profile.uid().to_string())
        } else {
            None
        }
    };

    todo!()
}

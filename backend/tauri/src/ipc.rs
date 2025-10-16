use std::result::Result as StdResult;

use crate::config::{
    core::Config,
    profile::{item::remote::RemoteProfileOptionsBuilder, profiles::Profiles},
};

type Result<T = ()> = StdResult<T, IpcError>;

pub enum IpcError {}

impl specta::Type for IpcError {
    fn inline(
        _type_map: &mut specta::TypeMap,
        _generics: specta::Generics,
    ) -> specta::datatype::DataType {
        specta::datatype::DataType::Primitive(specta::datatype::PrimitiveType::String)
    }
}

impl From<IpcError> for tauri::ipc::InvokeError {
    fn from(value: IpcError) -> Self {
        match value {}
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
    // let url = url::Url::parse(&url).context("failed to parse the url")?;
    // let mut builder = crate::config::profile::item::RemoteProfileBuilder::default();
    todo!()
}

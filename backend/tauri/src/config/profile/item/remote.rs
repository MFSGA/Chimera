use derive_builder::Builder;
use serde::Deserialize;
use specta::Type;

#[derive(Debug, Deserialize, Builder, Type)]
#[builder(derive(Debug, Deserialize, Type))]
pub struct RemoteProfileOptions {}

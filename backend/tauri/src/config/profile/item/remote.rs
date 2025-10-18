use derive_builder::Builder;
use serde::Deserialize;
use specta::Type;
use url::Url;

#[derive(Debug, Deserialize, Builder, Type, Clone)]
#[builder(derive(Debug, Deserialize, Type))]
pub struct RemoteProfileOptions {}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {}
    }
}

#[derive(Debug, Deserialize, Builder, Type, Clone)]
#[builder(derive(Debug, Deserialize, Type))]
#[builder(build_fn(skip, error = "RemoteProfileBuilderError"))]
pub struct RemoteProfile {
    /// subscription url
    pub url: Url,
    // #[builder_update(nested)]
    #[builder(field(
        ty = "RemoteProfileOptionsBuilder",
        build = "self.option.build().map_err(Into::into)?"
    ))]
    pub option: RemoteProfileOptions,
}

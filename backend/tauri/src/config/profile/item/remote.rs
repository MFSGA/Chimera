use derive_builder::Builder;
use serde::Deserialize;
use specta::Type;
use url::Url;

use crate::config::profile::{
    item::shared::{ProfileShared, ProfileSharedBuilder},
    item_type::ProfileItemType,
};

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
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(Into::into)?"
    ))]
    pub shared: ProfileShared,
}

#[derive(thiserror::Error, Debug)]
pub enum RemoteProfileBuilderError {
    #[error("validation error: {0}")]
    Validation(String),
}

impl RemoteProfileBuilder {
    fn validate(&self) -> Result<(), RemoteProfileBuilderError> {
        if self.url.is_none() {
            return Err(RemoteProfileBuilderError::Validation(
                "url should not be null".into(),
            ));
        }

        Ok(())
    }

    pub async fn build_no_blocking(&mut self) -> Result<RemoteProfile, RemoteProfileBuilderError> {
        self.validate()?;
        if self.shared.get_uid().is_none() {
            self.shared
                .uid(super::utils::generate_uid(&ProfileItemType::Remote));
        }
        todo!()
    }
}

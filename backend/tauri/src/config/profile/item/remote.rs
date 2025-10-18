use std::time::Duration;

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

impl RemoteProfileOptions {
    pub fn apply_default(&self) -> Self {
        let mut options = self.clone();
        /* todo: RemoteProfileOptions
         if options.user_agent.is_none() {
            options.user_agent = Some(format!("clash-nyanpasu/v{APP_VERSION}"));
        }
        if options.with_proxy.is_none() {
            options.with_proxy = Some(false);
        }
        if options.self_proxy.is_none() {
            options.self_proxy = Some(false);
        } */
        options
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
    /// 1
    #[error("validation error: {0}")]
    Validation(String),
    /// 2
    #[error("subscribe failed: {0}")]
    SubscribeFailed(#[from] SubscribeError),
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
        let url = self.url.take().unwrap();
        let options = self
            .option
            .build()
            .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?;
        let mut subscription = subscribe_url(&url, &options).await?;
        // let extra = subscription.info;

        todo!()
    }
}

#[derive(Debug)]
struct Subscription {}

#[derive(thiserror::Error, Debug)]
pub enum SubscribeError {}

/// perform a subscription
/// todo: tracing -> add tracing suupport
// #[tracing::instrument]
async fn subscribe_url(
    url: &Url,
    options: &RemoteProfileOptions,
) -> Result<Subscription, SubscribeError> {
    let options = options.apply_default();
    let mut builder = reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .no_proxy()
        .timeout(Duration::from_secs(30));

    // todo: proxy add the proxy client support
    todo!()
}

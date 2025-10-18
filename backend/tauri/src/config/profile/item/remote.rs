use std::time::Duration;

use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use crate::{
    config::profile::{
        item::shared::{ProfileShared, ProfileSharedBuilder},
        item_type::ProfileItemType,
    },
    utils::help,
};

use crate::utils::dirs::APP_VERSION;
use backon::Retryable;

#[derive(Debug, Deserialize, Builder, Type, Clone)]
#[builder(derive(Debug, Deserialize, Type))]
pub struct RemoteProfileOptions {
    pub user_agent: Option<String>,
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self { user_agent: None }
    }
}

impl RemoteProfileOptions {
    pub fn apply_default(&self) -> Self {
        let mut options = self.clone();
        if options.user_agent.is_none() {
            options.user_agent = Some(format!("clash-chimera/v{APP_VERSION}"));
        }
        /* todo: RemoteProfileOptions

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
pub enum SubscribeError {
    #[error("network issue at {url}: {source}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

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

    builder = builder.user_agent(options.user_agent.unwrap());

    let client = builder.build().map_err(|e| SubscribeError::Network {
        url: url.to_string(),
        source: e,
    })?;

    let perform_req = || async { client.get(url.as_str()).send().await?.error_for_status() };
    let resp = perform_req
        .retry(backon::ExponentialBuilder::default())
        // Only retry on network errors or server errors
        .when(|result| {
            !result.is_status()
                || result.status().is_some_and(|status_code| {
                    !matches!(
                        status_code,
                        reqwest::StatusCode::FORBIDDEN
                            | reqwest::StatusCode::NOT_FOUND
                            | reqwest::StatusCode::UNAUTHORIZED
                    )
                })
        })
        .await
        .map_err(|e| SubscribeError::Network {
            url: url.to_string(),
            source: e,
        })?;

    let header = resp.headers();
    // tracing::debug!("headers: {:#?}", header);

    // parse the Subscription UserInfo
    let extra = match header
        .get("subscription-userinfo")
        .or(header.get("Subscription-Userinfo"))
    {
        Some(value) => {
            // tracing::debug!("Subscription-Userinfo: {:?}", value);
            let sub_info = value.to_str().unwrap_or("");

            Some(SubscriptionInfo {
                upload: help::parse_str(sub_info, "upload").unwrap_or(0),
                download: help::parse_str(sub_info, "download").unwrap_or(0),
                total: help::parse_str(sub_info, "total").unwrap_or(0),
                expire: help::parse_str(sub_info, "expire").unwrap_or(0),
            })
        }
        None => None,
    };

    // Try to parse filename from headers
    // `Profile-Title` -> `Content-Disposition`
    let filename = utils::parse_profile_title_header(resp.headers())
        .or_else(|| utils::parse_filename_from_content_disposition(resp.headers()));
    todo!()
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    pub upload: usize,
    pub download: usize,
    pub total: usize,
    pub expire: usize,
}

mod utils {
    use base64::{engine::general_purpose, Engine};
    use reqwest::header::{self, HeaderMap};

    /// parse profile title from headers
    pub fn parse_profile_title_header(headers: &HeaderMap) -> Option<String> {
        headers
            .get("profile-title")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                if v.starts_with("base64:") {
                    let encoded = v.trim_start_matches("base64:");
                    general_purpose::STANDARD
                        .decode(encoded)
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                } else {
                    Some(v.to_string())
                }
            })
    }

    pub fn parse_filename_from_content_disposition(headers: &HeaderMap) -> Option<String> {
        let filename = crate::utils::help::parse_str::<String>(
            headers
                .get(header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            "filename",
        )?;
        // tracing::debug!("Content-Disposition: {:?}", filename);

        let filename = format!("{filename:?}");
        let filename = filename.trim_matches('"');
        match crate::utils::help::parse_str::<String>(filename, "filename*") {
            Some(filename) => {
                let iter = percent_encoding::percent_decode(filename.as_bytes());
                let filename = iter.decode_utf8().unwrap_or_default();
                filename
                    .split("''")
                    .last()
                    .map(|s| s.trim_matches('"').to_string())
            }
            None => match crate::utils::help::parse_str::<String>(filename, "filename") {
                Some(filename) => {
                    let filename = filename.trim_matches('"');
                    Some(filename.to_string())
                }
                None => None,
            },
        }
    }
}

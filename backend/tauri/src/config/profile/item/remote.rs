use std::time::Duration;

use ambassador::Delegate;
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use url::Url;

use crate::{
    config::profile::{
        item::{
            ProfileKindGetter, ProfileMetaGetter, ProfileMetaSetter,
            ambassador_impl_ProfileMetaGetter, ambassador_impl_ProfileMetaSetter,
            shared::{
                ProfileFileIo, ProfileShared, ProfileSharedBuilder, ambassador_impl_ProfileFileIo,
            },
        },
        item_type::{ProfileItemType, ProfileUid},
    },
    utils::help,
};

use crate::utils::dirs::APP_VERSION;
use backon::Retryable;

const PROFILE_TYPE: ProfileItemType = ProfileItemType::Remote;

pub trait RemoteProfileSubscription {
    async fn subscribe(&mut self, opts: Option<RemoteProfileOptionsBuilder>) -> anyhow::Result<()>;
}

#[derive(Debug, Deserialize, Serialize, Builder, Type, Clone, BuilderUpdate)]
#[builder(derive(Debug, Deserialize, Type))]
#[builder_update(patch_fn = "apply", getter)]
pub struct RemoteProfileOptions {
    /// see issue #13. must set the builder attr for build the user_agent for client
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default, setter(strip_option))]
    pub user_agent: Option<String>,
    /// subscription update interval
    #[builder(default = "120")]
    pub update_interval: u64,
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            update_interval: 120, // 2 hours
        }
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

#[derive(Debug, Deserialize, Serialize, Builder, Type, Clone, Delegate)]
#[builder(derive(Debug, Deserialize, Type))]
#[builder(build_fn(skip, error = "RemoteProfileBuilderError"))]
// #[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
#[delegate(ProfileMetaSetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct RemoteProfile {
    /// subscription url
    pub url: Url,
    // #[builder_update(nested)]
    #[builder(field(
        ty = "RemoteProfileOptionsBuilder",
        build = "self.option.build().map_err(Into::into)?"
    ))]
    pub option: RemoteProfileOptions,
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(Into::into)?"
    ))]
    #[builder_field_attr(serde(flatten))]
    // #[builder_update(nested)]
    pub shared: ProfileShared,

    pub chain: Vec<ProfileUid>,
    /// subscription user info
    #[builder(default)]
    #[serde(default)]
    pub extra: SubscriptionInfo,
}

impl ProfileKindGetter for RemoteProfile {
    fn kind(&self) -> ProfileItemType {
        PROFILE_TYPE
    }
}

impl RemoteProfileSubscription for RemoteProfile {
    #[tracing::instrument]
    async fn subscribe(
        &mut self,
        partial: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<()> {
        let mut opts = self.option.clone();
        if let Some(partial) = partial {
            opts.apply(partial);
        }
        let subscription = subscribe_url(&self.url, &opts).await?;
        self.extra = subscription.info;

        let content = serde_yaml::to_string(&subscription.data)?;
        self.write_file(content).await?;
        self.set_updated(chrono::Local::now().timestamp() as usize);
        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum RemoteProfileBuilderError {
    /// 1
    #[error("validation error: {0}")]
    Validation(String),
    /// 2
    #[error("subscribe failed: {0}")]
    SubscribeFailed(#[from] SubscribeError),
    /// 3
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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

        let extra = subscription.info;

        if self.shared.get_name().is_none()
            && let Some(filename) = subscription.filename.take()
        {
            self.shared.name(filename);
        }
        if self.option.get_update_interval().is_none() && subscription.opts.is_some() {
            self.option
                .update_interval(subscription.opts.take().unwrap().update_interval);
        }

        let profile = RemoteProfile {
            shared: self
                .shared
                .build(&PROFILE_TYPE)
                .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?,
            url,
            extra,
            option: self.option.build().unwrap(),
            chain: self.chain.take().unwrap_or_default(),
        };
        // write the profile to the file
        profile
            .shared
            .write_file(
                serde_yaml::to_string(&subscription.data)
                    .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?,
            )
            .await?;
        Ok(profile)
    }
}

#[derive(Debug)]
struct Subscription {
    pub url: Url,
    pub filename: Option<String>,
    pub data: Mapping,
    pub info: SubscriptionInfo,
    pub opts: Option<RemoteProfileOptions>,
}

#[derive(thiserror::Error, Debug)]
pub enum SubscribeError {
    #[error("network issue at {url}: {source}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("yaml parse error at {url}: {source}")]
    Parse {
        url: String,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("invalid profile at {url}: {reason}")]
    ValidationFailed { url: String, reason: String },
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

    // parse the profile-update-interval
    let opts = match header
        .get("profile-update-interval")
        .or(header.get("Profile-Update-Interval"))
    {
        Some(value) => {
            // tracing::debug!("profile-update-interval: {:?}", value);
            match value.to_str().unwrap_or("").parse::<u64>() {
                Ok(val) => Some(RemoteProfileOptions {
                    update_interval: val * 60, // hour -> min
                    ..RemoteProfileOptions::default()
                }),
                Err(_) => None,
            }
        }
        None => None,
    };

    let data = resp
        .text_with_charset("utf-8")
        .await
        .map_err(|e| SubscribeError::Network {
            url: url.to_string(),
            source: e,
        })?;

    // process the charset "UTF-8 with BOM"
    let data = data.trim_start_matches('\u{feff}');

    // check the data whether the valid yaml format
    let yaml = serde_yaml::from_str::<Mapping>(data).map_err(|e| SubscribeError::Parse {
        url: url.to_string(),
        source: e,
    })?;

    if !yaml.contains_key("proxies") && !yaml.contains_key("proxy-providers") {
        return Err(SubscribeError::ValidationFailed {
            url: url.to_string(),
            reason: "profile does not contain `proxies` or `proxy-providers`".to_string(),
        });
    }

    Ok(Subscription {
        url: url.clone(),
        filename,
        data: yaml,
        info: extra.unwrap_or_default(),
        opts,
    })
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    pub upload: usize,
    pub download: usize,
    pub total: usize,
    pub expire: usize,
}

mod utils {
    use base64::{Engine, engine::general_purpose};
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

use std::time::Duration;

use ambassador::Delegate;
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use encoding_rs::{Encoding, UTF_8};
use futures_util::StreamExt;
use mime::Mime;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use url::Url;

use crate::{
    config::profile::{
        item::{
            MAX_PROFILE_YAML_BYTES, ProfileKindGetter, ProfileMetaGetter, ProfileMetaSetter,
            ambassador_impl_ProfileMetaGetter, ambassador_impl_ProfileMetaSetter,
            shared::{ProfileShared, ProfileSharedBuilder, current_profile_timestamp},
            validate_profile_mapping_keys,
        },
        item_type::{ProfileItemType, ProfileUid},
    },
    utils::{
        config::{NyanpasuReqwestProxyExt, get_self_proxy, get_system_proxy},
        help,
    },
};

use crate::utils::dirs::APP_VERSION;
use backon::Retryable;

const PROFILE_TYPE: ProfileItemType = ProfileItemType::Remote;
const MAX_SUBSCRIPTION_REDIRECTS: usize = 10;
const MAX_SAFE_JAVASCRIPT_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES: u64 = MAX_SAFE_JAVASCRIPT_INTEGER / 2;
const MAX_JAVASCRIPT_DATE_SECONDS: u64 = 8_640_000_000_000;
pub(crate) const MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES: u64 = MAX_SAFE_JAVASCRIPT_INTEGER / 60;

#[derive(Debug, Deserialize, Serialize, Builder, Type, Clone, PartialEq, Eq, BuilderUpdate)]
#[builder(derive(Debug, Deserialize, Type))]
#[builder_update(patch_fn = "apply", getter)]
pub struct RemoteProfileOptions {
    /// see issue #13. must set the builder attr for build the user_agent for client
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default, setter(strip_option))]
    pub user_agent: Option<String>,
    #[serde(default)]
    #[builder(default)]
    pub with_proxy: bool,
    #[serde(default)]
    #[builder(default)]
    pub self_proxy: bool,
    /// subscription update interval in minutes
    #[serde(
        default = "default_update_interval_minutes",
        alias = "update_interval",
        deserialize_with = "deserialize_update_interval_minutes"
    )]
    #[builder(default = "default_update_interval_minutes()")]
    #[specta(type = u64)]
    pub update_interval_minutes: u64,
}

const fn default_update_interval_minutes() -> u64 {
    120
}

pub(crate) fn is_valid_profile_update_interval_minutes(value: u64) -> bool {
    (1..=MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES).contains(&value)
}

fn deserialize_update_interval_minutes<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    Ok(if is_valid_profile_update_interval_minutes(value) {
        value
    } else {
        default_update_interval_minutes()
    })
}

fn validate_update_interval_builder(
    builder: &RemoteProfileOptionsBuilder,
) -> Result<(), RemoteProfileBuilderError> {
    if let Some(value) = *builder.get_update_interval_minutes()
        && !is_valid_profile_update_interval_minutes(value)
    {
        return Err(RemoteProfileBuilderError::Validation(format!(
            "profile update interval must be between 1 and {MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES} minutes"
        )));
    }
    Ok(())
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            with_proxy: false,
            self_proxy: false,
            update_interval_minutes: default_update_interval_minutes(),
        }
    }
}

impl RemoteProfileOptions {
    pub fn apply_default(&self) -> Self {
        let mut options = self.clone();
        if options.user_agent.is_none() {
            options.user_agent = Some(format!("clash-chimera/v{APP_VERSION}"));
        }
        options
    }
}

#[derive(Debug, Deserialize, Serialize, Builder, Type, Clone, PartialEq, Eq, Delegate)]
#[builder(derive(Debug, Deserialize, Type))]
#[builder(build_fn(skip, error = "RemoteProfileBuilderError"))]
// #[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
#[delegate(ProfileMetaSetter, target = "shared")]
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

impl RemoteProfile {
    #[tracing::instrument(skip_all, fields(profile_uid = %self.shared.uid))]
    pub(crate) async fn prepare_subscription(
        &self,
        partial: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<(Self, String)> {
        let mut opts = self.option.clone();
        if let Some(partial) = partial {
            validate_update_interval_builder(&partial)?;
            opts.apply(partial);
        }
        if !is_valid_profile_update_interval_minutes(opts.update_interval_minutes) {
            anyhow::bail!(
                "profile update interval must be between 1 and {MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES} minutes"
            );
        }

        let subscription = subscribe_url(&self.url, &opts).await?;
        let content = serde_yaml::to_string(&subscription.data)?;
        let mut updated = self.clone();
        updated.extra = subscription.info;
        updated.set_updated(current_profile_timestamp());
        Ok((updated, content))
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
        validate_update_interval_builder(&self.option)
    }

    pub(crate) async fn build_no_blocking_unpersisted(
        &mut self,
    ) -> Result<(RemoteProfile, String), RemoteProfileBuilderError> {
        self.validate()?;
        if self.shared.get_uid().is_none() {
            self.shared
                .uid(super::utils::generate_uid(&ProfileItemType::Remote));
        }
        let url = self.url.take().ok_or_else(|| {
            RemoteProfileBuilderError::Validation("url should not be null".into())
        })?;
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
        if self.option.get_update_interval_minutes().is_none()
            && let Some(subscription_options) = subscription.opts.take()
        {
            self.option
                .update_interval_minutes(subscription_options.update_interval_minutes);
        }

        let profile = RemoteProfile {
            shared: self
                .shared
                .build(&PROFILE_TYPE)
                .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?,
            url,
            extra,
            option: self
                .option
                .build()
                .map_err(|error| RemoteProfileBuilderError::Validation(error.to_string()))?,
            chain: self.chain.take().unwrap_or_default(),
        };
        let content = serde_yaml::to_string(&subscription.data)
            .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?;
        Ok((profile, content))
    }

    pub fn patch_profile(
        &self,
        profile: &mut RemoteProfile,
    ) -> Result<(), RemoteProfileBuilderError> {
        validate_update_interval_builder(&self.option)?;
        if let Some(url) = self.url.as_ref() {
            profile.url = url.clone();
        }

        if let Some(name) = self.shared.get_name() {
            profile.shared.name = name.clone();
        }

        if let Some(desc) = self.shared.get_desc() {
            profile.shared.desc = desc.clone();
        }

        if let Some(updated) = self.shared.get_updated() {
            profile.shared.updated = *updated;
        }

        profile.option.apply(self.option.clone());

        if let Some(chain) = self.chain.as_ref() {
            profile.chain = chain.clone();
        }

        Ok(())
    }
}

#[derive(Debug)]
struct Subscription {
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

fn response_encoding(headers: &HeaderMap, default_encoding: &str) -> &'static Encoding {
    let encoding_name = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<Mime>().ok())
        .and_then(|mime| {
            mime.get_param("charset")
                .map(|charset| charset.as_str().to_string())
        })
        .unwrap_or_else(|| default_encoding.to_string());

    Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8)
}

fn redacted_subscription_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or("<unknown-host>");
    match url.port() {
        Some(port) => format!("{}://{host}:{port}/<redacted>", url.scheme()),
        None => format!("{}://{host}/<redacted>", url.scheme()),
    }
}

fn subscription_network_error(url: &Url, source: reqwest::Error) -> SubscribeError {
    SubscribeError::Network {
        url: redacted_subscription_url(url),
        source: source.without_url(),
    }
}

fn response_size_error(url: &Url, max_bytes: usize) -> SubscribeError {
    SubscribeError::ValidationFailed {
        url: redacted_subscription_url(url),
        reason: format!("profile response exceeds the maximum size of {max_bytes} bytes"),
    }
}

fn validate_subscription_mapping(url: &Url, mapping: &Mapping) -> Result<(), SubscribeError> {
    validate_profile_mapping_keys(mapping).map_err(|error| SubscribeError::ValidationFailed {
        url: redacted_subscription_url(url),
        reason: error.to_string(),
    })?;
    let proxies = mapping.get("proxies");
    let providers = mapping.get("proxy-providers");
    if proxies.is_none() && providers.is_none() {
        return Err(SubscribeError::ValidationFailed {
            url: redacted_subscription_url(url),
            reason: "profile does not contain `proxies` or `proxy-providers`".into(),
        });
    }
    if proxies.is_some_and(|value| !value.is_sequence()) {
        return Err(SubscribeError::ValidationFailed {
            url: redacted_subscription_url(url),
            reason: "profile `proxies` field must be a sequence".into(),
        });
    }
    if providers.is_some_and(|value| !value.is_mapping()) {
        return Err(SubscribeError::ValidationFailed {
            url: redacted_subscription_url(url),
            reason: "profile `proxy-providers` field must be a mapping".into(),
        });
    }
    Ok(())
}

fn validate_subscription_redirect(previous: &[Url], next: &Url) -> Result<(), &'static str> {
    if previous.len() > MAX_SUBSCRIPTION_REDIRECTS {
        return Err("remote profile redirect limit exceeded");
    }
    if !matches!(next.scheme(), "http" | "https") {
        return Err("remote profile redirect uses an unsupported scheme");
    }
    if previous
        .last()
        .is_some_and(|url| url.scheme() == "https" && next.scheme() == "http")
    {
        return Err("remote profile redirect attempted an HTTPS downgrade");
    }
    Ok(())
}

fn subscription_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        match validate_subscription_redirect(attempt.previous(), attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(reason) => attempt.error(reason),
        }
    })
}

fn is_retryable_subscription_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::REQUEST_TIMEOUT | reqwest::StatusCode::TOO_MANY_REQUESTS
    ) || status.is_server_error()
}

fn should_retry_subscription_request(error: &reqwest::Error) -> bool {
    error.is_timeout()
        || error.is_connect()
        || error.status().is_some_and(is_retryable_subscription_status)
}

fn parse_bounded_subscription_value(value: &str, key: &str, max: u64) -> usize {
    help::parse_str::<u64>(value, key)
        .filter(|value| *value <= max)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn parse_subscription_info(value: &str) -> SubscriptionInfo {
    SubscriptionInfo {
        upload: parse_bounded_subscription_value(
            value,
            "upload",
            MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES,
        ),
        download: parse_bounded_subscription_value(
            value,
            "download",
            MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES,
        ),
        total: parse_bounded_subscription_value(value, "total", MAX_SAFE_JAVASCRIPT_INTEGER),
        expire: parse_bounded_subscription_value(value, "expire", MAX_JAVASCRIPT_DATE_SECONDS),
    }
}

fn profile_update_interval_minutes(headers: &HeaderMap) -> Option<u64> {
    let hours = headers
        .get("profile-update-interval")?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()?;
    hours
        .checked_mul(60)
        .filter(|minutes| is_valid_profile_update_interval_minutes(*minutes))
}

async fn response_text_with_limit(
    response: reqwest::Response,
    default_encoding: &str,
    max_bytes: usize,
) -> Result<String, SubscribeError> {
    let url = response.url().clone();
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(response_size_error(&url, max_bytes));
    }

    let encoding = response_encoding(response.headers(), default_encoding);
    let capacity = response
        .content_length()
        .unwrap_or_default()
        .min(max_bytes as u64) as usize;
    let mut bytes = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| subscription_network_error(&url, source))?;
        let Some(next_len) = bytes.len().checked_add(chunk.len()) else {
            return Err(response_size_error(&url, max_bytes));
        };
        if next_len > max_bytes {
            return Err(response_size_error(&url, max_bytes));
        }
        bytes.extend_from_slice(&chunk);
    }

    let (text, _, _) = encoding.decode(&bytes);
    Ok(text.into_owned())
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
        .redirect(subscription_redirect_policy())
        .timeout(Duration::from_secs(30));

    let proxy_url = if options.self_proxy {
        get_self_proxy().ok()
    } else {
        None
    }
    .or_else(|| {
        if options.with_proxy {
            get_system_proxy().ok().flatten()
        } else {
            None
        }
    });
    if let Some(proxy_url) = proxy_url {
        builder = builder.swift_set_proxy(&proxy_url);
    }

    let user_agent = options
        .user_agent
        .ok_or_else(|| SubscribeError::ValidationFailed {
            url: redacted_subscription_url(url),
            reason: "remote profile user agent is missing after defaults were applied".into(),
        })?;
    builder = builder.user_agent(user_agent);

    let client = builder
        .build()
        .map_err(|error| subscription_network_error(url, error))?;

    let perform_req = || async { client.get(url.as_str()).send().await?.error_for_status() };
    let resp = perform_req
        .retry(backon::ExponentialBuilder::default())
        // Only retry on network errors or server errors
        .when(should_retry_subscription_request)
        .await
        .map_err(|error| subscription_network_error(url, error))?;

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

            Some(parse_subscription_info(sub_info))
        }
        None => None,
    };

    // Try to parse filename from headers
    // `Profile-Title` -> `Content-Disposition`
    let filename = utils::parse_profile_title_header(resp.headers())
        .or_else(|| utils::parse_filename_from_content_disposition(resp.headers()));

    // parse the profile-update-interval
    let opts = profile_update_interval_minutes(header).map(|update_interval_minutes| {
        RemoteProfileOptions {
            update_interval_minutes,
            ..RemoteProfileOptions::default()
        }
    });

    let data = response_text_with_limit(resp, "utf-8", MAX_PROFILE_YAML_BYTES).await?;

    // process the charset "UTF-8 with BOM"
    let data = data.trim_start_matches('\u{feff}');

    // check the data whether the valid yaml format
    let yaml = serde_yaml::from_str::<Mapping>(data).map_err(|source| SubscribeError::Parse {
        url: redacted_subscription_url(url),
        source,
    })?;

    validate_subscription_mapping(url, &yaml)?;

    Ok(Subscription {
        filename,
        data: yaml,
        info: extra.unwrap_or_default(),
        opts,
    })
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    pub upload: usize,
    pub download: usize,
    pub total: usize,
    pub expire: usize,
}

mod utils {
    use base64::{Engine, engine::general_purpose};
    use reqwest::header::{self, HeaderMap};

    fn normalize_profile_title(value: &str) -> Option<String> {
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            return None;
        }
        Some(value.to_string())
    }

    fn content_disposition_parameter(value: &str, key: &str) -> Option<String> {
        value.split(';').skip(1).find_map(|parameter| {
            let (name, value) = parameter.trim().split_once('=')?;
            if !name.trim().eq_ignore_ascii_case(key) {
                return None;
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            Some(value.to_string())
        })
    }

    /// parse profile title from headers
    pub fn parse_profile_title_header(headers: &HeaderMap) -> Option<String> {
        let value = headers.get("profile-title")?.to_str().ok()?;
        let decoded = if value
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("base64:"))
        {
            general_purpose::STANDARD
                .decode(&value[7..])
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())?
        } else {
            value.to_string()
        };
        normalize_profile_title(&decoded)
    }

    pub fn parse_filename_from_content_disposition(headers: &HeaderMap) -> Option<String> {
        let value = headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()?;
        if let Some(encoded) = content_disposition_parameter(value, "filename*") {
            let encoded = encoded
                .split_once("''")
                .map_or(encoded.as_str(), |(_, value)| value);
            if let Ok(decoded) = percent_encoding::percent_decode(encoded.as_bytes()).decode_utf8()
                && let Some(title) = normalize_profile_title(&decoded)
            {
                return Some(title);
            }
        }

        content_disposition_parameter(value, "filename")
            .and_then(|filename| normalize_profile_title(&filename))
    }
}

#[cfg(test)]
mod tests {
    use std::{future::pending, time::Duration};

    use base64::{Engine, engine::general_purpose};
    use reqwest::header::{CONTENT_DISPOSITION, HeaderMap, HeaderValue};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use url::Url;

    use super::{
        MAX_JAVASCRIPT_DATE_SECONDS, MAX_SAFE_JAVASCRIPT_INTEGER,
        MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES, MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES,
        RemoteProfileBuilder, RemoteProfileOptions, RemoteProfileOptionsBuilder, SubscribeError,
        is_retryable_subscription_status, parse_subscription_info, profile_update_interval_minutes,
        redacted_subscription_url, response_text_with_limit, subscription_network_error,
        utils::{parse_filename_from_content_disposition, parse_profile_title_header},
        validate_subscription_mapping, validate_subscription_redirect,
        validate_update_interval_builder,
    };

    #[test]
    fn profile_title_header_accepts_plain_and_base64_unicode_titles() {
        let mut headers = HeaderMap::new();
        headers.insert("profile-title", HeaderValue::from_static("Plain Profile"));
        assert_eq!(
            parse_profile_title_header(&headers).as_deref(),
            Some("Plain Profile")
        );

        let encoded = general_purpose::STANDARD.encode("代理配置");
        headers.insert(
            "profile-title",
            HeaderValue::from_str(&format!("BASE64:{encoded}"))
                .expect("valid encoded profile title header"),
        );
        assert_eq!(
            parse_profile_title_header(&headers).as_deref(),
            Some("代理配置")
        );
    }

    #[test]
    fn profile_title_header_rejects_empty_control_and_invalid_base64_titles() {
        let mut headers = HeaderMap::new();
        for decoded in ["   ", "unsafe\nname", "unsafe\tname"] {
            let encoded = general_purpose::STANDARD.encode(decoded);
            headers.insert(
                "profile-title",
                HeaderValue::from_str(&format!("base64:{encoded}"))
                    .expect("valid encoded invalid-title header"),
            );
            assert_eq!(parse_profile_title_header(&headers), None);
        }

        headers.insert(
            "profile-title",
            HeaderValue::from_static("base64:not-valid-***"),
        );
        assert_eq!(parse_profile_title_header(&headers), None);
    }

    #[test]
    fn content_disposition_prefers_rfc5987_unicode_filename() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static(
                "attachment; filename=\"fallback.yaml\"; filename*=UTF-8''%E4%BB%A3%E7%90%86.yaml",
            ),
        );

        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("代理.yaml")
        );
    }

    #[test]
    fn content_disposition_parameter_names_are_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; FILENAME=\"Profile.yaml\""),
        );

        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("Profile.yaml")
        );
    }

    #[test]
    fn invalid_extended_filename_falls_back_to_plain_filename() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static(
                "attachment; filename=\"fallback.yaml\"; filename*=UTF-8''%FF",
            ),
        );

        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("fallback.yaml")
        );
    }

    #[test]
    fn content_disposition_rejects_empty_and_control_character_filenames() {
        let mut headers = HeaderMap::new();
        for value in [
            "attachment; filename=\"   \"",
            "attachment; filename*=UTF-8''unsafe%0Aname",
            "attachment; filename*=UTF-8''unsafe%09name",
        ] {
            headers.insert(
                CONTENT_DISPOSITION,
                HeaderValue::from_str(value).expect("valid invalid-filename header fixture"),
            );
            assert_eq!(parse_filename_from_content_disposition(&headers), None);
        }
    }

    async fn response_from_raw(raw_response: Vec<u8>) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind remote profile response fixture");
        let address = listener
            .local_addr()
            .expect("failed to read remote profile response fixture address");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("failed to accept remote profile response fixture request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(&raw_response)
                .await
                .expect("failed to write remote profile response fixture");
            socket
                .shutdown()
                .await
                .expect("failed to close remote profile response fixture");
        });

        let response = reqwest::Client::new()
            .get(format!("http://{address}/profile.yaml"))
            .send()
            .await
            .expect("failed to request remote profile response fixture");
        server
            .await
            .expect("remote profile response fixture task failed");
        response
    }

    async fn response_with_stalled_body(
        raw_headers: Vec<u8>,
    ) -> (reqwest::Response, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind stalled response fixture");
        let address = listener
            .local_addr()
            .expect("failed to read stalled response fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("failed to accept stalled response fixture request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(&raw_headers)
                .await
                .expect("failed to write stalled response headers");
            socket
                .flush()
                .await
                .expect("failed to flush stalled response headers");
            pending::<()>().await;
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/profile.yaml"))
            .send()
            .await
            .expect("failed to request stalled response fixture");
        (response, server)
    }

    fn fixed_response(body: &[u8], content_type: &str) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn subscription_mapping(yaml: &str) -> serde_yaml::Mapping {
        serde_yaml::from_str(yaml).expect("valid subscription mapping fixture")
    }

    #[test]
    fn subscription_info_accepts_exact_frontend_safe_boundaries() {
        let info = parse_subscription_info(&format!(
            "upload={MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES}; download={MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES}; total={MAX_SAFE_JAVASCRIPT_INTEGER}; expire={MAX_JAVASCRIPT_DATE_SECONDS}"
        ));

        assert_eq!(
            info.upload,
            usize::try_from(MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES).unwrap_or(0)
        );
        assert_eq!(
            info.download,
            usize::try_from(MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES).unwrap_or(0)
        );
        assert_eq!(
            info.total,
            usize::try_from(MAX_SAFE_JAVASCRIPT_INTEGER).unwrap_or(0)
        );
        assert_eq!(
            info.expire,
            usize::try_from(MAX_JAVASCRIPT_DATE_SECONDS).unwrap_or(0)
        );
        assert!(
            info.upload
                .checked_add(info.download)
                .is_some_and(|used| used as u64 <= MAX_SAFE_JAVASCRIPT_INTEGER)
        );
    }

    #[test]
    fn subscription_info_rejects_values_that_frontend_numbers_cannot_represent() {
        let info = parse_subscription_info(&format!(
            "upload={}; download={}; total={}; expire={} ",
            MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES + 1,
            MAX_SAFE_SUBSCRIPTION_TRANSFER_BYTES + 1,
            MAX_SAFE_JAVASCRIPT_INTEGER + 1,
            MAX_JAVASCRIPT_DATE_SECONDS + 1,
        ));

        assert_eq!(info.upload, 0);
        assert_eq!(info.download, 0);
        assert_eq!(info.total, 0);
        assert_eq!(info.expire, 0);
    }

    #[test]
    fn subscription_info_preserves_zero_fallback_for_malformed_values() {
        let info = parse_subscription_info(
            "upload=-1; download=1.5; total=not-a-number; expire=18446744073709551616",
        );

        assert_eq!(info.upload, 0);
        assert_eq!(info.download, 0);
        assert_eq!(info.total, 0);
        assert_eq!(info.expire, 0);
    }

    #[test]
    fn subscription_retry_statuses_include_only_transient_http_failures() {
        for status in [
            reqwest::StatusCode::REQUEST_TIMEOUT,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::BAD_GATEWAY,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            reqwest::StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert!(
                is_retryable_subscription_status(status),
                "transient subscription status must be retried: {status}"
            );
        }

        for status in [
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::METHOD_NOT_ALLOWED,
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            assert!(
                !is_retryable_subscription_status(status),
                "deterministic subscription status must not be retried: {status}"
            );
        }
    }

    #[test]
    fn subscription_redirects_allow_network_transitions_without_tls_downgrade() {
        let http = Url::parse("http://example.com/profile").expect("valid HTTP redirect fixture");
        let https =
            Url::parse("https://example.com/profile").expect("valid HTTPS redirect fixture");

        validate_subscription_redirect(&[http.clone()], &http)
            .expect("HTTP redirect may remain on HTTP");
        validate_subscription_redirect(&[http], &https)
            .expect("HTTP redirect may upgrade to HTTPS");
        validate_subscription_redirect(&[https.clone()], &https)
            .expect("HTTPS redirect may remain on HTTPS");
    }

    #[test]
    fn subscription_redirects_reject_tls_downgrades_and_unsupported_schemes() {
        let https =
            Url::parse("https://example.com/profile").expect("valid HTTPS redirect fixture");
        let http = Url::parse("http://example.com/profile").expect("valid HTTP redirect fixture");
        let file = Url::parse("file:///C:/profile.yaml").expect("valid file redirect fixture");

        let downgrade = validate_subscription_redirect(&[https.clone()], &http)
            .expect_err("HTTPS redirect downgrade must be rejected");
        assert!(downgrade.contains("HTTPS downgrade"));

        let unsupported = validate_subscription_redirect(&[https], &file)
            .expect_err("non-network redirect scheme must be rejected");
        assert!(unsupported.contains("unsupported scheme"));
    }

    #[test]
    fn subscription_redirects_enforce_the_default_ten_hop_limit() {
        let url =
            Url::parse("https://example.com/profile").expect("valid redirect limit URL fixture");
        validate_subscription_redirect(&vec![url.clone(); 10], &url)
            .expect("ten previous redirect entries must remain within the configured limit");

        let error = validate_subscription_redirect(&vec![url.clone(); 11], &url)
            .expect_err("the eleventh redirect entry must be rejected");
        assert!(error.contains("limit exceeded"));
    }

    #[test]
    fn subscription_error_urls_hide_credentials_paths_queries_and_fragments() {
        let url = Url::parse(
            "https://user:password@example.com:8443/private/path-token?access_token=query-secret#fragment-secret",
        )
        .expect("valid sensitive subscription URL fixture");
        assert_eq!(
            redacted_subscription_url(&url),
            "https://example.com:8443/<redacted>"
        );

        let error = validate_subscription_mapping(&url, &subscription_mapping("dns: {}\n"))
            .expect_err("invalid subscription fixture must produce a redacted error");
        let message = error.to_string();
        assert!(message.contains("https://example.com:8443/<redacted>"));
        for secret in [
            "user",
            "password",
            "private",
            "path-token",
            "access_token",
            "query-secret",
            "fragment-secret",
        ] {
            assert!(
                !message.contains(secret),
                "subscription error leaked sensitive URL component: {secret}"
            );
        }
    }

    #[tokio::test]
    async fn network_errors_remove_the_reqwest_internal_url() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sensitive network-error fixture");
        let address = listener
            .local_addr()
            .expect("read sensitive network-error fixture address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("accept sensitive network-error fixture request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("write sensitive network-error fixture response");
        });

        let url = Url::parse(&format!(
            "http://user:password@{address}/private/path-token?access_token=query-secret#fragment-secret"
        ))
        .expect("valid sensitive network-error URL fixture");
        let source = reqwest::ClientBuilder::new()
            .no_proxy()
            .build()
            .expect("build no-proxy sensitive network-error client")
            .get(url.clone())
            .send()
            .await
            .expect("sensitive network-error fixture must return a response")
            .error_for_status()
            .expect_err("500 response must produce a reqwest status error");
        server
            .await
            .expect("sensitive network-error fixture task must finish");
        let message = subscription_network_error(&url, source).to_string();

        assert!(message.contains(&redacted_subscription_url(&url)));
        for secret in [
            "user",
            "password",
            "private",
            "path-token",
            "access_token",
            "query-secret",
            "fragment-secret",
        ] {
            assert!(
                !message.contains(secret),
                "network error leaked sensitive URL component: {secret}"
            );
        }
    }

    #[test]
    fn subscription_mapping_accepts_proxy_sequences_and_provider_maps() {
        let url = Url::parse("https://example.com/profile.yaml")
            .expect("valid subscription mapping URL fixture");
        for yaml in [
            "proxies: []\n",
            "proxy-providers: {}\n",
            "proxies: []\nproxy-providers: {}\n",
        ] {
            validate_subscription_mapping(&url, &subscription_mapping(yaml))
                .expect("valid subscription structure must be accepted");
        }
    }

    #[test]
    fn subscription_mapping_rejects_invalid_top_level_keys() {
        let url = Url::parse("https://example.com/profile.yaml")
            .expect("valid subscription mapping URL fixture");
        for yaml in ["proxies: []\n1: value\n", "proxies: []\n\"\": value\n"] {
            let error = validate_subscription_mapping(&url, &subscription_mapping(yaml))
                .expect_err("invalid subscription top-level key must be rejected");
            assert!(error.to_string().contains("top-level keys"));
        }
    }

    #[test]
    fn subscription_mapping_rejects_missing_profile_sections() {
        let url = Url::parse("https://example.com/profile.yaml")
            .expect("valid subscription mapping URL fixture");

        let error = validate_subscription_mapping(&url, &subscription_mapping("dns: {}\n"))
            .expect_err("subscription without proxies or providers must be rejected");

        assert!(error.to_string().contains("does not contain"));
        assert!(error.to_string().contains("example.com"));
    }

    #[test]
    fn subscription_mapping_rejects_non_sequence_proxies() {
        let url = Url::parse("https://example.com/profile.yaml")
            .expect("valid subscription mapping URL fixture");

        let error = validate_subscription_mapping(
            &url,
            &subscription_mapping("proxies: invalid\nproxy-providers: {}\n"),
        )
        .expect_err("non-sequence proxies must be rejected");

        assert!(error.to_string().contains("must be a sequence"));
    }

    #[test]
    fn subscription_mapping_rejects_non_mapping_providers() {
        let url = Url::parse("https://example.com/profile.yaml")
            .expect("valid subscription mapping URL fixture");

        let error = validate_subscription_mapping(
            &url,
            &subscription_mapping("proxies: []\nproxy-providers: []\n"),
        )
        .expect_err("non-mapping providers must be rejected");

        assert!(error.to_string().contains("must be a mapping"));
    }

    #[tokio::test]
    async fn remote_profile_builder_rejects_missing_url_without_network_access() {
        let mut builder = RemoteProfileBuilder::default();

        let error = builder
            .build_no_blocking_unpersisted()
            .await
            .expect_err("remote profile builder without URL must be rejected");

        assert!(error.to_string().contains("url should not be null"));
    }

    #[test]
    fn remote_profile_options_apply_default_supplies_a_user_agent() {
        let options = RemoteProfileOptions::default().apply_default();

        assert!(
            options
                .user_agent
                .as_deref()
                .is_some_and(|user_agent| user_agent.starts_with("clash-chimera/v"))
        );
    }

    #[test]
    fn remote_profile_options_apply_default_preserves_an_explicit_user_agent() {
        let options = RemoteProfileOptions {
            user_agent: Some("custom-agent".into()),
            ..RemoteProfileOptions::default()
        }
        .apply_default();

        assert_eq!(options.user_agent.as_deref(), Some("custom-agent"));
    }

    #[test]
    fn remote_profile_update_interval_converts_hours_to_minutes() {
        let mut headers = HeaderMap::new();
        headers.insert("profile-update-interval", HeaderValue::from_static("2"));

        assert_eq!(profile_update_interval_minutes(&headers), Some(120));
    }

    #[test]
    fn remote_profile_update_interval_rejects_zero_and_invalid_values() {
        for value in ["0", "not-a-number", " 2 "] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "profile-update-interval",
                HeaderValue::from_str(value).expect("valid header fixture"),
            );
            assert_eq!(profile_update_interval_minutes(&headers), None);
        }
    }

    #[test]
    fn remote_profile_update_interval_rejects_multiplication_overflow() {
        let overflowing_hours = u64::MAX / 60 + 1;
        let mut headers = HeaderMap::new();
        headers.insert(
            "profile-update-interval",
            HeaderValue::from_str(&overflowing_hours.to_string())
                .expect("valid overflowing interval fixture"),
        );

        assert_eq!(profile_update_interval_minutes(&headers), None);
    }

    #[test]
    fn remote_profile_update_interval_accepts_the_largest_frontend_safe_hour_value() {
        let hours = MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES / 60;
        let mut headers = HeaderMap::new();
        headers.insert(
            "profile-update-interval",
            HeaderValue::from_str(&hours.to_string()).expect("valid maximum interval fixture"),
        );

        assert_eq!(profile_update_interval_minutes(&headers), Some(hours * 60));

        headers.insert(
            "profile-update-interval",
            HeaderValue::from_str(&(hours + 1).to_string())
                .expect("valid over-limit interval fixture"),
        );
        assert_eq!(profile_update_interval_minutes(&headers), None);
    }

    #[test]
    fn remote_profile_update_interval_deserialization_normalizes_legacy_unsafe_values() {
        for value in [0, MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES + 1] {
            let options: RemoteProfileOptions =
                serde_yaml::from_str(&format!("update_interval_minutes: {value}\n"))
                    .expect("legacy remote options must remain deserializable");
            assert_eq!(
                options.update_interval_minutes,
                super::default_update_interval_minutes()
            );
        }

        let options: RemoteProfileOptions = serde_yaml::from_str(&format!(
            "update_interval_minutes: {MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES}\n"
        ))
        .expect("maximum safe remote options must deserialize");
        assert_eq!(
            options.update_interval_minutes,
            MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES
        );
    }

    #[test]
    fn remote_profile_update_interval_builder_rejects_unsafe_values() {
        for value in [0, MAX_SAFE_PROFILE_UPDATE_INTERVAL_MINUTES + 1] {
            let mut builder = RemoteProfileOptionsBuilder::default();
            builder.update_interval_minutes(value);
            let error = validate_update_interval_builder(&builder)
                .expect_err("unsafe builder interval must be rejected");
            assert!(error.to_string().contains("must be between 1"));
        }
    }

    #[tokio::test]
    async fn declared_oversized_remote_profile_is_rejected_before_body_read() {
        let (response, server) = response_with_stalled_body(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/yaml\r\nContent-Length: 9\r\nConnection: close\r\n\r\n"
                .to_vec(),
        )
        .await;

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            response_text_with_limit(response, "utf-8", 8),
        )
        .await
        .expect("declared oversized response must not wait for the body")
        .expect_err("oversized declared response must be rejected");
        server.abort();

        let SubscribeError::ValidationFailed { reason, .. } = error else {
            panic!("oversized declared response must be a validation error");
        };
        assert!(reason.contains("maximum size of 8 bytes"));
    }

    #[tokio::test]
    async fn chunked_remote_profile_is_rejected_when_stream_crosses_limit() {
        let response = response_from_raw(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/yaml\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nabcd\r\n4\r\nefgh\r\n0\r\n\r\n"
                .to_vec(),
        )
        .await;

        let error = response_text_with_limit(response, "utf-8", 7)
            .await
            .expect_err("chunked response crossing the limit must be rejected");

        let SubscribeError::ValidationFailed { reason, .. } = error else {
            panic!("chunked overflow must be a validation error");
        };
        assert!(reason.contains("maximum size of 7 bytes"));
    }

    #[tokio::test]
    async fn remote_profile_response_accepts_exactly_the_limit() {
        let response = response_from_raw(fixed_response(b"abcd", "text/yaml")).await;

        let text = response_text_with_limit(response, "utf-8", 4)
            .await
            .expect("response exactly at the limit must be accepted");

        assert_eq!(text, "abcd");
    }

    #[tokio::test]
    async fn remote_profile_response_preserves_declared_charset_decoding() {
        let response = response_from_raw(fixed_response(
            b"name: caf\xe9",
            "text/yaml; charset=windows-1252",
        ))
        .await;

        let text = response_text_with_limit(response, "utf-8", 10)
            .await
            .expect("declared response charset must be decoded");

        assert_eq!(text, "name: café");
    }

    #[tokio::test]
    async fn truncated_remote_profile_response_is_reported_as_network_failure() {
        let response = response_from_raw(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/yaml\r\nContent-Length: 8\r\nConnection: close\r\n\r\nabcd"
                .to_vec(),
        )
        .await;

        let error = response_text_with_limit(response, "utf-8", 8)
            .await
            .expect_err("truncated response must not be accepted");

        assert!(matches!(error, SubscribeError::Network { .. }));
    }
}

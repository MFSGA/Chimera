//! Subscription ports extracted from the pinned ref/profiles/ports.rs.
//! File and materialization owners are migrated separately.

use chimera_config::profile::{RemoteProfileOptions, SubscriptionInfo};
use url::Url;

use super::error::SubscriptionFetchError;

/// Downloaded profile text and the subscription metadata advertised by its server.
#[derive(Debug, Clone)]
pub struct FetchedSubscription {
    pub content: String,
    /// Server-provided display name (`profile-title` / `Content-Disposition`).
    pub filename: Option<String>,
    pub subscription: SubscriptionInfo,
    /// Server-advertised `profile-update-interval`, normalized from hours to minutes.
    pub suggested_update_interval_minutes: Option<u64>,
    /// The media type of `Content-Type`, lowercased and without parameters.
    pub content_type: Option<String>,
    /// The file name in `Content-Disposition`.
    pub file_name: Option<String>,
}

/// Subscription download. Network timeouts are managed inside the
/// implementation (D9); content validation is the caller's concern (per
/// target profile kind, design fig. 13.3).
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait SubscriptionFetcher: Send + Sync + 'static {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> Result<FetchedSubscription, SubscriptionFetchError>;
}

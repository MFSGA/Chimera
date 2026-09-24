//! Running-core Clash API capability owned by ChimeraClient.
//!
//! This follows the reference client ownership direction while the core
//! lifecycle adapter is still backed by the legacy CoreManager.

use anyhow::Result;

use super::ChimeraClient;
use crate::core::clash::api::ApiClient;

impl ChimeraClient {
    pub(crate) async fn active_clash_info(&self) -> Result<crate::config::clash::ClashInfo> {
        self.inner.core.active_clash_info().await
    }

    pub(crate) async fn clash_api_client(&self) -> Result<ApiClient> {
        ApiClient::new(self.active_clash_info().await?)
    }
}

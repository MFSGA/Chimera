//! Running-core Clash API capability owned by ChimeraClient.
//!
//! This mirrors REF's `client/clash_api` ownership direction while the
//! lower-level core lifecycle is still backed by the legacy CoreManager.

use anyhow::Result;

use super::ChimeraClient;
use crate::core::clash::api::ApiClient;

impl ChimeraClient {
    pub(crate) fn clash_api_client(&self) -> Result<ApiClient> {
        ApiClient::new(self.clash_info())
    }
}

//! Running-core Clash API capability owned by ChimeraClient.
//!
//! This follows the reference client ownership direction while the core
//! lifecycle adapter is still backed by the legacy CoreManager.

use anyhow::Result;

use super::ChimeraClient;
use crate::core::clash::api::ApiClient;

impl ChimeraClient {
    pub(crate) fn clash_api_client(&self) -> Result<ApiClient> {
        ApiClient::new(self.clash_info())
    }
}

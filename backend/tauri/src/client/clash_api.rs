//! Running-core Clash API capability owned by ChimeraClient.
//!
//! This follows the reference client ownership direction and reads the active
//! runtime through the shared CoreFacade host boundary.

use anyhow::Result;

use super::ChimeraClient;
use crate::core::clash::api::ApiClient;

impl ChimeraClient {
    pub(crate) async fn active_core_binding(
        &self,
    ) -> Result<crate::client::core_lifecycle::ports::ActiveCoreBinding> {
        self.inner.core.active_core_binding().await
    }

    pub(crate) async fn active_clash_info(&self) -> Result<crate::config::clash::ClashInfo> {
        self.inner.core.active_clash_info().await
    }

    pub(crate) async fn clash_api_client(&self) -> Result<ApiClient> {
        ApiClient::new(self.active_clash_info().await?)
    }
}

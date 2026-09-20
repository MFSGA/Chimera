//! Running-core Clash API capability owned by ChimeraClient.
//!
//! This mirrors REF's `client/clash_api` ownership direction while the
//! lower-level core lifecycle is still backed by the legacy CoreManager.

use anyhow::Result;

use super::ChimeraClient;
use crate::core::clash::api::ApiClient;

impl ChimeraClient {
    pub(crate) async fn clash_api_client(&self) -> Result<ApiClient> {
        let status = self.core_status().await?;
        if let Some(connection) = self.inner.core.api_connection().await? {
            return ApiClient::from_connection(connection);
        }
        if status.run_type == crate::core::RunType::Service {
            anyhow::bail!("the running Service core did not publish an instance-bound API binding");
        }
        ApiClient::new(self.clash_info())
    }
}

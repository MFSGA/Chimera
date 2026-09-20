//! Legacy daemon core-control adapter.
//!
//! This isolates Chimera's current /core/start|stop|status wire behind one
//! injected boundary. The actor_v2 ServiceEndpoint can reuse this adapter
//! while the daemon wire is migrated toward ref's submit/wait protocol.

use std::{borrow::Cow, path::Path};

use async_trait::async_trait;
use chimera_ipc::{
    api::{core::start::CoreStartReq, status::CoreState},
    client::shortcuts::Client,
};

#[async_trait]
pub(crate) trait ServiceCoreHost: Send + Sync + std::fmt::Debug {
    async fn status(&self) -> anyhow::Result<(CoreState, i64)>;
    async fn start(
        &self,
        config_path: &Path,
        core_type: &chimera_utils::core::CoreType,
    ) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Default)]
pub(crate) struct LegacyServiceCoreHost;

#[async_trait]
impl ServiceCoreHost for LegacyServiceCoreHost {
    async fn status(&self) -> anyhow::Result<(CoreState, i64)> {
        let info = Client::service_default()
            .status()
            .await
            .map_err(anyhow::Error::from)?;
        Ok((info.core_infos.state, info.core_infos.state_changed_at))
    }

    async fn start(
        &self,
        config_path: &Path,
        core_type: &chimera_utils::core::CoreType,
    ) -> anyhow::Result<()> {
        let client = Client::service_default();

        // The service can survive app restarts. Stop an already running core
        // so the promoted config path and selected core are applied together.
        if matches!(self.status().await, Ok((CoreState::Running, _))) {
            client.stop_core().await?;
        }

        let payload = CoreStartReq {
            config_file: Cow::Owned(config_path.to_path_buf()),
            core_type: Cow::Borrowed(core_type),
        };
        match client.start_core(&payload).await {
            Ok(()) => Ok(()),
            Err(error)
                if error
                    .to_string()
                    .to_ascii_lowercase()
                    .contains("core is already running") =>
            {
                // Status can race with a concurrent daemon transition. Keep
                // the legacy one-shot stop/start recovery in this adapter.
                client.stop_core().await?;
                client
                    .start_core(&payload)
                    .await
                    .map_err(|error| anyhow::anyhow!("failed to start core: {error}"))
            }
            Err(error) => Err(anyhow::anyhow!("failed to start core: {error}")),
        }
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Client::service_default().stop_core().await?;
        Ok(())
    }
}

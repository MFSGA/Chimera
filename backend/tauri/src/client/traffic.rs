//! Desktop startup adapter for the shared traffic owner.
use super::ChimeraClient;
use chimera_core::clash::ws::ClashConnectionsFrame;
use std::sync::Arc;

impl ChimeraClient {
    pub(crate) fn start_traffic(
        &self,
        frames: tokio::sync::watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    ) -> anyhow::Result<()> {
        tauri::async_runtime::block_on(self.inner.shared.start_traffic(frames))
    }
}

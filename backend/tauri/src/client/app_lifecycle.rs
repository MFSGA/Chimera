//! The application lifecycle owned by the composed client.
//!
//! Follows ref/backend/nyanpasu-core/src/client/app_lifecycle.rs: the
//! successful client becomes the shutdown owner; partial startup remains
//! owned by the Tauri composition root.

use super::ChimeraClient;

impl ChimeraClient {
    /// Stops admission to all client-owned tasks. Repeated requests are safe.
    pub(crate) fn request_shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tasks.close();
    }

    /// Drains every client owner before stopping the core control plane.
    /// Unlike ref, retain the fallible shutdown result: Chimera's updater must
    /// not install a new binary if core shutdown is unconfirmed.
    pub(crate) async fn wait_shutdown(&self) -> anyhow::Result<()> {
        self.inner.tasks.wait().await;
        self.shutdown_core().await
    }
}

//! Application composition root.

use tauri::{Manager, Runtime};

use crate::client::ChimeraClient;

pub fn setup<R: Runtime, M: Manager<R>>(app: &M) -> anyhow::Result<()> {
    app.manage(ChimeraClient::legacy()?);
    Ok(())
}

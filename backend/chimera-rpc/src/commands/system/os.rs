//! OS integration: files, dialogs, the clipboard, notifications and shell actions.

use crate::{ports::HostEnvironment, registry::RpcState};

#[chimera_rpc::rpc(query, expose = [http])]
pub fn is_appimage(environment: RpcState<'_, HostEnvironment>) -> anyhow::Result<bool> {
    Ok(environment.is_appimage)
}

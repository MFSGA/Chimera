//! Tray and widget icons.

use anyhow::Result;
use chimera_paths::PathResolver;
use chimera_rpc_protocol::values::TrayIcon;

use crate::{ports::HostIconFiles, registry::RpcState};

chimera_rpc::resource!(route "icon_fetch", targets = [http]);

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn is_tray_icon_set(
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
) -> Result<bool> {
    Ok(icons.is_tray_icon_set(paths, mode).await)
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn is_widget_icon_set(
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
) -> Result<bool> {
    Ok(icons.is_widget_icon_set(paths, mode))
}

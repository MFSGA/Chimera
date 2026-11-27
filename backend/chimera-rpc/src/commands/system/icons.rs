//! Tray and widget icons.

use anyhow::Result;
use base64::{Engine as _, prelude::BASE64_STANDARD};
use chimera_core::client::ChimeraClient;
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

#[chimera_rpc::rpc(capabilities(presentation), expose = [http])]
pub async fn set_tray_icon_from_bytes(
    client: RpcState<'_, ChimeraClient>,
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
    bytes_base64: String,
) -> Result<()> {
    let bytes = decode_icon_upload(&bytes_base64, "Tray")?;
    icons.set_tray_icon_from_bytes(paths, mode, &bytes)?;
    client.request_tray_refresh()?;
    Ok(())
}

/// Decodes an uploaded icon of at most 1 MiB. `kind` names the icon in the
/// error.
fn decode_icon_upload(bytes_base64: &str, kind: &str) -> Result<Vec<u8>> {
    const MAX_ICON_BYTES: usize = 1024 * 1024;
    let too_large = || Err(anyhow::anyhow!("{kind} icon image exceeds the 1 MiB limit"));
    if bytes_base64.len() > MAX_ICON_BYTES.div_ceil(3) * 4 {
        return too_large();
    }
    let bytes = BASE64_STANDARD
        .decode(bytes_base64)
        .map_err(anyhow::Error::from)?;
    if bytes.len() > MAX_ICON_BYTES {
        return too_large();
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_upload_decodes_and_enforces_the_one_mib_limit() {
        let bytes = vec![0u8; 1024 * 1024];
        assert_eq!(
            decode_icon_upload(&BASE64_STANDARD.encode(&bytes), "Tray").unwrap(),
            bytes
        );
        assert!(
            decode_icon_upload(&BASE64_STANDARD.encode(vec![0u8; 1024 * 1024 + 1]), "Tray")
                .is_err()
        );
        assert!(decode_icon_upload("invalid!", "Tray").is_err());
    }
}

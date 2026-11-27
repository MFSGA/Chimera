//! Transport-neutral icon byte reads over existing application ports.

pub use chimera_rpc_protocol::icons::{
    IconBody, IconCachePolicy, IconRequest, SECURITY_HEADERS, parse,
};
use chimera_rpc_protocol::{RpcError, icons::allowed_content_type};

use crate::{error, registry::RpcDependencies};

/// Reads through the existing cache/host ports and checks MIME on every exit.
///
/// # Errors
/// Returns application failures, unavailable host files, or disallowed image MIME.
pub async fn read(
    dependencies: &RpcDependencies,
    visibility: chimera_rpc_protocol::ResponseVisibility,
    request: IconRequest,
) -> Result<IconBody, RpcError> {
    let (mime, bytes, cache_policy) = match request {
        IconRequest::Cached { url } => {
            let (mime, bytes) = dependencies
                .client
                .get_cached_icon(&url)
                .await
                .map_err(|error| error::application_error_for(visibility, error))?;
            (mime, bytes.to_vec(), IconCachePolicy::Remote)
        }
        IconRequest::Tray { mode, .. } | IconRequest::Widget { mode, .. } => {
            let icons = dependencies
                .icon_files
                .as_deref()
                .ok_or_else(|| RpcError::new("unavailable", "host icon files are unavailable"))?;
            let bytes = if matches!(request, IconRequest::Tray { .. }) {
                icons.tray_icon(&dependencies.paths, mode)
            } else {
                icons.widget_icon(&dependencies.paths, mode)
            };
            ("image/png".to_owned(), bytes, IconCachePolicy::Mutable)
        }
    };
    Ok(IconBody {
        content_type: allowed_content_type(&mime)
            .map_err(|error| RpcError::new("invalid_icon", error.to_string()))?,
        bytes,
        cache_policy,
    })
}

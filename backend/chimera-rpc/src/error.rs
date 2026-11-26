//! Fail closed for public diagnostics until typed domain projections are ported.
use chimera_rpc_protocol::{ResponseVisibility, RpcError};

pub fn application_error_for(
    visibility: ResponseVisibility,
    error: impl std::fmt::Display,
) -> RpcError {
    let message = match visibility {
        ResponseVisibility::Full => error.to_string(),
        ResponseVisibility::Public => "application command failed".into(),
    };
    RpcError::new("application_error", message)
}

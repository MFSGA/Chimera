//! Tauri's IPC constructor for the core-owned, ref-style endpoint factory.
//!
//! The factory and its readiness contract live in `chimera-core`. Only the
//! desktop owner-directory resolver stays here.

use std::sync::Arc;

pub use chimera_core::actor_v2::service_endpoint_factory::IpcServiceEndpointFactory;
use chimera_ipc::client::shortcuts::Client;

use chimera_core::actor_v2::endpoint::ServiceEndpoint;
use chimera_core_manager::{CoreError, CoreErrorKind};

/// Supplies Chimera's process-global IPC client without creating an
/// independent endpoint owner or probing the daemon during construction.
pub fn ipc_service_endpoint_factory(client: &'static Client<'static>) -> IpcServiceEndpointFactory {
    IpcServiceEndpointFactory::new(move || Arc::new(service_endpoint(client)))
}

pub fn service_endpoint(client: &'static Client<'static>) -> ServiceEndpoint {
    ServiceEndpoint::new(client, || {
        let config_dir = crate::utils::dirs::app_config_dir().map_err(|error| {
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("failed to resolve this application's config directory: {error}"),
                false,
            )
        })?;
        let data_dir = crate::utils::dirs::app_data_dir().map_err(|error| {
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("failed to resolve this application's data directory: {error}"),
                false,
            )
        })?;
        Ok((config_dir, data_dir))
    })
}

pub fn default_service_endpoint() -> ServiceEndpoint {
    service_endpoint(Client::service_default())
}

pub fn default_service_endpoint_factory() -> IpcServiceEndpointFactory {
    ipc_service_endpoint_factory(Client::service_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::actor_v2::{endpoint::ExecutionHost, service_router::ServiceEndpointFactory};

    #[test]
    fn factory_builds_a_service_host_endpoint_without_contacting_the_daemon() {
        let factory = default_service_endpoint_factory();
        assert_eq!(factory.endpoint().host(), ExecutionHost::Service);
    }
}

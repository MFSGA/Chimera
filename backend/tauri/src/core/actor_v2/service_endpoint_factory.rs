//! Tauri's IPC constructor for the core-owned, ref-style endpoint factory.
//!
//! The factory and its readiness contract live in `chimera-core`. Only the
//! platform-specific `ServiceEndpoint` adaptation stays here.

use std::sync::Arc;

pub use chimera_core::actor_v2::service_endpoint_factory::IpcServiceEndpointFactory;
use chimera_ipc::client::shortcuts::Client;

use super::service_endpoint::ServiceEndpoint;

/// Supplies Chimera's process-global IPC client without creating an
/// independent endpoint owner or probing the daemon during construction.
pub fn ipc_service_endpoint_factory(client: &'static Client<'static>) -> IpcServiceEndpointFactory {
    IpcServiceEndpointFactory::new(move || Arc::new(ServiceEndpoint::new(client)))
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

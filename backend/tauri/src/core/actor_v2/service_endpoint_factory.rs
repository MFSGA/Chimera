//! IPC endpoint construction after the service version gate, following
//! ref/backend/nyanpasu-core/src/actor_v2/service_endpoint_factory.rs.

use std::sync::Arc;

use chimera_core::actor_v2::{endpoint::EndpointHandle, service_router::ServiceEndpointFactory};

use super::service_endpoint::ServiceEndpoint;

/// Constructs the application endpoint only after ServiceClient passes the
/// daemon compatibility gate. The IPC handle is process-global in Chimera,
/// unlike ref's owned nyanpasu_ipc::client::Client.
pub struct IpcServiceEndpointFactory {
    client: &'static chimera_ipc::client::shortcuts::Client<'static>,
}

impl IpcServiceEndpointFactory {
    pub fn new(client: &'static chimera_ipc::client::shortcuts::Client<'static>) -> Self {
        Self { client }
    }

    pub fn service_default() -> Self {
        Self::new(chimera_ipc::client::shortcuts::Client::service_default())
    }
}

impl ServiceEndpointFactory for IpcServiceEndpointFactory {
    fn endpoint(&self) -> EndpointHandle {
        Arc::new(ServiceEndpoint::new(self.client))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::actor_v2::endpoint::ExecutionHost;

    #[test]
    fn factory_builds_a_service_host_endpoint_without_contacting_the_daemon() {
        let factory = IpcServiceEndpointFactory::service_default();
        assert_eq!(factory.endpoint().host(), ExecutionHost::Service);
    }
}

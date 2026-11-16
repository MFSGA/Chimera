//! Service endpoint creation after the daemon's version gate.
//!
//! The upstream `IpcServiceEndpointFactory` owns an IPC client and creates a
//! ServiceEndpoint only after `ServiceActor` has proved daemon readiness.
//! Chimera's IPC client is process-global and its endpoint adapter currently
//! lives in Tauri; inject that narrow construction operation here rather than
//! moving any Tauri or OS process concerns into the core crate.

use std::sync::Arc;

use super::{endpoint::EndpointHandle, service_router::ServiceEndpointFactory};

/// Core-owned factory with an OS/IPC-specific endpoint constructor.
pub struct IpcServiceEndpointFactory {
    make_endpoint: Arc<dyn Fn() -> EndpointHandle + Send + Sync>,
}

impl IpcServiceEndpointFactory {
    pub fn new(make_endpoint: impl Fn() -> EndpointHandle + Send + Sync + 'static) -> Self {
        Self {
            make_endpoint: Arc::new(make_endpoint),
        }
    }
}

impl ServiceEndpointFactory for IpcServiceEndpointFactory {
    fn endpoint(&self) -> EndpointHandle {
        (self.make_endpoint)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_v2::endpoint::{
        ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    };
    use chimera_core_manager::{CoreError, OperationId};
    use chimera_ipc::api::core::v2::OperationInfo;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    struct FakeServiceEndpoint;

    #[async_trait::async_trait]
    impl ControlEndpoint for FakeServiceEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Service
        }

        async fn submit(&self, _submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
            unreachable!("endpoint construction must not submit any core command")
        }

        async fn wait_operation(
            &self,
            _id: OperationId,
            _timeout: Duration,
        ) -> Option<OperationInfo> {
            unreachable!("endpoint construction must not wait for a core operation")
        }

        async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
            unreachable!("endpoint construction must not probe the daemon")
        }
    }

    #[test]
    fn constructs_one_service_endpoint_per_adoption_without_running_commands() {
        let creations = Arc::new(AtomicUsize::new(0));
        let captured = creations.clone();
        let factory = IpcServiceEndpointFactory::new(move || {
            captured.fetch_add(1, Ordering::SeqCst);
            Arc::new(FakeServiceEndpoint)
        });
        assert_eq!(creations.load(Ordering::SeqCst), 0);
        assert_eq!(factory.endpoint().host(), ExecutionHost::Service);
        assert_eq!(factory.endpoint().host(), ExecutionHost::Service);
        assert_eq!(creations.load(Ordering::SeqCst), 2);
    }
}

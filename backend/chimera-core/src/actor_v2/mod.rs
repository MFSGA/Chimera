pub mod api;
pub mod endpoint;
pub mod intent;
pub mod routed_endpoint;
pub mod router;
pub mod service_actor;
pub mod service_command_port;
pub mod service_endpoint_factory;
pub mod service_policy;
pub mod service_process;
pub mod service_router;
pub mod service_status;

// Match ref's public actor handle path; legacy Facade adapters remain until
// Tauri's composition root switches owners as one operation.
pub use router::CoreClient;

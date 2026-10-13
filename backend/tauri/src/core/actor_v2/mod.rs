//! Lower core-host facade boundary aligned with the ref actor_v2 layout.
//!
//! Production local execution uses the shared CoreControl host and typed
//! Profile inputs through `LocalRuntimeHost`. The older manager-only path is
//! retained behind a test-only compatibility constructor while callers finish
//! migrating to the shared runtime contract.

pub mod control_endpoint;
pub mod endpoint;
pub mod facade;
pub mod intent;
pub mod local_host;
mod local_runtime;
pub mod service_endpoint;

pub(crate) use endpoint::CoreStatusSnapshot;
pub(crate) use facade::CoreFacade;

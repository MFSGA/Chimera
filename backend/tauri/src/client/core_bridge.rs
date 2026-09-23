//! Compatibility re-exports for the pre-ref core bridge path.
//!
//! New code should import the ports and adapters from `core_lifecycle`.
//! Keeping this shim avoids breaking the staged migration or downstream
//! legacy callers while the actor-backed lifecycle is introduced.

#[allow(unused_imports)]
pub(crate) use super::core_lifecycle::adapters::{LegacyCoreBridge, LegacyRunningConfigBridge};
#[allow(unused_imports)]
pub(crate) use super::core_lifecycle::ports::{
    CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot, RunningConfigPort,
    RuntimeTransformDiagnostics, RuntimeTransformFailureDiagnostics,
};

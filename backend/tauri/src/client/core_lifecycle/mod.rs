//! Lower Core/Service lifecycle boundary and compatibility exports.
//! Runtime mutation admission and its single mailbox live in reconciler.

pub(crate) mod adapters;
pub(in crate::client) mod apply;
mod ownership;
pub(crate) mod ports;
mod service;
pub(in crate::client) mod workflow;

#[allow(unused_imports)]
pub(in crate::client) use super::reconciler::CompletedLifecycleOperation;
#[allow(unused_imports)]
pub(crate) use super::reconciler::CoreLifecycleOperationResult;
pub(crate) use super::reconciler::{
    CoreLifecycleStatus, RuntimeReconcilerClient as CoreLifecycleClient,
};
pub(in crate::client) use workflow::{Command, CoreLifecycleWorkflow};

#[allow(unused_imports)]
pub(crate) use adapters::{ActorRunningConfigBridge, LegacyCoreBridge, LegacyServiceBridge};
#[allow(unused_imports)]
pub(crate) use ports::{
    BinaryInstallProgress, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
    RunningConfigPort, RuntimeTransformDiagnostics, ServiceLifecyclePort, ServiceTransitionLease,
};

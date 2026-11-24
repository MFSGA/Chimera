//! Runtime application and lifecycle operations share one actor mailbox.
//! Lower host commands remain delegated to the CoreLifecycle ports.

mod actor;
pub(in crate::client) use actor::CompletedLifecycleOperation;
pub(crate) use actor::{
    CoreLifecycleOperationResult, CoreLifecycleStatus, RuntimeReconcilerClient,
};

pub(crate) mod apply;
pub(crate) mod attempt;
pub(in crate::client) mod authority;
pub(in crate::client) mod check;
pub(in crate::client) mod checked;
pub(crate) mod committed;
pub(in crate::client) mod composition;
pub(crate) mod decision;
pub(in crate::client) mod inputs;
// Only isolated test fixtures still need the old CoreLifecycle validator.
#[cfg(test)]
pub(crate) mod legacy_check;
pub(in crate::client) mod legacy_preparation;
pub(in crate::client) mod legacy_source;
pub(crate) mod mutation;
pub(in crate::client) mod ports;
pub(in crate::client) mod preparation;
pub(crate) mod proof;
pub(crate) mod release;

pub(crate) mod startup;
pub(crate) mod state;
pub(crate) mod status;

// Pure upstream planner is already the single shared implementation in chimera-core.
pub(crate) use chimera_core::client::reconciler::plan::Conclusion;

pub(crate) mod workflow;

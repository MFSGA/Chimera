//! Ref candidate checking, now used by the Profiles production source gate.
//!
//! The existing CoreLifecycle Actor remains the single mutating runtime
//! owner; Application/Clash and the remaining ref workflow are still partial.

pub(in crate::client) mod adapters;
pub(crate) mod apply;
pub(crate) mod attempt;
pub(in crate::client) mod authority;
pub(in crate::client) mod check;
pub(crate) mod committed;
pub(in crate::client) mod composition;
pub(crate) mod decision;
pub(in crate::client) mod inputs;
// Only isolated test fixtures still need the old CoreLifecycle validator.
#[cfg(test)]
pub(crate) mod legacy_check;
pub(crate) mod legacy_mutation;
pub(in crate::client) mod legacy_preparation;
pub(crate) mod mutation;
pub(in crate::client) mod ports;
pub(in crate::client) mod preparation;
pub(crate) mod profiles;
pub(crate) mod proof;
pub(crate) mod release;

pub(crate) mod legacy_status;
pub(crate) mod startup;
pub(crate) mod state;
pub(crate) mod status;

// Pure upstream planner is already the single shared implementation in chimera-core.
pub(crate) use chimera_core::client::reconciler::plan::Conclusion;

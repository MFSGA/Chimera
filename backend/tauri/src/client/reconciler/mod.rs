//! Independent candidate checking ahead of the unified ref RuntimeGate actor.
//!
//! There is still a single mutating CoreLifecycle owner in Chimera. The
//! check boundary can migrate before its postcommit reconcile mailbox.

pub(in crate::client) mod adapters;
pub(crate) mod attempt;
pub(in crate::client) mod check;
pub(crate) mod committed;
pub(crate) mod decision;
pub(in crate::client) mod inputs;
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
pub(crate) mod status;

// Pure upstream planner is already the single shared implementation in chimera-core.
pub(crate) use chimera_core::client::reconciler::plan::Conclusion;

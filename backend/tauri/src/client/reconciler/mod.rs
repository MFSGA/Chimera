//! Independent candidate checking ahead of the unified ref RuntimeGate actor.
//!
//! There is still a single mutating CoreLifecycle owner in Chimera. The
//! check boundary can migrate before its postcommit reconcile mailbox.

pub(in crate::client) mod adapters;
pub(crate) mod attempt;
pub(crate) mod check;
pub(crate) mod committed;
pub(crate) mod decision;
pub(in crate::client) mod inputs;
pub(crate) mod mutation;
pub(in crate::client) mod ports;
pub(in crate::client) mod preparation;
pub(crate) mod profiles;
pub(crate) mod proof;
pub(crate) mod release;
pub(crate) mod status;

//! Independent candidate checking ahead of the unified ref RuntimeGate actor.
//!
//! There is still a single mutating CoreLifecycle owner in Chimera. The
//! check boundary can migrate before its postcommit reconcile mailbox.

pub(crate) mod attempt;
pub(crate) mod check;
pub(crate) mod committed;
pub(crate) mod status;

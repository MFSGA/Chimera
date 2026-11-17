//! Compatibility path for the ref-aligned convergence policy in chimera-core.
//! Runtime backoff and health vocabulary have a single shared definition.

pub use chimera_core::client::convergence::ConvergenceHealth;
pub(crate) use chimera_core::client::convergence::{
    OutcomeClass, RETRY_DELAYS, RetryBudget, next_wait,
};

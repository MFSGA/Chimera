//! Startup-injected Profiles candidate checks and committed-source notifications.

mod change;
pub(crate) use change::DomainChange;
pub(crate) mod impact;
mod ports;
pub(crate) use ports::{RuntimeCandidate, RuntimeGate};
mod profile_content;
pub(crate) mod runtime_apply;
pub(crate) use profile_content::{ProfileContentLock, ProfileContentReader};

#[cfg(test)]
mod legacy_tests;
#[cfg(test)]
pub(crate) use legacy_tests::MutationCoordinator;

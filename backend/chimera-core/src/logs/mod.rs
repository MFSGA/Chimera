//! Redb log storage and codec adapters for the observation owner.

mod codec;
mod redb;

pub use chimera_observers::logs::{CoreLogStore, CoreLogsClient, UnavailableCoreLogStore};
pub use redb::RedbCoreLogStore;

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub const LOG_CAP: u64 = 16 * 1024 * 1024;
pub const LOG_AGE: std::time::Duration = std::time::Duration::from_secs(3 * 24 * 60 * 60);

pub mod query;

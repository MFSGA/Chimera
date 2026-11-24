//! The country index traffic regions are looked up in, built from the database
//! the running core reads and kept in step with it.
mod actor;
mod adapters;
mod client;
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
mod ports;
#[cfg(test)]
mod tests;

pub use actor::GeoIndexArgs;
pub use adapters::FsCountryIndexSource;
pub use client::GeoIndexClient;
#[cfg(any(test, feature = "test-support"))]
pub use ports::MockCountryIndexSource;
pub use ports::{
    CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, CountryIndexSource,
    GeoIndexError, GeodataMode, IndexKey, Loaded, OnChange,
};

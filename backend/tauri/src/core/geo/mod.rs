//! Desktop compatibility exports. The country index owner and filesystem
//! adapter live in chimera-core; no desktop-specific actor or watcher remains.
pub use chimera_core::geo::{
    CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, CountryIndexSource,
    FsCountryIndexSource, GeoIndexArgs, GeoIndexClient, GeodataMode,
};
#[cfg(test)]
pub(crate) use chimera_core::geo::{IndexKey, Loaded, MockCountryIndexSource, fixtures};

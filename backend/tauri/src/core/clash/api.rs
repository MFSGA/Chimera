//! Compatibility facade. The real Clash DTO implementation now lives in the
//! independent `chimera-core` crate, following ref's `core::clash::api` path.
//! Keep existing Tauri call sites and Specta export names intact during migration.
pub use chimera_core::clash::api::*;

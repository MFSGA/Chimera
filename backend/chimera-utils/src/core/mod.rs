//! Core process helpers.
//!
//! The shared identity types come from the Chimera platform crate. Process
//! supervision is implemented locally so this workspace can evolve the
//! lifecycle without changing the wire type used by `chimera-ipc`.

pub use chimera_platform_utils::core::{
    ClashCoreType, CommandEvent, CoreMetaData, CoreType, CoresMetaMap, TerminatedPayload,
};

pub mod instance;
pub mod prelude;
pub mod utils;

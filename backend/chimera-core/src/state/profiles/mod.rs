//! Profiles domain state (PR-3). Tauri-free.

pub mod actor;
pub mod jobs;
pub mod ports;
mod scheduler;

pub use actor::*;

pub use chimera_core_manager::control::payload_digest;

//! Shared utility surface for Chimera applications and services.
//!
//! Runtime, directory, network, operating-system, and core process helpers.
//!
//! Portable runtime, directory, network, and operating-system helpers are
//! implemented in this crate. Core identity types are re-exported from the
//! Chimera platform package so the app and its IPC client continue to share
//! the same `CoreType`, including the `ChimeraClient` variant.

#[cfg(feature = "core_manager")]
#[macro_use]
extern crate derive_builder;

#[cfg(feature = "core_manager")]
pub mod core;
#[cfg(feature = "dirs")]
pub mod dirs;
#[cfg(feature = "network")]
pub mod network;
#[cfg(feature = "os")]
pub mod os;
pub mod runtime;

pub mod io;

#[cfg(feature = "process")]
pub mod process;

#[cfg(feature = "reqwest")]
pub mod reqwest_ext;

//! Shared utility surface for Chimera applications and services.
//!
//! Product-specific core, OS, directory, network, and runtime helpers are
//! provided by the Chimera platform utilities package. Process supervision,
//! epoch-bound pid files, atomic filesystem operations, and named-pipe retry
//! behavior live here so Chimera's core manager does not depend on a separate
//! application crate.

#[cfg(feature = "core_manager")]
pub use chimera_platform_utils::core;
#[cfg(feature = "dirs")]
pub use chimera_platform_utils::dirs;
#[cfg(feature = "network")]
pub use chimera_platform_utils::network;
#[cfg(feature = "os")]
pub use chimera_platform_utils::os;
pub use chimera_platform_utils::runtime;

pub mod io;

#[cfg(feature = "process")]
pub mod process;

#[cfg(feature = "reqwest")]
pub mod reqwest_ext;

//! Lower core-host facade boundary aligned with the ref actor_v2 layout.
//!
//! Chimera does not yet implement the ref submit/wait operation protocol here.
//! This module owns the legacy CoreManager behind a narrow facade so client
//! lifecycle orchestration no longer depends on the concrete manager.

pub mod endpoint;
pub mod facade;

pub(crate) use endpoint::CoreStatusSnapshot;
pub(crate) use facade::CoreFacade;

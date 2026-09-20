//! Lower core-host control boundary aligned with the ref actor_v2 layout.
//!
//! The local endpoint owns the legacy CoreManager behind a submit/wait/status
//! protocol, while the facade maps application lifecycle workflow onto that
//! endpoint. Service-host routing and revision-aware operation outputs remain
//! staged follow-up work.

pub mod endpoint;
pub mod facade;

pub(crate) use endpoint::CoreStatusSnapshot;
pub(crate) use facade::CoreFacade;

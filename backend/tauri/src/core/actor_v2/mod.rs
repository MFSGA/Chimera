//! Lower core-host control boundary aligned with the ref actor_v2 layout.
//!
//! The local endpoint owns the legacy CoreManager behind a submit/wait/status
//! protocol, while the facade maps application lifecycle workflow onto that
//! endpoint. Successful local operations publish typed terminal outputs with
//! the applied runtime identity where available, and reconcile uses an
//! expected-applied revision CAS. Service-host routing remains staged follow-up
//! work.

pub mod endpoint;
pub mod facade;

pub(crate) use endpoint::CoreStatusSnapshot;
pub(crate) use facade::CoreFacade;

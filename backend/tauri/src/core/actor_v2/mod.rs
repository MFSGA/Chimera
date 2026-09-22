//! Lower core-host control boundary aligned with the ref actor_v2 layout.
//!
//! The local endpoint owns the legacy CoreManager behind a submit/wait/status
//! protocol, while the facade maps application lifecycle workflow onto that
//! endpoint. Successful local operations publish typed terminal outputs with
//! the applied runtime identity where available, and reconcile uses an
//! expected-applied revision CAS. Privileged daemon commands are serialized by
//! a staged ServiceActor; daemon core-control endpoint routing remains follow-up.

pub mod endpoint;
pub mod facade;
pub mod intent;
pub mod service_actor;

pub(crate) use endpoint::CoreStatusSnapshot;
pub(crate) use facade::CoreFacade;

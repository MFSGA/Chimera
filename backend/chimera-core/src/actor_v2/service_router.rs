//! Compatibility import path for the ref-aligned ServiceActor.
//! New code should use `actor_v2::service_actor` directly.

pub use super::service_actor::{
    ServiceActor, ServiceActorArgs, ServiceActorMessage, ServiceActorState, ServiceClient,
    ServiceCommandKind, ServiceEndpointFactory, ServiceHostStatus, ServicePhase,
};

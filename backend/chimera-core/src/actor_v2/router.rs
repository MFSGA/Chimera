//! Compatibility import path for the ref-aligned CoreActor in `actor_v2`.
//! New code should use the canonical `actor_v2` exports.

pub use super::{
    ControllerGeneration, CoreActor, CoreActorArgs, CoreActorMessage, CoreActorState, CoreClient,
    CoreObserver, CoreStatusInfo, CoreStatusProjection, EndpointConnectivity, HandoffReport,
    ShutdownReport, SubmitFailure, SubmitTicket,
};

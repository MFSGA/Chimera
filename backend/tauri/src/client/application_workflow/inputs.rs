//! Compatibility re-export while ApplicationWorkflow still handles mutations.
//! The canonical input ownership follows ref: client/reconciler/inputs.rs.

pub(in crate::client) use crate::client::reconciler::inputs::{
    FrozenProfileContent, RuntimeInputs,
};

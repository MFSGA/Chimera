//! Mutation receipt projection for write-authorized shared commands.
//! All linked privacy declarations require write on their own resource,
//! so admission guarantees Full. The public domain projector is not yet linked.
use crate::audit::{CompletionSummary, summarize_mutation, summarize_public_mutation};
use chimera_rpc_protocol::{
    ResponseVisibility, RpcError,
    types::{
        RpcFolderMutationOutcome, RpcMutationOutcome, RpcProfileMutationOutcome,
        RpcUnitMutationOutcome,
    },
};

/// Run before invoking a handler, not after a durable commit.
pub fn require_full_visibility(visibility: ResponseVisibility) -> Result<(), RpcError> {
    if visibility == ResponseVisibility::Full {
        Ok(())
    } else {
        Err(RpcError::forbidden(
            "public mutation projection unavailable",
        ))
    }
}

pub trait RpcResponseProjection: Sized {
    fn summarize_completion(&self) -> CompletionSummary;
    fn encode_for_visibility(
        self,
        visibility: ResponseVisibility,
    ) -> Result<serde_json::Value, RpcError>;
}

impl<T: serde::Serialize> RpcResponseProjection for RpcMutationOutcome<T> {
    fn summarize_completion(&self) -> CompletionSummary {
        match self {
            Self::Full(outcome) => summarize_mutation(outcome),
            Self::Public(outcome) => summarize_public_mutation(outcome),
        }
    }
    fn encode_for_visibility(
        self,
        visibility: ResponseVisibility,
    ) -> Result<serde_json::Value, RpcError> {
        let encoded = match (visibility, self) {
            (ResponseVisibility::Full, Self::Full(outcome)) => serde_json::to_value(outcome),
            (_, Self::Public(outcome)) => serde_json::to_value(outcome),
            (ResponseVisibility::Public, Self::Full(_)) => {
                return Err(RpcError::forbidden(
                    "public mutation projection unavailable",
                ));
            }
        };
        encoded.map_err(|error| crate::error::application_error_for(visibility, error))
    }
}

macro_rules! project_wrapper {
    ($($ty:ty),+ $(,)?) => {$(
        impl RpcResponseProjection for $ty {
            fn summarize_completion(&self) -> CompletionSummary { self.0.summarize_completion() }
            fn encode_for_visibility(self, visibility: ResponseVisibility) -> Result<serde_json::Value, RpcError> {
                self.0.encode_for_visibility(visibility)
            }
        }
    )+};
}
project_wrapper!(
    RpcUnitMutationOutcome,
    RpcProfileMutationOutcome,
    RpcFolderMutationOutcome
);

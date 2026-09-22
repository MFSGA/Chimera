//! Service endpoint error adapter.
//!
//! Service lifecycle execution is owned by `core::actor_v2::endpoint` and the
//! daemon v2 control plane. This module only preserves typed server errors and
//! marks transport failures whose mutation outcome cannot be proven.

use std::borrow::Cow;

use chimera_ipc::{api::CoreErrorKind, client::ClientError};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct ServiceCoreOutcomeUncertain(String);

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ServiceOperationError {
    message: String,
    error_kind: Option<String>,
    retryable: bool,
}

impl ServiceOperationError {
    pub(crate) fn core_error_kind(&self) -> Option<CoreErrorKind> {
        self.error_kind
            .as_deref()
            .and_then(CoreErrorKind::from_wire)
    }

    pub(crate) fn retryable(&self) -> bool {
        self.retryable
    }
}

pub(crate) fn is_outcome_uncertain(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<ServiceCoreOutcomeUncertain>()
            .is_some()
    })
}

fn uncertain(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(ServiceCoreOutcomeUncertain(message.into()))
}

pub(crate) fn submit_error(error: ClientError<'_>) -> anyhow::Error {
    match error {
        ClientError::ServerResponseFailed(response) => {
            let error_kind = response.error_kind.map(Cow::into_owned);
            let retryable = response.retryable.unwrap_or_else(|| {
                error_kind
                    .as_deref()
                    .and_then(CoreErrorKind::from_wire)
                    .is_some_and(|kind| kind.default_retryable())
            });
            let error = ServiceOperationError {
                message: response.msg.into_owned(),
                error_kind,
                retryable,
            };
            if let Some(kind) = error.core_error_kind() {
                tracing::debug!(
                    %kind,
                    retryable = error.retryable(),
                    "service operation admission failed with a typed classification"
                );
            }
            anyhow::Error::new(error)
        }
        error => uncertain(format!(
            "service core operation admission reply was lost; outcome is uncertain: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_error_preserves_typed_server_classification() {
        let response = chimera_ipc::api::RBuilder::<Option<()>>::other_error_with_kind(
            Cow::Borrowed("operation conflict"),
            Some(CoreErrorKind::OperationConflict),
            Some(false),
        );
        let error = submit_error(ClientError::ServerResponseFailed(response));
        let classified = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<ServiceOperationError>())
            .expect("typed service operation error");

        assert_eq!(
            classified.core_error_kind(),
            Some(CoreErrorKind::OperationConflict)
        );
        assert!(!classified.retryable());
        assert!(classified.to_string().contains("operation conflict"));
    }

    #[test]
    fn uncertain_marker_is_discoverable_through_anyhow_context_chain() {
        let error = uncertain("query lost").context("service apply failed");
        assert!(is_outcome_uncertain(&error));
        assert!(!is_outcome_uncertain(&anyhow::anyhow!(
            "known terminal failure"
        )));
    }
}

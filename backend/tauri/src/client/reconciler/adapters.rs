use async_trait::async_trait;

use chimera_core::actor_v2::{
    CoreClient,
    endpoint::{CheckSubmission, CheckSupport},
};

/// Checks inline documents through the session's local control plane.
///
/// The local endpoint stays alive while the service owns the running core.
/// Its read-only check runs outside the mutating queue and is awaited to completion.
pub(in crate::client) struct CoreCheckValidator {
    core: CoreClient,
}

/// The phrase the runtime puts in the message when its own check bound
/// elapses. The kind is `ConfigCheckFailed` either way, so the message is the
/// only thing that tells a timeout apart from a verdict — and the runtime
/// writes it deliberately, "so a slow core is diagnosable".
const RUNTIME_CHECK_TIMEOUT: &str = "config check timed out after ";

impl CoreCheckValidator {
    pub(in crate::client) fn new(core: &CoreClient) -> Self {
        Self { core: core.clone() }
    }
}

#[async_trait]
impl super::ports::RuntimeValidatorPort for CoreCheckValidator {
    async fn check(
        &self,
        request: super::ports::RuntimeCheckRequest<'_>,
    ) -> super::ports::RuntimeCheckOutcome {
        let support = self
            .core
            .check_config(CheckSubmission {
                core_spec: request.core_spec,
                core_type: request.intent.core_type.clone(),
                config_bytes: request.intent.config_text.as_bytes().to_vec(),
                digest: request.intent.digest.clone(),
                staged_config: None,
            })
            .await;
        classify_check_support(support)
    }
}

/// The pinned-ref decision tree, extracted so the distinct Core-check verdicts
/// remain covered without starting a second runtime mutation owner.
fn classify_check_support(support: CheckSupport) -> super::ports::RuntimeCheckOutcome {
    use super::ports::RuntimeCheckOutcome;
    use chimera_contracts::runtime_error::RuntimeCheckUnavailable;
    match support {
        CheckSupport::Ran(Ok(())) => RuntimeCheckOutcome::Passed,
        CheckSupport::Ran(Err(error)) => {
            // Only the core's own verdict on the document is a rejection,
            // and its own check timeout is not one: it arrives under the
            // same kind but says the binary never answered, which is
            // retryable and must keep the deferral the failure matrix
            // allows for an unreachable check.
            if error.message.contains(RUNTIME_CHECK_TIMEOUT) {
                RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                    kind: error.kind,
                    message: error.message,
                    retryable: true,
                })
            } else if matches!(
                error.kind,
                Some(
                    chimera_core_manager::CoreErrorKind::ConfigCheckFailed
                        | chimera_core_manager::CoreErrorKind::InvalidConfig
                )
            ) {
                RuntimeCheckOutcome::Rejected {
                    kind: error.kind,
                    message: error.message,
                }
            } else {
                RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                    kind: error.kind,
                    message: error.message,
                    retryable: error.retryable,
                })
            }
        }
        CheckSupport::Unsupported { reason } => {
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                kind: None,
                message: reason,
                retryable: false,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ports::RuntimeCheckOutcome;
    use super::*;
    use chimera_contracts::runtime_error::RuntimeCheckUnavailable;
    use chimera_core_manager::{CoreError, CoreErrorKind};

    #[test]
    fn ref_check_reports_positive_and_deterministic_negative_verdicts() {
        assert!(matches!(
            classify_check_support(CheckSupport::Ran(Ok(()))),
            RuntimeCheckOutcome::Passed
        ));
        for kind in [
            CoreErrorKind::ConfigCheckFailed,
            CoreErrorKind::InvalidConfig,
        ] {
            let verdict = classify_check_support(CheckSupport::Ran(Err(CoreError::new(
                kind,
                "bad document",
                false,
            ))));
            assert!(
                matches!(verdict, RuntimeCheckOutcome::Rejected {kind: Some(k), message} if k == kind && message == "bad document")
            );
        }
    }

    #[test]
    fn ref_check_timeout_and_backend_failures_never_become_rejections() {
        let timed_out = classify_check_support(CheckSupport::Ran(Err(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "config check timed out after 30s",
            false,
        ))));
        assert!(matches!(
            timed_out,
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                retryable: true,
                ..
            })
        ));
        let backend_down = classify_check_support(CheckSupport::Ran(Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "no endpoint",
            true,
        ))));
        assert!(matches!(
            backend_down,
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                retryable: true,
                ..
            })
        ));
        let not_supported = classify_check_support(CheckSupport::Unsupported {
            reason: "unsupported".into(),
        });
        assert!(
            matches!(not_supported, RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {retryable: false, message, .. }) if message == "unsupported")
        );
    }
}

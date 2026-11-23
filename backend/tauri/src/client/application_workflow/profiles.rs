use crate::client::runtime;

/// Map crate-internal profile materialization degradations onto the public
/// wire. Actor-internal Cleanup/Reconcile phases collapse to
/// `ProfileMaterialization`; retryability stays code-derived.
pub(in crate::client) fn map_profile_degradation(
    degradation: &crate::state::profiles::ports::ProfileDegradation,
) -> runtime::Degradation {
    runtime::Degradation::from_shared(chimera_contracts::runtime::Degradation {
        phase: runtime::DegradationPhase::ProfileMaterialization,
        reason: degradation.code.into(),
        message: degradation.message.clone(),
        retryable: degradation.code.retryable(),
    })
}

#[cfg(test)]
mod shared_profile_degradations_tests {
    use super::*;
    use chimera_contracts::profiles::ports::{
        ProfileDegradation, ProfileDegradationCode, ProfileDegradationPhase,
    };

    #[test]
    fn profile_maintenance_causes_use_upstream_codes_without_changing_ipc() {
        for (code, expected, retryable) in [
            (
                ProfileDegradationCode::JournalInvalid,
                "journal_invalid",
                false,
            ),
            (
                ProfileDegradationCode::MaterializationDeferred,
                "materialization_deferred",
                true,
            ),
            (
                ProfileDegradationCode::CleanupDeferred,
                "cleanup_deferred",
                true,
            ),
        ] {
            let port = ProfileDegradation {
                phase: ProfileDegradationPhase::Reconcile,
                code,
                message: "profile maintenance".into(),
            };
            let output = map_profile_degradation(&port);
            assert_eq!(
                output.phase,
                runtime::DegradationPhase::ProfileMaterialization
            );
            assert_eq!(output.code, expected);
            assert_eq!(output.retryable, retryable);
            assert_eq!(output.message, "profile maintenance");
            assert!(
                serde_json::to_value(output)
                    .unwrap()
                    .get("reason")
                    .is_none()
            );
        }
    }
}

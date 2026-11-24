use crate::client::runtime;

/// Legacy desktop wire bridge; the domain mapping now follows the copied
/// ref/client/reconciler/profiles.rs implementation.
pub(in crate::client) fn map_profile_degradation(
    degradation: &crate::state::profiles::ports::ProfileDegradation,
) -> runtime::Degradation {
    runtime::Degradation::from_shared(
        chimera_core::client::reconciler::profiles::map_profile_degradation(degradation),
    )
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

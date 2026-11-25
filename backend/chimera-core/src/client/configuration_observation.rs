//! Shared configuration observation values.
pub use chimera_runtime_values::observation::{
    ConfigurationStatusSummary, EffectConvergenceSummary, OperationSummary,
    RuntimeConvergenceSummary, SourceOutcomeSummary, SourceRejection, SourceSummary,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{configuration::*, convergence::ConvergenceHealth};
    use chimera_config::profile::ProfileId;
    use serde::Serialize;
    use serde_json::json;
    // Hostile values: a credential in a URL path, host, userinfo, query, header, bearer and secret field.
    const SENTINELS: [&str; 7] = [
        "sentinel-path-token",
        "sentinel-host-token.example",
        "sentinel-user:sentinel-pass",
        "sentinel-query-token",
        "sentinel-header-token",
        "sentinel-bearer-token",
        "sentinel-clash-secret",
    ];

    fn assert_clean(value: &impl Serialize) {
        let text = serde_json::to_string(value).unwrap();
        for sentinel in SENTINELS {
            assert!(!text.contains(sentinel), "{sentinel} leaked: {text}");
        }
    }

    fn hostile() -> String {
        format!(
            "GET https://{}@{}/{}/sub?token={} Authorization: Bearer {} {} {}",
            SENTINELS[2],
            SENTINELS[1],
            SENTINELS[0],
            SENTINELS[3],
            SENTINELS[5],
            SENTINELS[4],
            SENTINELS[6]
        )
    }

    fn configuration() -> ConfigurationStatus {
        let profile: ProfileId = serde_json::from_value(json!("pAbc1234")).unwrap();
        ConfigurationStatus {
            event_seq: 9,
            maintenance: Some(hostile()),
            source_versions: SourceVersions {
                application: 1,
                clash: 2,
                session: 3,
                profiles: 4,
            },
            runtime: RuntimeConvergence {
                health: ConvergenceHealth::RetryScheduled,
                operation_id: Some("op-1".into()),
                message: Some(hostile()),
            },
            effects: vec![EffectConvergence {
                kind: EffectKind::SystemProxy,
                health: ConvergenceHealth::Blocked,
                desired_revision: 5,
                applied_revision: 4,
                attempts: 3,
                automatic_remaining: 0,
                code: Some(EffectFailureCode::SystemProxyApplyFailed),
                message: Some(hostile()),
            }],
            sources: vec![
                SourceStatus {
                    profile: profile.clone(),
                    name: hostile(),
                    origin: SourceOrigin::ScheduledRefresh,
                    outcome: SourceOutcome::Failed { message: hostile() },
                    health: ConvergenceHealth::Blocked,
                    at: 7,
                },
                SourceStatus {
                    profile: profile.clone(),
                    name: hostile(),
                    origin: SourceOrigin::ManualRefresh,
                    outcome: SourceOutcome::rejected(
                        SourceOutcome::SUBSCRIPTION_REJECTED,
                        hostile(),
                    ),
                    health: ConvergenceHealth::Blocked,
                    at: 8,
                },
                SourceStatus {
                    profile: profile.clone(),
                    name: hostile(),
                    origin: SourceOrigin::ExternalFile,
                    outcome: SourceOutcome::Superseded { reason: hostile() },
                    health: ConvergenceHealth::Healthy,
                    at: 9,
                },
                SourceStatus {
                    profile,
                    name: hostile(),
                    origin: SourceOrigin::ExternalFile,
                    outcome: SourceOutcome::Rejected {
                        code: hostile(),
                        message: hostile(),
                    },
                    health: ConvergenceHealth::Blocked,
                    at: 10,
                },
            ],
            active: Some("op-2".into()),
            recent_operations: vec![OperationStatus {
                operation_id: "op-3".into(),
                domain: "Profiles".into(),
                outcome: "Rejected".into(),
                message: Some(hostile()),
            }],
        }
    }

    #[test]
    fn configuration_summary_keeps_structure_and_drops_text() {
        let summary = ConfigurationStatusSummary::from(&configuration());
        assert_clean(&summary);

        assert!(summary.maintenance_active);
        assert_eq!(summary.event_seq, 9);
        assert_eq!(summary.runtime.health, ConvergenceHealth::RetryScheduled);
        assert_eq!(summary.runtime.operation_id.as_deref(), Some("op-1"));
        assert!(summary.runtime.has_diagnostic);
        let effect = &summary.effects[0];
        assert_eq!(effect.code, Some(EffectFailureCode::SystemProxyApplyFailed));
        assert_eq!((effect.desired_revision, effect.applied_revision), (5, 4));
        assert!(effect.has_diagnostic);
        assert_eq!(summary.active.as_deref(), Some("op-2"));
        assert_eq!(summary.recent_operations[0].outcome, "Rejected");
        assert!(summary.recent_operations[0].has_diagnostic);
    }

    #[test]
    fn source_outcomes_keep_a_reason_tag_and_never_read_as_success() {
        let summary = ConfigurationStatusSummary::from(&configuration());
        let outcomes: Vec<_> = summary.sources.iter().map(|s| s.outcome.clone()).collect();
        assert_eq!(
            outcomes,
            [
                SourceOutcomeSummary::Failed,
                SourceOutcomeSummary::Rejected {
                    code: SourceRejection::SubscriptionRejected
                },
                SourceOutcomeSummary::Superseded,
                // An unclassified code is still a refusal, and its text is not repeated.
                SourceOutcomeSummary::Rejected {
                    code: SourceRejection::Other
                },
            ]
        );
        assert_eq!(summary.sources[0].health, ConvergenceHealth::Blocked);
        assert_eq!(summary.sources[3].health, ConvergenceHealth::Blocked);
    }

    #[test]
    fn an_absent_diagnostic_stays_absent() {
        let mut status = configuration();
        status.maintenance = None;
        status.runtime.message = None;
        status.effects[0].message = None;
        status.recent_operations[0].message = None;
        let summary = ConfigurationStatusSummary::from(&status);
        assert!(!summary.maintenance_active);
        assert!(!summary.runtime.has_diagnostic);
        assert!(!summary.effects[0].has_diagnostic);
        assert!(!summary.recent_operations[0].has_diagnostic);
    }
}

//! Read-only configuration status. No repair or retry occurs on inspection.
use super::{
    configuration::*,
    convergence::ConvergenceHealth,
    effects::{actor::EffectsSnapshot, status::EffectHealth},
    reconciler::{plan::Conclusion, status::ReconcilerStatus},
};
use crate::state::profiles::sources::SourcesSnapshot;

pub fn configuration_status(
    status: &ReconcilerStatus,
    effects: &EffectsSnapshot,
    sources: SourcesSnapshot,
    source_versions: SourceVersions,
) -> ConfigurationStatus {
    let (health, message) = match &status.conclusion {
        Conclusion::Converged | Conclusion::Inactive => (ConvergenceHealth::Healthy, None),
        Conclusion::Waiting { cause } => (
            ConvergenceHealth::WaitingDependency,
            Some(cause.to_string()),
        ),
        Conclusion::NotInEffect { cause } => (ConvergenceHealth::Blocked, Some(cause.to_string())),
        Conclusion::Unknown { cause } => (
            ConvergenceHealth::WaitingDependency,
            Some(cause.to_string()),
        ),
        Conclusion::Closed => (
            ConvergenceHealth::WaitingDependency,
            Some(chimera_contracts::runtime_error::RuntimeError::ShuttingDown.to_string()),
        ),
    };
    let runtime = RuntimeConvergence {
        health: if status.active.is_some() {
            ConvergenceHealth::Pending
        } else {
            health
        },
        operation_id: status
            .active
            .or_else(|| status.recent.back().map(|round| round.operation_id))
            .map(|id| id.to_string()),
        message,
    };
    ConfigurationStatus {
        maintenance: status.maintenance.clone(),
        event_seq: status.seq
            + effects.event_seq
            + sources.event_seq
            + source_versions.application
            + source_versions.clash
            + source_versions.session
            + source_versions.profiles,
        source_versions,
        runtime,
        active: status.active.map(|id| id.to_string()),
        effects: effects
            .effects
            .iter()
            .map(|progress| EffectConvergence {
                kind: progress.status.kind,
                health: progress.health,
                desired_revision: progress.status.desired_revision.get(),
                applied_revision: progress.status.applied_revision.get(),
                attempts: progress.attempts,
                automatic_remaining: progress.automatic_remaining,
                code: match &progress.status.health {
                    EffectHealth::Degraded { code, .. } | EffectHealth::Unsupported { code } => {
                        Some(EffectFailureCode::from_owner_code(code))
                    }
                    _ => None,
                },
                message: match &progress.status.health {
                    EffectHealth::Degraded { message, .. } => Some(message.clone()),
                    _ => None,
                },
            })
            .collect(),
        sources: sources.entries,
        recent_operations: status
            .recent
            .iter()
            .rev()
            .map(|round| OperationStatus {
                operation_id: round.operation_id.to_string(),
                domain: round.trigger.clone(),
                outcome: round.conclusion.clone(),
                message: round.cause.as_ref().map(ToString::to_string),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{
        configuration_observation::ConfigurationStatusSummary,
        effects::{
            actor::EffectProgress,
            status::{EffectRevision, EffectStatus},
        },
        reconciler::mutation::RoundRecord,
    };
    use chimera_config::profile::ProfileId;
    use std::sync::Arc;

    #[test]
    fn aggregates_the_actual_owner_versions_and_recent_operations() {
        let mut status = ReconcilerStatus::default();
        status.seq = 3;
        status.conclusion = Conclusion::Converged;
        status
            .recent
            .push_back(RoundRecord::new(1, "Retry", &Conclusion::Converged));
        status.recent.push_back(RoundRecord::new(
            2,
            "SourceCommitted",
            &Conclusion::Inactive,
        ));
        status.begin(3);
        status.set_maintenance(Some("private diagnostic".into()));
        let effects = EffectsSnapshot {
            event_seq: 5,
            effects: vec![EffectProgress {
                status: EffectStatus {
                    kind: EffectKind::SystemProxy,
                    desired_revision: EffectRevision::new(7),
                    applied_revision: EffectRevision::new(6),
                    health: EffectHealth::Degraded {
                        code: "system_proxy_apply_failed",
                        message: "private proxy value".into(),
                        retryable: true,
                    },
                },
                health: ConvergenceHealth::RetryScheduled,
                attempts: 1,
                automatic_remaining: 2,
            }],
            ..Default::default()
        };
        let sources = SourcesSnapshot {
            event_seq: 8,
            entries: vec![SourceStatus {
                profile: ProfileId("p1".into()),
                name: "private label".into(),
                origin: SourceOrigin::ScheduledRefresh,
                outcome: SourceOutcome::Failed {
                    message: "private URL".into(),
                },
                health: ConvergenceHealth::Blocked,
                at: 1,
            }],
        };
        let result = configuration_status(
            &status,
            &effects,
            sources,
            SourceVersions {
                application: 1,
                clash: 2,
                session: 3,
                profiles: 4,
            },
        );
        assert_eq!(result.event_seq, status.seq + 5 + 8 + 10);
        assert_eq!(result.runtime.health, ConvergenceHealth::Pending);
        assert_eq!(result.runtime.operation_id.as_deref(), Some("3"));
        assert_eq!(
            result
                .recent_operations
                .iter()
                .map(|r| r.operation_id.as_str())
                .collect::<Vec<_>>(),
            ["2", "1"]
        );
        assert_eq!(
            result.effects[0].code,
            Some(EffectFailureCode::SystemProxyApplyFailed)
        );
        assert_eq!(result.effects[0].automatic_remaining, 2);
        let summary = ConfigurationStatusSummary::from(&result);
        assert!(!serde_json::to_string(&summary).unwrap().contains("private"));
        assert!(summary.maintenance_active);
        assert!(summary.effects[0].has_diagnostic);
        assert_eq!(summary.sources[0].health, ConvergenceHealth::Blocked);
    }

    #[test]
    fn waiting_refusal_inactivity_and_shutdown_are_not_confused() {
        let cause = Arc::new(chimera_contracts::runtime_error::RuntimeError::ShuttingDown);
        for (conclusion, expected) in [
            (Conclusion::Converged, ConvergenceHealth::Healthy),
            (Conclusion::Inactive, ConvergenceHealth::Healthy),
            (
                Conclusion::Waiting {
                    cause: cause.clone(),
                },
                ConvergenceHealth::WaitingDependency,
            ),
            (
                Conclusion::Unknown {
                    cause: cause.clone(),
                },
                ConvergenceHealth::WaitingDependency,
            ),
            (
                Conclusion::NotInEffect { cause },
                ConvergenceHealth::Blocked,
            ),
            (Conclusion::Closed, ConvergenceHealth::WaitingDependency),
        ] {
            let mut status = ReconcilerStatus::default();
            status.conclusion = conclusion;
            let result = configuration_status(
                &status,
                &EffectsSnapshot::default(),
                SourcesSnapshot::default(),
                SourceVersions {
                    application: 0,
                    clash: 0,
                    session: 0,
                    profiles: 0,
                },
            );
            assert_eq!(result.runtime.health, expected);
            assert_eq!(
                result.runtime.message.is_some(),
                expected != ConvergenceHealth::Healthy
            );
        }
        assert_eq!(
            EffectFailureCode::from_owner_code("credential@example.invalid"),
            EffectFailureCode::Other
        );
    }
}

//! Conclusions sampled by response waiters without involving the workflow mailbox.
use std::{collections::VecDeque, sync::Arc};

use chimera_contracts::{mutation::ConfigDomain, runtime::Degradation};
use chimera_core::state::Version;
use chimera_core_manager::OperationId;

use super::{Conclusion, mutation::RoundRecord};

/// Source-engine versions read together with one round's committed inputs.
#[derive(Clone, Debug, Default)]
pub struct EngineVersions {
    pub application: Version,
    pub clash: Version,
    pub profiles: Version,
}
impl EngineVersions {
    pub(crate) fn of(&self, domain: ConfigDomain) -> Version {
        match domain {
            ConfigDomain::Application => self.application,
            ConfigDomain::Clash => self.clash,
            ConfigDomain::Profiles => self.profiles,
        }
    }
}

/// The current round and the last published conclusion, including bounded history.
#[derive(Clone, Debug)]
pub struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<OperationId>,
    pub seen: EngineVersions,
    pub conclusion: Conclusion,
    pub degradations: Vec<Degradation>,
    pub maintenance: Option<String>,
    pub recent: VecDeque<RoundRecord>,
}
impl Default for ReconcilerStatus {
    fn default() -> Self {
        Self {
            seq: 0,
            active: None,
            seen: EngineVersions::default(),
            conclusion: Conclusion::Unknown {
                cause: Arc::new(chimera_contracts::runtime_error::RuntimeError::new(
                    "runtime reconciliation has not started",
                )),
            },
            degradations: Vec::new(),
            maintenance: None,
            recent: VecDeque::new(),
        }
    }
}

pub(in crate::client) fn outcome(
    mut receipt: chimera_contracts::runtime::CommitReceipt,
    settled: Option<ReconcilerStatus>,
) -> chimera_contracts::runtime::MutationOutcome<()> {
    use chimera_contracts::runtime::{
        Degradation, DegradationPhase, DegradationReason, MutationOutcome, RuntimeCommitStatus,
    };
    let mut degradations = Vec::new();
    let runtime = match settled {
        None => RuntimeCommitStatus::Unchanged,
        Some(status) => match status.conclusion {
            Conclusion::Converged => {
                degradations = status.degradations;
                RuntimeCommitStatus::Applied
            }
            Conclusion::Inactive => RuntimeCommitStatus::SavedInactive,
            Conclusion::Waiting { cause } | Conclusion::NotInEffect { cause } => {
                degradations.push(Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    message: cause.to_string(),
                    retryable: true,
                    reason: DegradationReason::RuntimeDeferred { cause },
                });
                RuntimeCommitStatus::Deferred
            }
            conclusion => {
                let cause = match conclusion {
                    Conclusion::Unknown { cause } => cause,
                    Conclusion::Closed => {
                        Arc::new(chimera_contracts::runtime_error::RuntimeError::ShuttingDown)
                    }
                    _ => unreachable!("only unknown and closed conclusions remain"),
                };
                degradations.push(Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    message: cause.to_string(),
                    retryable: false,
                    reason: DegradationReason::RuntimeDeferred { cause },
                });
                RuntimeCommitStatus::Deferred
            }
        },
    };
    receipt.runtime = runtime;
    MutationOutcome::from_parts((), degradations).with_commit(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_contracts::runtime::{CommitReceipt, RuntimeCommitStatus};
    use chimera_contracts::runtime_error::RuntimeError;

    fn receipt() -> CommitReceipt {
        CommitReceipt {
            operation_id: Some("checked-source-commit".into()),
            domain: "application".into(),
            source_version: 8,
            runtime: RuntimeCommitStatus::Unchanged,
        }
    }

    #[test]
    fn upstream_engine_versions_are_separate_for_each_source() {
        let versions = EngineVersions {
            application: Version::new(7),
            clash: Version::new(3),
            profiles: Version::new(12),
        };
        assert_eq!(versions.of(ConfigDomain::Application), Version::new(7));
        assert_eq!(versions.of(ConfigDomain::Clash), Version::new(3));
        assert_eq!(versions.of(ConfigDomain::Profiles), Version::new(12));
    }

    #[test]
    fn runtime_outcome_does_not_invent_an_apply_for_unsettled_commit() {
        let none = outcome(receipt(), None);
        let wire = serde_json::to_value(none).unwrap();
        assert_eq!(wire["commits"][0]["runtime"], "unchanged");
        let inactive = outcome(
            receipt(),
            Some(ReconcilerStatus {
                conclusion: Conclusion::Inactive,
                ..Default::default()
            }),
        );
        let wire = serde_json::to_value(inactive).unwrap();
        assert_eq!(wire["commits"][0]["runtime"], "saved_inactive");
        let applied = outcome(
            receipt(),
            Some(ReconcilerStatus {
                conclusion: Conclusion::Converged,
                ..Default::default()
            }),
        );
        let wire = serde_json::to_value(applied).unwrap();
        assert_eq!(wire["commits"][0]["runtime"], "applied");
    }

    #[test]
    fn waiting_and_unknown_have_distinct_retryability_and_typed_cause() {
        for (conclusion, retryable, expected_kind) in [
            (
                Conclusion::Waiting {
                    cause: Arc::new(RuntimeError::new("service not ready")),
                },
                true,
                "configuration_not_in_effect",
            ),
            (
                Conclusion::Unknown {
                    cause: Arc::new(RuntimeError::ShuttingDown),
                },
                false,
                "shutting_down",
            ),
            (Conclusion::Closed, false, "shutting_down"),
        ] {
            let result = outcome(
                receipt(),
                Some(ReconcilerStatus {
                    conclusion,
                    ..Default::default()
                }),
            );
            let wire = serde_json::to_value(result).unwrap();
            assert_eq!(wire["commits"][0]["runtime"], "deferred");
            assert_eq!(wire["status"], "committed_degraded");
            assert_eq!(
                wire["degradations"][0]["reason"]["code"],
                "runtime_deferred"
            );
            assert_eq!(
                wire["degradations"][0]["reason"]["cause"]["kind"],
                expected_kind
            );
            assert_eq!(wire["degradations"][0]["retryable"], retryable);
        }
    }
}

//! Conclusions sampled by response waiters without involving the workflow mailbox.
//!
//! Chimera still executes through one CoreLifecycle mailbox; this projection
//! never applies a Core command, owns a source writer or infers success from a
//! notification. Its "seen" versions are sampled at the start of a runtime
//! round, not copied from an unprocessed SourceCommitted message.
use std::{collections::VecDeque, sync::Arc};

use crate::client::reconciler::plan::{Conclusion, RunIntent, RuntimeError};

use super::{attempt::LegacyApplyVerdict, mutation::RoundRecord};

#[cfg(any(test, feature = "test-support"))]
use super::attempt::observed_apply_verdict;
use super::committed::{SourceDomain, SourceVersions};
use chimera_contracts::runtime::{
    Degradation, DegradationPhase, DegradationReason, MutationOutcome, RuntimeCommitStatus,
};

/// Source-engine versions sampled by the real mailbox, with absent values
/// distinguished from version zero.
pub type EngineVersions = SourceVersions;

const HISTORY_LEN: usize = 32;

#[derive(Debug, Clone)]
pub struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<u64>,
    pub seen: EngineVersions,
    pub conclusion: Conclusion,
    pub degradations: Vec<Degradation>,
    pub maintenance: Option<String>,
    /// Ref pending_product's status projection, read from the one accepted
    /// LocalRuntimeHost receipt at Actor completion (never a second writer).
    pending_product_target: Option<String>,
    pub recent: VecDeque<RoundRecord>,
}

impl Default for ReconcilerStatus {
    fn default() -> Self {
        Self {
            seq: 0,
            active: None,
            seen: SourceVersions::default(),
            conclusion: Conclusion::Unknown {
                cause: Arc::new(RuntimeError::new("runtime reconciliation has not started")),
            },
            degradations: Vec::new(),
            maintenance: None,
            pending_product_target: None,
            recent: VecDeque::new(),
        }
    }
}

impl ReconcilerStatus {
    /// A response waiter sees only an actually completed source version.
    /// An unobserved version (including zero) cannot settle a commit.
    pub fn settled_for(&self, domain: SourceDomain, version: u64) -> bool {
        matches!(self.conclusion, Conclusion::Closed)
            || (self.active.is_none()
                && self
                    .seen
                    .version(domain)
                    .is_some_and(|seen| seen >= version))
    }

    /// Maintenance is independent of the last runtime conclusion. A Core may
    /// be Converged while an owned Service still needs safe retirement; neither
    /// a successful source commit nor a recorded retry can clear that debt.
    pub fn set_maintenance(&mut self, reason: Option<String>) {
        if self.maintenance != reason {
            self.seq = self.seq.saturating_add(1);
            self.maintenance = reason;
        }
    }

    pub fn close(&mut self) {
        self.seq = self.seq.saturating_add(1);
        self.active = None;
        self.conclusion = Conclusion::Closed;
    }

    pub fn begin(&mut self, operation_id: u64) {
        self.seq = self.seq.saturating_add(1);
        self.active = Some(operation_id);
        // REF: degradations describe the current Core round, not errors
        // inherited from an unrelated successfully completed request.
        self.degradations.clear();
    }

    pub fn record_system_degradation(
        &mut self,
        degradation: chimera_contracts::runtime::Degradation,
    ) {
        self.seq = self.seq.saturating_add(1);
        self.degradations.push(degradation);
    }

    pub fn product_publication_maintenance(&self) -> Option<String> {
        self.pending_product_target
            .as_ref()
            .map(|target| format!("RuntimeProduct publication pending for target {target}"))
    }

    /// Reconcile status owns only the projection of publication debt. The
    /// actual receipt and derived-file writer live in LocalRuntimeHost; the
    /// Actor samples that owner once after the Core operation has settled.
    pub fn observe_product_publication(&mut self, target: Option<&str>) {
        if self.pending_product_target.as_deref() != target {
            self.seq = self.seq.saturating_add(1);
            self.pending_product_target = target.map(str::to_owned);
        }
        if let Some(target) = target {
            self.record_product_publication_pending(target);
        }
    }

    /// Only the active Core owner's confirmed-but-unpublished product can
    /// produce this REF RuntimeBuild degradation. This records publication
    /// debt without inventing an upstream RuntimeError cause that the old
    /// CoreLifecycle port cannot yet return. Retrying it must not submit Core.
    pub fn record_product_publication_pending(&mut self, target: &str) {
        if self.degradations.iter().any(|degradation| {
            matches!(&degradation.reason, DegradationReason::RuntimeProductPublicationPending { target: recorded } if recorded == target)
        }) {
            return;
        }
        self.seq = self.seq.saturating_add(1);
        self.degradations.push(Degradation {
            phase: DegradationPhase::RuntimeBuild,
            reason: DegradationReason::RuntimeProductPublicationPending {
                target: target.to_owned(),
            },
            message: format!("RuntimeProduct publication pending for target {target}"),
            retryable: true,
        });
    }

    /// A source-only commit is already durable, but no Core application was
    /// requested. Keep the previous runtime conclusion unchanged.
    pub fn observe_source_only(&mut self, domain: SourceDomain, version: u64) {
        self.seen.observe(domain, version);
        self.seq = self.seq.saturating_add(1);
    }

    /// Always record the versions visible before this round started. New
    /// commits arriving while Core applies are left for a subsequent round.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        operation_id: u64,
        trigger: &str,
        observed_sources: &SourceVersions,
        intent: RunIntent,
        result: &anyhow::Result<()>,
        reconciled: bool,
        verified_running: bool,
        verified_stopped: bool,
        outcome_uncertain: bool,
        source_advanced: bool,
    ) {
        self.finish_observed(
            operation_id,
            trigger,
            observed_sources,
            intent,
            result,
            reconciled,
            verified_running,
            verified_stopped,
            outcome_uncertain,
            source_advanced,
            None,
        )
    }

    /// The one Actor's final source and Core-running evidence remains the
    /// convergence authority. An observed Prepared terminal supplies the
    /// upstream typed cause, not a shortcut to Converged.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments)]
    pub fn finish_observed(
        &mut self,
        operation_id: u64,
        trigger: &str,
        observed_sources: &SourceVersions,
        intent: RunIntent,
        result: &anyhow::Result<()>,
        reconciled: bool,
        verified_running: bool,
        verified_stopped: bool,
        outcome_uncertain: bool,
        source_advanced: bool,
        prepared: Option<&super::mutation::ApplyOutcome>,
    ) {
        let verdict = observed_apply_verdict(result, outcome_uncertain, source_advanced, prepared);
        self.finish_with_verdict(
            operation_id,
            trigger,
            observed_sources,
            intent,
            reconciled,
            verified_running,
            verified_stopped,
            &verdict,
        );
    }

    /// Production Completed receives the *single* typed terminal computed
    /// by the sole Actor, shared with RuntimeProgress::settle_with_verdict.
    /// Source and Core proof still gate Converged independently of Applied.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_with_verdict(
        &mut self,
        operation_id: u64,
        trigger: &str,
        observed_sources: &SourceVersions,
        intent: RunIntent,
        reconciled: bool,
        verified_running: bool,
        verified_stopped: bool,
        verdict: &LegacyApplyVerdict,
    ) {
        self.seq = self.seq.saturating_add(1);
        self.active = None;
        self.seen.merge(observed_sources);
        self.conclusion = if trigger == "Shutdown" {
            Conclusion::Closed
        } else if trigger == "ServiceRelease" {
            // Daemon cleanup is maintenance after an established Local Core,
            // not a new runtime application. Its failure must not revoke a
            // separately proven Converged runtime conclusion.
            self.conclusion.clone()
        } else if let LegacyApplyVerdict::Unknown(cause) = &verdict {
            Conclusion::Unknown {
                cause: cause.clone(),
            }
        } else if let LegacyApplyVerdict::Waiting(cause) = &verdict {
            Conclusion::Waiting {
                cause: cause.clone(),
            }
        } else if let LegacyApplyVerdict::Refused(cause) = &verdict {
            Conclusion::NotInEffect {
                cause: cause.clone(),
            }
        } else if intent == RunIntent::Stopped && trigger == "Stop" && verified_stopped {
            Conclusion::Inactive
        } else if intent == RunIntent::Stopped && trigger == "Stop" {
            Conclusion::Waiting {
                cause: Arc::new(RuntimeError::new(
                    "stop returned successfully but a Stopped Core state has not been observed",
                )),
            }
        } else if intent == RunIntent::Stopped {
            self.conclusion.clone()
        } else if reconciled && verified_running {
            Conclusion::Converged
        } else if reconciled {
            Conclusion::Waiting {
                cause: Arc::new(RuntimeError::new(
                    "core apply returned successfully, but the current target, host, and active revision have not all been verified",
                )),
            }
        } else {
            self.conclusion.clone()
        };
        if self.recent.len() == HISTORY_LEN {
            self.recent.pop_front();
        }
        self.recent
            .push_back(RoundRecord::new(operation_id, trigger, &self.conclusion));
    }
}

pub fn outcome(
    mut receipt: chimera_contracts::runtime::CommitReceipt,
    settled: Option<ReconcilerStatus>,
) -> MutationOutcome<()> {
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

    #[test]
    fn actor_completion_keeps_typed_prepared_terminal_without_bypassing_source_fence() {
        use super::super::mutation::ApplyOutcome;
        let mut status = ReconcilerStatus::default();
        let cause = Arc::new(RuntimeError::CoreRolledBack {
            reason: Some("Core restored original revision".into()),
        });
        let outcome = ApplyOutcome::Refused {
            cause: cause.clone(),
        };
        let generic = Err(anyhow::anyhow!("legacy Core error lost rollback details"));
        status.begin(11);
        status.finish_observed(
            11,
            "SourceCommitted",
            &SourceVersions::default(),
            RunIntent::Running,
            &generic,
            true,
            false,
            false,
            false,
            false,
            Some(&outcome),
        );
        assert!(matches!(
            &status.conclusion,
            Conclusion::NotInEffect {cause: actual} if Arc::ptr_eq(actual, &cause)
        ));
        assert!(matches!(
            status.recent.back().and_then(|record| record.cause.as_ref()),
            Some(actual) if Arc::ptr_eq(actual, &cause)
        ));
        // If a newer source arrived in the mailbox, its independent source
        // fence wins over the older terminal's original refusal.
        status.begin(12);
        let superseded = Err(anyhow::Error::new(
            crate::client::reconciler::attempt::CommittedTargetAdvanced,
        ));
        status.finish_observed(
            12,
            "SourceCommitted",
            &SourceVersions::default(),
            RunIntent::Running,
            &superseded,
            true,
            false,
            false,
            false,
            true,
            Some(&outcome),
        );
        assert!(matches!(status.conclusion, Conclusion::Waiting { .. }));

        let unknown = Arc::new(RuntimeError::SubmissionUnobserved {
            failure: chimera_contracts::runtime_error::CoreFailure {
                kind: None,
                message: "Core terminal lost".into(),
                retryable: false,
                operation_id: Some("original-op-id".into()),
            },
        });
        status.begin(13);
        status.finish_observed(
            13,
            "Retry",
            &SourceVersions::default(),
            RunIntent::Running,
            &generic,
            true,
            false,
            false,
            true,
            false,
            Some(&ApplyOutcome::Unknown(unknown.clone())),
        );
        assert!(matches!(
            &status.conclusion,
            Conclusion::Unknown {cause: actual} if Arc::ptr_eq(actual, &unknown)
        ));
    }

    #[test]
    fn system_effect_degradation_is_structured_and_scoped_to_completed_round() {
        use chimera_contracts::runtime::{
            Degradation as SharedDegradation, DegradationPhase, DegradationReason, InterruptFailure,
        };
        let mut status = ReconcilerStatus::default();
        status.begin(1);
        status.finish(
            1,
            "SourceCommitted",
            &SourceVersions::default(),
            RunIntent::Running,
            &Ok(()),
            true,
            true,
            false,
            false,
            false,
        );
        assert!(matches!(status.conclusion, Conclusion::Converged));
        status.record_system_degradation(SharedDegradation {
            phase: DegradationPhase::SystemEffect,
            reason: DegradationReason::ProfileInterruptionFailed {
                cause: InterruptFailure::Stale,
            },
            message: "previous Core API is stale".into(),
            retryable: false,
        });
        assert!(matches!(status.conclusion, Conclusion::Converged));
        assert_eq!(status.degradations.len(), 1);
        assert_eq!(
            status.degradations[0].reason.code(),
            "profile_interruption_failed"
        );
        assert_eq!(status.degradations[0].phase, DegradationPhase::SystemEffect);
        assert!(!status.degradations[0].retryable);
        status.begin(2);
        assert!(
            status.degradations.is_empty(),
            "a new round must not inherit the previous effect failure"
        );
    }

    #[test]
    fn maintenance_changes_do_not_assert_runtime_convergence_or_advance_sources() {
        let mut status = ReconcilerStatus::default();
        status.conclusion = Conclusion::Converged;
        status.observe_source_only(SourceDomain::Profiles, 4);
        let initial = status.seq;
        status.set_maintenance(Some("Service owner release is pending".into()));
        assert_eq!(status.seq, initial + 1);
        assert!(matches!(status.conclusion, Conclusion::Converged));
        assert_eq!(status.seen.version(SourceDomain::Profiles), Some(4));
        status.set_maintenance(Some("Service owner release is pending".into()));
        assert_eq!(
            status.seq,
            initial + 1,
            "duplicate maintenance does not republish"
        );
        status.set_maintenance(None);
        assert!(status.maintenance.is_none());
        assert_eq!(status.seq, initial + 2);
    }

    #[test]
    fn pending_product_status_is_one_projection_not_a_runtime_reapply() {
        let mut status = ReconcilerStatus::default();
        status.conclusion = Conclusion::Converged;
        status.observe_product_publication(Some("committed-profile"));
        assert_eq!(
            status.product_publication_maintenance().as_deref(),
            Some("RuntimeProduct publication pending for target committed-profile"),
        );
        assert_eq!(status.degradations.len(), 1);
        assert_eq!(
            status.degradations[0].reason.code(),
            "runtime_product_publish_failed"
        );
        assert!(matches!(status.conclusion, Conclusion::Converged));

        let seq = status.seq;
        status.observe_product_publication(Some("committed-profile"));
        assert_eq!(
            status.seq, seq,
            "an unchanged debt must not republish status"
        );
        status.observe_product_publication(None);
        assert!(status.product_publication_maintenance().is_none());
        assert!(matches!(status.conclusion, Conclusion::Converged));
    }

    #[test]
    fn source_only_version_keeps_prior_conclusion_and_round_history_bounded() {
        let mut status = ReconcilerStatus::default();
        status.conclusion = Conclusion::Converged;
        status.observe_source_only(SourceDomain::Profiles, 8);
        assert_eq!(status.seen.version(SourceDomain::Profiles), Some(8));
        assert!(matches!(status.conclusion, Conclusion::Converged));

        for id in 0..45 {
            status.begin(id);
            status.finish(
                id,
                "Retry",
                &SourceVersions::default(),
                RunIntent::Running,
                &Ok(()),
                true,
                true,
                false,
                false,
                false,
            );
        }
        assert_eq!(status.recent.len(), HISTORY_LEN);
        assert_eq!(status.recent.front().unwrap().operation_id, 13);
        assert!(status.active.is_none());
    }

    #[test]
    fn legacy_core_verdicts_project_ref_outcome_without_inventing_applied_receipts() {
        use chimera_core_manager::{CoreError, CoreErrorKind};

        let mut status = ReconcilerStatus::default();
        let sampled = SourceVersions::default();
        status.begin(30);
        status.finish(
            30,
            "Startup",
            &sampled,
            RunIntent::Running,
            &Err(CoreError::new(CoreErrorKind::ConfigCheckFailed, "bad source", false).into()),
            true,
            false,
            false,
            false,
            false,
        );
        assert!(matches!(status.conclusion, Conclusion::NotInEffect { .. }));
        status.begin(31);
        status.finish(
            31,
            "ServiceReadiness",
            &sampled,
            RunIntent::Running,
            &Err(CoreError::new(CoreErrorKind::BackendUnavailable, "host offline", false).into()),
            true,
            false,
            false,
            false,
            false,
        );
        assert!(matches!(status.conclusion, Conclusion::Waiting { .. }));
        status.begin(32);
        status.finish(
            32,
            "SourceCommitted",
            &sampled,
            RunIntent::Running,
            &Err(CoreError::new(CoreErrorKind::ApplyFailed, "unobserved", false).into()),
            true,
            false,
            false,
            true,
            false,
        );
        assert!(matches!(status.conclusion, Conclusion::Unknown { .. }));
        status.begin(33);
        status.finish(
            33,
            "Startup",
            &sampled,
            RunIntent::Running,
            &Ok(()),
            true,
            false,
            false,
            false,
            false,
        );
        assert!(
            matches!(status.conclusion, Conclusion::Waiting { .. }),
            "a legacy Ok without a Running receipt must not claim Converged"
        );
        status.begin(34);
        status.finish(
            34,
            "Startup",
            &sampled,
            RunIntent::Running,
            &Ok(()),
            true,
            true,
            false,
            false,
            false,
        );
        assert!(matches!(status.conclusion, Conclusion::Converged));
    }

    #[test]
    fn transient_host_conflict_is_waiting_not_permanent_refusal() {
        use chimera_core_manager::{CoreError, CoreErrorKind};

        let mut status = ReconcilerStatus::default();
        let observed = SourceVersions::default();
        for (id, kind) in [
            CoreErrorKind::BackendUnavailable,
            CoreErrorKind::QueueFull,
            CoreErrorKind::OperationConflict,
            CoreErrorKind::RevisionConflict,
        ]
        .into_iter()
        .enumerate()
        {
            status.begin(id as u64);
            let outcome =
                Err(CoreError::new(kind, "the runtime host is temporarily busy", false).into());
            status.finish(
                id as u64,
                "SourceCommitted",
                &observed,
                RunIntent::Running,
                &outcome,
                true,
                false,
                false,
                false,
                false,
            );
            assert!(
                matches!(status.conclusion, Conclusion::Waiting { .. }),
                "{kind:?} should wait for the dependency, not block the target"
            );
        }
    }

    #[test]
    fn stale_round_never_claims_newer_source_and_explicit_stop_is_inactive() {
        let mut status = ReconcilerStatus::default();
        let mut sampled = SourceVersions::default();
        sampled.observe(SourceDomain::Profiles, 1);
        status.begin(9);
        status.finish(
            9,
            "SourceCommitted",
            &sampled,
            RunIntent::Running,
            &Err(anyhow::anyhow!("source advanced during apply")),
            true,
            false,
            false,
            false,
            true,
        );
        assert_eq!(status.seen.version(SourceDomain::Profiles), Some(1));
        assert!(matches!(status.conclusion, Conclusion::Waiting { .. }));
        status.begin(10);
        status.finish(
            10,
            "Stop",
            &sampled,
            RunIntent::Stopped,
            &Ok(()),
            false,
            false,
            true,
            false,
            false,
        );
        assert!(matches!(status.conclusion, Conclusion::Inactive));
    }
}

#[cfg(test)]
mod ref_outcome_tests {
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
        let mut versions = EngineVersions::default();
        versions.observe(SourceDomain::Application, 7);
        versions.observe(SourceDomain::Clash, 3);
        versions.observe(SourceDomain::Profiles, 12);
        assert_eq!(versions.version(SourceDomain::Application), Some(7));
        assert_eq!(versions.version(SourceDomain::Clash), Some(3));
        assert_eq!(versions.version(SourceDomain::Profiles), Some(12));
        let mut empty = EngineVersions::default();
        assert_eq!(empty.version(SourceDomain::Application), None);
        empty.observe(SourceDomain::Application, 0);
        assert_eq!(empty.version(SourceDomain::Application), Some(0));
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
    fn converged_round_preserves_typed_interruption_and_pending_publication() {
        let mut status = ReconcilerStatus {
            conclusion: Conclusion::Converged,
            ..Default::default()
        };
        status.record_system_degradation(Degradation {
            phase: DegradationPhase::SystemEffect,
            reason: DegradationReason::ModeInterruptionFailed {
                cause: chimera_contracts::runtime::InterruptFailure::Timeout,
            },
            message: "connection interruption timed out".into(),
            retryable: true,
        });
        status.observe_product_publication(Some("accepted-target"));
        let wire = serde_json::to_value(outcome(receipt(), Some(status))).unwrap();
        assert_eq!(wire["status"], "committed_degraded");
        assert_eq!(wire["commits"][0]["runtime"], "applied");
        assert_eq!(
            wire["degradations"][0]["reason"],
            serde_json::json!({"code":"mode_interruption_failed","cause":"timeout"})
        );
        assert_eq!(
            wire["degradations"][1]["reason"],
            serde_json::json!({"code":"runtime_product_publication_pending","target":"accepted-target"})
        );
    }

    #[test]
    fn waiting_and_unknown_have_distinct_retryability_and_typed_cause() {
        for (conclusion, retryable) in [
            (
                Conclusion::Waiting {
                    cause: Arc::new(RuntimeError::new("service not ready")),
                },
                true,
            ),
            (
                Conclusion::Unknown {
                    cause: Arc::new(RuntimeError::ShuttingDown),
                },
                false,
            ),
            (Conclusion::Closed, false),
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
            assert_eq!(wire["degradations"][0]["retryable"], retryable);
            assert!(wire["degradations"][0]["reason"]["cause"]["kind"].is_string());
        }
    }
}

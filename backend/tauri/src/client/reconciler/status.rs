//! Ref-shaped, read-only RuntimeReconciler projections.
//!
//! Chimera still executes through one CoreLifecycle mailbox; this projection
//! never applies a Core command, owns a source writer or infers success from a
//! notification. Its "seen" versions are sampled at the start of a runtime
//! round, not copied from an unprocessed SourceCommitted message.
use std::{collections::VecDeque, sync::Arc};

use chimera_core::client::reconciler::plan::{Conclusion, RunIntent, RuntimeError};

use super::committed::{SourceDomain, SourceVersions};
use crate::client::runtime::Degradation;

const HISTORY_LEN: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct RoundRecord {
    pub operation_id: u64,
    pub trigger: String,
    pub conclusion: String,
    pub cause: Option<Arc<RuntimeError>>,
}

impl RoundRecord {
    fn new(operation_id: u64, trigger: &str, conclusion: &Conclusion) -> Self {
        let (label, cause) = match conclusion {
            Conclusion::Converged => ("Converged", None),
            Conclusion::Inactive => ("Inactive", None),
            Conclusion::NotInEffect { cause } => ("NotInEffect", Some(cause.clone())),
            Conclusion::Waiting { cause } => ("Waiting", Some(cause.clone())),
            Conclusion::Unknown { cause } => ("Unknown", Some(cause.clone())),
            Conclusion::Closed => ("Closed", None),
        };
        Self {
            operation_id,
            trigger: trigger.to_owned(),
            conclusion: label.to_owned(),
            cause,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<u64>,
    pub seen: SourceVersions,
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
            seen: SourceVersions::default(),
            conclusion: Conclusion::Unknown {
                cause: Arc::new(RuntimeError::new("runtime reconciliation has not started")),
            },
            degradations: Vec::new(),
            maintenance: None,
            recent: VecDeque::new(),
        }
    }
}

impl ReconcilerStatus {
    /// Maintenance is independent of the last runtime conclusion. A Core may
    /// be Converged while an owned Service still needs safe retirement; neither
    /// a successful source commit nor a recorded retry can clear that debt.
    pub(crate) fn set_maintenance(&mut self, reason: Option<String>) {
        if self.maintenance != reason {
            self.seq = self.seq.saturating_add(1);
            self.maintenance = reason;
        }
    }

    pub(crate) fn close(&mut self) {
        self.seq = self.seq.saturating_add(1);
        self.active = None;
        self.conclusion = Conclusion::Closed;
    }

    pub(crate) fn begin(&mut self, operation_id: u64) {
        self.seq = self.seq.saturating_add(1);
        self.active = Some(operation_id);
    }

    /// A source-only commit is already durable, but no Core application was
    /// requested. Keep the previous runtime conclusion unchanged.
    pub(crate) fn observe_source_only(&mut self, domain: SourceDomain, version: u64) {
        self.seen.observe(domain, version);
        self.seq = self.seq.saturating_add(1);
    }

    /// Always record the versions visible before this round started. New
    /// commits arriving while Core applies are left for a subsequent round.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish(
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
        self.seq = self.seq.saturating_add(1);
        self.active = None;
        self.seen.merge(observed_sources);
        self.conclusion = if trigger == "Shutdown" {
            Conclusion::Closed
        } else if outcome_uncertain {
            Conclusion::Unknown {
                cause: Arc::new(RuntimeError::new(
                    result
                        .as_ref()
                        .err()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "core lifecycle outcome is uncertain".into()),
                )),
            }
        } else if let Err(error) = result {
            let cause = Arc::new(RuntimeError::new(error.to_string()));
            if source_advanced || super::attempt::waiting_dependency(error) {
                Conclusion::Waiting { cause }
            } else {
                Conclusion::NotInEffect { cause }
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

#[cfg(test)]
mod tests {
    use super::*;

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

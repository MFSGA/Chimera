//! Ref RuntimeReconciler's active target attempt and applied selection.
//!
//! The one CoreLifecycle executor shares this state across its cloned
//! workflows. Neither a source CAS nor an unobserved Core reply may promote
//! selection; the sole Actor still owns final Conclusion and source fences.
//! No Core commands, writer mailbox, or second runtime session live here.
use chimera_core::{
    actor_v2::endpoint::ExecutionHost,
    client::reconciler::plan::{Attempt, Trigger},
};

#[cfg(test)]
use super::attempt::observed_apply_verdict;
#[cfg(test)]
use super::mutation::ApplyOutcome;
use super::{
    attempt::{AttemptPolicy, LegacyApplyVerdict, PreviousRefusal},
    authority::RuntimeSourceAuthority,
    committed::Selection,
};
use crate::client::core_lifecycle::ports::CoreLifecyclePort;

/// A worker-side declaration that an actual target/host attempt was admitted.
/// Never classify its terminal until the one Actor Completed message observes
/// the final source generation and running Core receipt.
#[derive(Debug, Clone)]
struct PendingAttempt {
    target: String,
    host: ExecutionHost,
}

#[derive(Default)]
pub(crate) struct RuntimeProgress {
    attempt: AttemptPolicy,
    applied_selection: Option<Selection>,
    pending: Option<PendingAttempt>,
}

impl RuntimeProgress {
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.applied_selection.clone()
    }

    /// A saved source or an uncertain Core is not a verified selection.
    pub(crate) fn set_selection(&mut self, selection: Option<Selection>) {
        self.applied_selection = selection;
    }

    pub(crate) fn invalidate_selection(&mut self) {
        self.applied_selection = None;
    }

    /// The selected source/host attempt belongs to this same runtime owner;
    /// reads and updates are short and never span an asynchronous Core probe.
    pub(crate) fn attempt(&self) -> Option<Attempt> {
        self.attempt.snapshot()
    }

    pub(crate) fn allow(
        &self,
        target: &str,
        host: ExecutionHost,
        trigger: Trigger,
    ) -> Result<(), PreviousRefusal> {
        self.attempt.allow(target, host, trigger)
    }

    /// A different lifecycle command must never inherit a worker's proposal.
    /// Called once at the beginning of CoreLifecycleWorkflow::execute.
    pub(crate) fn clear_pending(&mut self) {
        self.pending = None;
    }

    /// Called only after the ref planner/admission permits a Core attempt.
    /// Service-to-Local fallback replaces the staged host, not the last
    /// *settled* refusal, because only the final host may establish Running.
    pub(crate) fn stage(&mut self, target: String, host: ExecutionHost) {
        self.pending = Some(PendingAttempt { target, host });
    }

    /// Consume exactly once, at Actor Completed, from the identical final
    /// source + Core terminal that produces the published Conclusion.
    /// Even an Ok(()) submit is only Waiting until the selected live receipt
    /// is verified. Source advancement and unobserved Core operations must
    /// not poison the current target as Refused/Applied.
    #[cfg(test)]
    pub(crate) fn settle(
        &mut self,
        result: &anyhow::Result<()>,
        uncertain: bool,
        source_advanced: bool,
        verified_running: bool,
        prepared: Option<&ApplyOutcome>,
    ) {
        let verdict = observed_apply_verdict(result, uncertain, source_advanced, prepared);
        self.settle_with_verdict(&verdict, verified_running);
    }

    /// Only the sole Actor can settle a staged attempt; its observed verdict
    /// is the exact same source/Core terminal used to publish Conclusion.
    /// No provisional worker response is classified again on this path.
    pub(crate) fn settle_with_verdict(
        &mut self,
        verdict: &LegacyApplyVerdict,
        verified_running: bool,
    ) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let mut verdict = verdict.clone();
        if matches!(verdict, LegacyApplyVerdict::Applied) && !verified_running {
            verdict = LegacyApplyVerdict::Waiting(std::sync::Arc::new(
                chimera_core::client::reconciler::plan::RuntimeError::new(
                    "Core apply returned success but the current Running receipt was not proven",
                ),
            ));
        }
        self.attempt
            .record_observed(pending.target, pending.host, verdict);
    }
}

/// Read-only ref applied_selection recovery: only two bounded committed
/// source reads and an unchanged active Core receipt can recover the last
/// selected Profile/Mode after a legacy reconcile. A failed probe clears
/// selection; this function never starts or submits a Core.
pub(crate) async fn proven_selection_from_receipt(
    core: &dyn CoreLifecyclePort,
    authority: Option<&dyn RuntimeSourceAuthority>,
) -> Option<Selection> {
    let authority = authority?;
    let first = match core.active_runtime_receipt().await {
        Ok(Some(receipt)) if receipt.target.is_some() => receipt,
        Ok(_) => return None,
        Err(error) => {
            tracing::debug!(%error, "cannot prove active Core for selection recovery");
            return None;
        }
    };
    let target = first.target.as_deref().expect("checked above");
    let selection = match authority.selection_for_applied_target(target).await {
        Ok(selection) => selection,
        Err(error) => {
            tracing::debug!(%error, "cannot prove committed sources for selection recovery");
            None
        }
    };
    // A source commit racing the first async read must not seed a selection
    // which was never proved to match the running product.
    let latest_selection = authority
        .selection_for_applied_target(target)
        .await
        .ok()
        .flatten();
    // Recheck the exact accepted receipt, including its process/connection identity.
    let still_active = core
        .active_runtime_receipt()
        .await
        .ok()
        .flatten()
        .is_some_and(|latest| std::sync::Arc::ptr_eq(&first, &latest));
    if still_active && selection == latest_selection {
        selection
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
    };
    use std::sync::Arc;

    #[test]
    fn one_observed_refusal_is_shared_by_attempt_and_conclusion() {
        use chimera_core::client::reconciler::plan::{AttemptOutcome, Conclusion, RunIntent};
        let cause = Arc::new(
            chimera_core::client::reconciler::plan::RuntimeError::CoreRolledBack {
                reason: Some("Core restored the original revision".into()),
            },
        );
        let prepared = ApplyOutcome::Refused {
            cause: cause.clone(),
        };
        let result = Err(anyhow::anyhow!("legacy port omitted the rollback cause"));
        // Completed owns this one classification, not both projections.
        let verdict = observed_apply_verdict(&result, false, false, Some(&prepared));

        let mut progress = RuntimeProgress::default();
        progress.stage("committed-profile".into(), ExecutionHost::Local);
        progress.settle_with_verdict(&verdict, false);

        let mut status = super::super::legacy_status::ReconcilerStatus::default();
        status.begin(9);
        status.finish_with_verdict(
            9,
            "SourceCommitted",
            &super::super::committed::SourceVersions::default(),
            RunIntent::Running,
            true,
            false,
            false,
            &verdict,
        );
        assert!(matches!(
            progress.attempt().unwrap().outcome,
            AttemptOutcome::Refused(actual) if Arc::ptr_eq(&actual, &cause)
        ));
        assert!(matches!(
            status.conclusion,
            Conclusion::NotInEffect { cause: actual } if Arc::ptr_eq(&actual, &cause)
        ));
    }

    #[test]
    fn one_observed_unknown_is_shared_without_losing_operation_identity() {
        use chimera_core::client::reconciler::plan::{AttemptOutcome, Conclusion, RunIntent};
        let cause = Arc::new(
            chimera_core::client::reconciler::plan::RuntimeError::SubmissionUnobserved {
                failure: chimera_contracts::runtime_error::CoreFailure {
                    kind: Some(chimera_core_manager::CoreErrorKind::BackendUnavailable),
                    message: "Core operation terminal lost".into(),
                    retryable: true,
                    operation_id: Some("original-operation-42".into()),
                },
            },
        );
        let prepared = ApplyOutcome::Unknown(cause.clone());
        let result = Err(anyhow::anyhow!("compatibility Core error"));
        let verdict = observed_apply_verdict(&result, true, false, Some(&prepared));

        let mut progress = RuntimeProgress::default();
        progress.stage("committed-profile".into(), ExecutionHost::Service);
        progress.settle_with_verdict(&verdict, false);

        let mut status = super::super::legacy_status::ReconcilerStatus::default();
        status.begin(10);
        status.finish_with_verdict(
            10,
            "Retry",
            &super::super::committed::SourceVersions::default(),
            RunIntent::Running,
            true,
            false,
            false,
            &verdict,
        );
        assert!(matches!(
            progress.attempt().unwrap().outcome,
            AttemptOutcome::Unknown(actual) if Arc::ptr_eq(&actual, &cause)
        ));
        assert!(matches!(
            status.conclusion,
            Conclusion::Unknown { cause: actual } if Arc::ptr_eq(&actual, &cause)
        ));
    }

    #[test]
    fn mailbox_source_change_supersedes_already_failed_core_attempt() {
        use chimera_core::client::reconciler::plan::{Conclusion, RunIntent};
        let prepared = ApplyOutcome::Refused {
            cause: Arc::new(chimera_core::client::reconciler::plan::RuntimeError::new(
                "old candidate refused",
            )),
        };
        // A newer source notice can arrive after the Core error but before
        // Completed; the old failure must not poison the current target.
        let result = Err(anyhow::anyhow!("old candidate refused"));
        let verdict = observed_apply_verdict(&result, false, true, Some(&prepared));

        let mut progress = RuntimeProgress::default();
        progress.stage("superseded-profile".into(), ExecutionHost::Local);
        progress.settle_with_verdict(&verdict, false);

        let mut status = super::super::legacy_status::ReconcilerStatus::default();
        status.begin(11);
        status.finish_with_verdict(
            11,
            "SourceCommitted",
            &super::super::committed::SourceVersions::default(),
            RunIntent::Running,
            true,
            false,
            false,
            &verdict,
        );
        assert!(
            progress.attempt().is_none(),
            "superseded failures must not enter the retry-blocking cache"
        );
        assert!(matches!(status.conclusion, Conclusion::Waiting { .. }));
    }

    #[test]
    fn one_owner_retains_attempt_across_selection_invalidation() {
        let mut progress = RuntimeProgress::default();
        let selection = Selection::of(&super::super::inputs::RuntimeInputs {
            app: ChimeraAppConfig::default(),
            clash: ClashConfig::default(),
            profiles: Arc::new(Profiles::default()),
            content: Default::default(),
        });
        progress.set_selection(Some(selection.clone()));
        assert_eq!(progress.selection(), Some(selection));

        let host = ExecutionHost::Local;
        let refusal = Err(anyhow::anyhow!("Core refused committed candidate"));
        progress.stage("committed-target".into(), host);
        assert!(
            progress.attempt().is_none(),
            "worker admission alone cannot produce the final ref Attempt"
        );
        progress.settle(&refusal, false, false, false, None);
        assert!(
            progress
                .allow("committed-target", host, Trigger::SourceCommitted)
                .is_err()
        );
        progress.invalidate_selection();
        assert!(progress.selection().is_none());
        assert!(
            progress
                .allow("committed-target", host, Trigger::SourceCommitted)
                .is_err()
        );
        assert!(
            progress
                .allow("committed-target", host, Trigger::Retry)
                .is_ok()
        );
        assert!(
            progress
                .allow("new-target", host, Trigger::SourceCommitted)
                .is_ok()
        );
    }

    #[test]
    fn final_actor_fence_prevents_premature_apply_or_refusal() {
        use chimera_core::client::reconciler::plan::AttemptOutcome;
        let host = ExecutionHost::Local;
        let mut progress = RuntimeProgress::default();

        progress.stage("target-a".into(), host);
        progress.settle(&Ok(()), false, false, false, None);
        assert!(
            progress.attempt().is_none(),
            "Ok without verified Running is Waiting"
        );

        progress.stage("target-a".into(), host);
        progress.settle(&Ok(()), false, false, true, None);
        assert!(matches!(
            progress
                .attempt()
                .expect("confirmed running must settle")
                .outcome,
            AttemptOutcome::Applied
        ));

        let refused = super::super::mutation::ApplyOutcome::Refused {
            cause: Arc::new(chimera_core::client::reconciler::plan::RuntimeError::new(
                "rejected candidate",
            )),
        };
        progress.stage("target-b".into(), host);
        progress.settle(
            &Err(anyhow::Error::new(
                super::super::attempt::CommittedTargetAdvanced,
            )),
            false,
            true,
            false,
            Some(&refused),
        );
        assert!(
            progress.attempt().is_none(),
            "new committed source must supersede an earlier Core refusal"
        );
        assert!(
            progress
                .allow("target-b", host, Trigger::SourceCommitted)
                .is_ok(),
            "a moving target is never cached as a refusal"
        );

        progress.stage("target-c".into(), host);
        progress.settle(
            &Err(anyhow::anyhow!("Core refused")),
            false,
            false,
            false,
            Some(&refused),
        );
        assert!(matches!(
            progress
                .attempt()
                .expect("confirmed refusal should settle")
                .outcome,
            AttemptOutcome::Refused(_)
        ));
        assert!(
            progress
                .allow("target-c", host, Trigger::SourceCommitted)
                .is_err()
        );
    }

    #[test]
    fn unknown_terminal_and_service_fallback_use_last_staged_owner() {
        use chimera_core::client::reconciler::plan::AttemptOutcome;
        let mut progress = RuntimeProgress::default();
        let cause = Arc::new(chimera_core::client::reconciler::plan::RuntimeError::new(
            "unknown operation",
        ));
        let unknown = ApplyOutcome::Unknown(cause.clone());
        progress.stage("target-a".into(), ExecutionHost::Service);
        progress.settle(
            &Err(anyhow::anyhow!("lost operation terminal")),
            true,
            false,
            false,
            Some(&unknown),
        );
        assert!(matches!(
            progress.attempt().unwrap().outcome,
            AttemptOutcome::Unknown(observed) if Arc::ptr_eq(&observed, &cause)
        ));

        progress.stage("target-b".into(), ExecutionHost::Service);
        progress.stage("target-b".into(), ExecutionHost::Local);
        progress.settle(&Ok(()), false, false, true, None);
        let final_attempt = progress.attempt().unwrap();
        assert_eq!(final_attempt.host, ExecutionHost::Local);
        assert!(matches!(final_attempt.outcome, AttemptOutcome::Applied));

        progress.stage("obsolete".into(), ExecutionHost::Local);
        progress.clear_pending();
        progress.settle(
            &Err(anyhow::anyhow!("unrelated command failed")),
            false,
            false,
            false,
            None,
        );
        assert_eq!(progress.attempt().unwrap().target, "target-b");
    }
}

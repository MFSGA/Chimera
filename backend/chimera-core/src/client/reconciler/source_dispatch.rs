//! Shared source message handling for the single RuntimeReconciler mailbox.
//! Only the desktop adapter performs queue insertion, timers and RPC replies.
use super::{
    checked::{CheckedRuntimeCandidate, CheckedSelection},
    committed::SourceDomain,
    plan::RunIntent,
    workflow::{
        CheckedSourceNoticeKind, RuntimeReconcilerState, checked_mode_effect_deferred,
        deferred_mode_effect_degradation,
    },
};
use chimera_contracts::{runtime::Degradation, runtime_error::RuntimeError};
use std::sync::Arc;

/// One synchronous mailbox observation; never sampled across a Core await.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceDispatchFacts {
    pub shutting_down: bool,
    pub uncertain: bool,
    pub startup_owner_unproven: bool,
    pub owner_transition_pending: bool,
    pub queue_full: bool,
    pub authority_available: bool,
}

#[derive(Default)]
pub enum SourceAction {
    #[default]
    None,
    MarkDirty,
    RecoverStartup,
    ReplanAfterHandoff,
    ApplyChecked(CheckedRuntimeCandidate),
}

pub struct SourceDispatch {
    pub observe_source_only: bool,
    pub action: SourceAction,
    pub degradation: Option<Degradation>,
}

impl RuntimeReconcilerState {
    pub fn admit_checked_candidate(
        &mut self,
        checked: CheckedRuntimeCandidate,
        facts: SourceDispatchFacts,
    ) -> Result<(), Arc<RuntimeError>> {
        if facts.shutting_down || facts.uncertain {
            Err(Arc::new(RuntimeError::ShuttingDown))
        } else {
            self.checked.accept(checked)
        }
    }

    pub fn abort_checked_candidate(
        &mut self,
        domain: SourceDomain,
        request: Option<&str>,
    ) -> usize {
        match request {
            Some(request) => self.checked.discard_request(domain, request),
            None => self.checked.discard_domain(domain),
        }
    }

    fn running_and_available(&self, facts: SourceDispatchFacts) -> bool {
        self.run_intent == RunIntent::Running && !facts.uncertain && !facts.shutting_down
    }

    fn request_latest_source(
        &mut self,
        facts: SourceDispatchFacts,
        force_handoff: bool,
        replan_pending: bool,
    ) -> SourceAction {
        if force_handoff || facts.startup_owner_unproven || facts.owner_transition_pending {
            self.source_after_handoff = true;
            if facts.startup_owner_unproven {
                SourceAction::RecoverStartup
            } else if replan_pending || !facts.owner_transition_pending {
                SourceAction::ReplanAfterHandoff
            } else {
                SourceAction::None
            }
        } else {
            self.source_dirty = true;
            SourceAction::MarkDirty
        }
    }

    pub fn dispatch_source_committed(
        &mut self,
        domain: SourceDomain,
        version: u64,
        reconcile_requested: bool,
        startup_recovery_required: bool,
        facts: SourceDispatchFacts,
    ) -> SourceDispatch {
        self.seen_sources.observe(domain, version);
        let action = if reconcile_requested && self.running_and_available(facts) {
            self.request_latest_source(facts, startup_recovery_required, false)
        } else {
            SourceAction::None
        };
        SourceDispatch {
            observe_source_only: !reconcile_requested || self.run_intent == RunIntent::Stopped,
            action,
            degradation: None,
        }
    }

    /// Checked is an admitted proposal, never a second owner. Only a uniquely
    /// matched durable receipt can authorize the prepared submission.
    pub fn dispatch_checked_source(
        &mut self,
        domain: SourceDomain,
        version: u64,
        request: Option<&str>,
        facts: SourceDispatchFacts,
    ) -> SourceDispatch {
        let notice = self.observe_checked_source(domain, version, request);
        let action = match notice.kind {
            CheckedSourceNoticeKind::Duplicate => {
                tracing::debug!(?domain, version, "duplicate checked commit ignored");
                SourceAction::None
            }
            CheckedSourceNoticeKind::Stale => {
                tracing::debug!(?domain, version, previous = ?notice.previous, "stale checked receipt requires latest source recovery");
                if self.running_and_available(facts) {
                    self.request_latest_source(facts, false, true)
                } else {
                    SourceAction::None
                }
            }
            CheckedSourceNoticeKind::Superseded => {
                tracing::debug!(
                    ?domain,
                    version,
                    "ignoring superseded checked source notice"
                );
                SourceAction::None
            }
            CheckedSourceNoticeKind::Current if !self.running_and_available(facts) => {
                tracing::debug!(
                    ?domain,
                    version,
                    "checked commit observed while runtime stopped or unavailable"
                );
                SourceAction::None
            }
            CheckedSourceNoticeKind::Current
                if facts.startup_owner_unproven || facts.owner_transition_pending =>
            {
                self.request_latest_source(facts, false, true)
            }
            CheckedSourceNoticeKind::Current if facts.queue_full || !facts.authority_available => {
                self.source_dirty = true;
                SourceAction::MarkDirty
            }
            CheckedSourceNoticeKind::Current => {
                let (selection, checked) = self.checked.select_committed(domain, request);
                match (selection, checked) {
                    (CheckedSelection::Matched, Some(checked)) => {
                        SourceAction::ApplyChecked(checked)
                    }
                    (selection, _) => {
                        tracing::debug!(
                            ?domain,
                            ?selection,
                            "checked source requires latest-target recovery"
                        );
                        self.source_dirty = true;
                        SourceAction::MarkDirty
                    }
                }
            }
        };
        let degradation = checked_mode_effect_deferred(
            notice.previous,
            version,
            notice.mode_requested,
            matches!(action, SourceAction::ApplyChecked(_)),
            self.run_intent == RunIntent::Running && !facts.shutting_down,
        )
        .then(|| deferred_mode_effect_degradation(version));
        if let Some(degradation) = &degradation {
            self.connection_interruption_maintenance = Some(degradation.message.clone());
        }
        SourceDispatch {
            observe_source_only: true,
            action,
            degradation,
        }
    }

    pub fn take_explicit_checked(
        &mut self,
        domain: SourceDomain,
        version: u64,
        target: &str,
        facts: SourceDispatchFacts,
    ) -> Option<CheckedRuntimeCandidate> {
        if !self.running_and_available(facts)
            || facts.startup_owner_unproven
            || facts.owner_transition_pending
            || facts.queue_full
            || self.seen_sources.version(domain) != Some(version)
            || self.checked.matching_count(domain, Some(target)) != 1
        {
            return None;
        }
        let (selection, checked) = self.checked.take_unique(domain, Some(target));
        debug_assert_eq!(selection, CheckedSelection::Matched);
        checked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_commit_retains_handoff_debt_until_owner_proven() {
        let mut state = RuntimeReconcilerState::default();
        let domain = SourceDomain::Application;
        let facts = SourceDispatchFacts {
            startup_owner_unproven: true,
            ..Default::default()
        };
        let result = state.dispatch_source_committed(domain, 7, true, false, facts);
        assert!(matches!(result.action, SourceAction::RecoverStartup));
        assert!(state.source_after_handoff);
        assert!(!state.source_dirty);
        assert_eq!(state.seen_sources.version(domain), Some(7));
        let result =
            state.dispatch_source_committed(domain, 8, true, true, SourceDispatchFacts::default());
        assert!(matches!(result.action, SourceAction::ReplanAfterHandoff));
        assert_eq!(state.seen_sources.version(domain), Some(8));
    }
    #[test]
    fn stopped_owner_observes_durable_source_without_automatic_submission() {
        let mut state = RuntimeReconcilerState::default();
        state.run_intent = RunIntent::Stopped;
        let result = state.dispatch_source_committed(
            SourceDomain::Profiles,
            3,
            true,
            false,
            Default::default(),
        );
        assert!(result.observe_source_only);
        assert!(matches!(result.action, SourceAction::None));
        let result =
            state.dispatch_checked_source(SourceDomain::Profiles, 4, None, Default::default());
        assert!(matches!(result.action, SourceAction::None));
        assert_eq!(state.seen_sources.version(SourceDomain::Profiles), Some(4));
        assert!(!state.source_dirty);
    }
    #[test]
    fn missing_authority_or_full_queue_replans_without_consuming_a_proposal() {
        for facts in [
            SourceDispatchFacts::default(),
            SourceDispatchFacts {
                queue_full: true,
                authority_available: true,
                ..Default::default()
            },
        ] {
            let mut state = RuntimeReconcilerState::default();
            let result = state.dispatch_checked_source(SourceDomain::Clash, 1, None, facts);
            assert!(matches!(result.action, SourceAction::MarkDirty));
            assert!(state.source_dirty);
            let result = state.dispatch_checked_source(SourceDomain::Clash, 1, None, facts);
            assert!(matches!(result.action, SourceAction::None));
        }
    }
}

//! RuntimeReconciler's bounded pre-commit receipt cache.
use super::committed::SourceDomain;
use crate::client::core_lifecycle::ports::PreparedRuntime;
use std::{collections::VecDeque, sync::Arc};

pub const MAX_CHECKED_PENDING: usize = 32;

/// One admitted checked product *and the exact executable specification* used
/// for validation. Do not re-resolve the binary at SourceCommitted or Apply.
pub struct CheckedRuntimeCandidate {
    pub domain: SourceDomain,
    /// Source request identity, not a Core ownership lease. None preserves
    /// legacy Profiles admission where the version notice lacks a receipt.
    pub request: Option<String>,
    /// Explicit mode interruption from the *source patch plan*, owned by
    /// this Checked receipt. It is never reconstructed from an old Core mode.
    pub interrupt_mode: bool,
    pub prepared: PreparedRuntime,
    pub core_spec: chimera_core_manager::CoreSpec,
}

#[derive(Default)]
pub struct CheckedRuntimeInbox {
    pending: VecDeque<CheckedRuntimeCandidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckedSelection {
    Missing,
    Ambiguous,
    Matched,
}

impl CheckedRuntimeInbox {
    pub fn accept(
        &mut self,
        checked: CheckedRuntimeCandidate,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        use chimera_contracts::runtime_error::{RuntimeCheckUnavailable, RuntimeError};
        if checked.interrupt_mode && checked.domain != SourceDomain::Clash {
            return Err(Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: "checked mode interruption requires a Clash source receipt".into(),
                },
            }));
        }
        if checked.prepared.target.is_none() {
            return Err(Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: "checked candidate has no source target identity".into(),
                },
            }));
        }
        if self.pending.len() >= MAX_CHECKED_PENDING {
            return Err(Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: "checked candidate inbox is full; source commit must not be authorized"
                        .into(),
                },
            }));
        }
        if let Some(request) = checked.request.as_deref()
            && self.pending.iter().any(|entry| {
                entry.domain == checked.domain && entry.request.as_deref() == Some(request)
            })
        {
            return Err(Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: "duplicate checked request identity for one source domain".into(),
                },
            }));
        }
        self.pending.push_back(checked);
        Ok(())
    }

    /// Failed source CAS cannot authorize Core Apply. Without an operation
    /// token, discard every uncommitted proposal in that domain: another
    /// racing successful CAS will conservatively rebuild the latest source.
    /// Never discard checks admitted for the other source domains.
    pub fn discard_request(&mut self, domain: SourceDomain, request: &str) -> usize {
        let before = self.pending.len();
        self.pending
            .retain(|entry| entry.domain != domain || entry.request.as_deref() != Some(request));
        before - self.pending.len()
    }

    pub fn take_request(
        &mut self,
        domain: SourceDomain,
        request: &str,
    ) -> Option<CheckedRuntimeCandidate> {
        let index = self.pending.iter().position(|entry| {
            entry.domain == domain && entry.request.as_deref() == Some(request)
        })?;
        self.pending.remove(index)
    }

    pub fn discard_domain(&mut self, domain: SourceDomain) -> usize {
        let before = self.pending.len();
        self.pending.retain(|entry| entry.domain != domain);
        before - self.pending.len()
    }

    pub fn matching_count(&self, domain: SourceDomain, target: Option<&str>) -> usize {
        self.pending
            .iter()
            .filter(|entry| {
                entry.domain == domain
                    && target
                        .is_none_or(|expected| entry.prepared.target.as_deref() == Some(expected))
            })
            .count()
    }

    /// A source notification has no target identity; consume its checked
    /// product only if exactly one candidate belongs to that source domain.
    /// Multiple candidates in one domain may have been committed in another
    /// order, so discard that domain's ambiguous proposals and rebuild the
    /// authoritative *latest committed* target in the existing Core owner.
    pub fn take_unique(
        &mut self,
        domain: SourceDomain,
        target: Option<&str>,
    ) -> (CheckedSelection, Option<CheckedRuntimeCandidate>) {
        let matched = self.matching_count(domain, target);
        if matched == 0 {
            return (CheckedSelection::Missing, None);
        }
        if matched > 1 {
            self.pending.retain(|entry| entry.domain != domain);
            return (CheckedSelection::Ambiguous, None);
        }
        let index = self
            .pending
            .iter()
            .position(|entry| {
                entry.domain == domain
                    && target
                        .is_none_or(|expected| entry.prepared.target.as_deref() == Some(expected))
            })
            .expect("the matched checked candidate is still in this mailbox");
        (CheckedSelection::Matched, self.pending.remove(index))
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn requests_mode_interruption(&self, domain: SourceDomain, request: &str) -> bool {
        self.pending.iter().any(|candidate| {
            candidate.domain == domain
                && candidate.request.as_deref() == Some(request)
                && candidate.interrupt_mode
        })
    }

    pub fn select_committed(
        &mut self,
        domain: SourceDomain,
        request: Option<&str>,
    ) -> (CheckedSelection, Option<CheckedRuntimeCandidate>) {
        if let Some(request) = request {
            match self.take_request(domain, request) {
                Some(checked) => (CheckedSelection::Matched, Some(checked)),
                None => (CheckedSelection::Missing, None),
            }
        } else {
            self.take_unique(domain, None)
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn first(&self) -> Option<&CheckedRuntimeCandidate> {
        self.pending.front()
    }
}

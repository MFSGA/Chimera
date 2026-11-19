//! Commit-version fencing for the compatibility runtime owner.
//!
//! ref RuntimeGate::committed drives one reconciler against the latest source
//! snapshots. Until that actor is ported, the existing synchronous apply path
//! must not deliberately apply a target that is already superseded. Check
//! before *and* after CoreLifecycle's mutation, and leave the source committed
//! if an apply or version observation fails.
use std::future::Future;

use anyhow::Result;

use crate::client::{
    application_workflow::tcc::safe_to_retry_deferred_runtime,
    runtime::{Degradation, DegradationPhase},
};

fn deferred(domain: &'static str, message: String, retryable: bool) -> Degradation {
    Degradation {
        phase: DegradationPhase::RuntimeApply,
        code: format!("{domain}_runtime_deferred"),
        message,
        retryable,
    }
}

fn out_of_date(
    domain: &'static str,
    version: u64,
    latest: Result<u64>,
    when: &'static str,
    apply_error: Option<&anyhow::Error>,
) -> Option<Degradation> {
    let suffix = apply_error
        .map(|error| format!("; previous apply failed: {error:#}"))
        .unwrap_or_default();
    match latest {
        Ok(actual) if actual == version => None,
        Ok(actual) => Some(deferred(
            domain,
            format!(
                "{domain} source advanced from version {version} to {actual} {when} runtime apply; the latest committed target needs reconciliation{suffix}"
            ),
            true,
        )),
        Err(error) => Some(deferred(
            domain,
            format!(
                "{domain} config was saved, but latest source version could not be verified {when} runtime apply: {error:#}{suffix}"
            ),
            true,
        )),
    }
}

/// Apply only while the committed source version is still current.
///
/// The pre-apply fence avoids knowingly running a stale target. The post-apply
/// fence handles a new commit racing a Core operation already in flight. Both
/// races schedule convergence of the latest target through the *existing*
/// CoreLifecycle actor; this helper never starts a second runtime controller.
pub(crate) async fn apply_committed<Read, ReadFuture, Apply, ApplyFuture>(
    domain: &'static str,
    committed_version: u64,
    mut read_version: Read,
    apply: Apply,
) -> Option<Degradation>
where
    Read: FnMut() -> ReadFuture,
    ReadFuture: Future<Output = Result<u64>>,
    Apply: FnOnce() -> ApplyFuture,
    ApplyFuture: Future<Output = Result<()>>,
{
    if let Some(degradation) = out_of_date(
        domain,
        committed_version,
        read_version().await,
        "before",
        None,
    ) {
        return Some(degradation);
    }

    let result = apply().await;
    if let Some(degradation) = out_of_date(
        domain,
        committed_version,
        read_version().await,
        "after",
        result.as_ref().err(),
    ) {
        return Some(degradation);
    }
    result.err().map(|error| {
        let retryable = safe_to_retry_deferred_runtime(&error);
        deferred(
            domain,
            format!("{domain} config was saved, but runtime reconciliation failed: {error:#}"),
            retryable,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    };

    use super::*;

    #[tokio::test]
    async fn superseded_commit_never_applies_a_stale_runtime_target() {
        let current = Arc::new(AtomicU64::new(4));
        let applies = Arc::new(AtomicUsize::new(0));
        let reads = current.clone();
        let writes = applies.clone();
        let deferred = apply_committed(
            "clash",
            3,
            move || {
                let current = reads.clone();
                async move { Ok(current.load(Ordering::Acquire)) }
            },
            move || async move {
                writes.fetch_add(1, Ordering::AcqRel);
                Ok(())
            },
        )
        .await
        .expect("superseded commit is deferred");
        assert_eq!(applies.load(Ordering::Acquire), 0);
        assert_eq!(deferred.code, "clash_runtime_deferred");
        assert!(deferred.retryable);
        assert!(deferred.message.contains("before runtime apply"));
    }

    #[tokio::test]
    async fn concurrent_source_advance_during_apply_requires_reconcile() {
        let version = Arc::new(AtomicU64::new(3));
        let read = version.clone();
        let advance = version.clone();
        let deferred = apply_committed(
            "application",
            3,
            move || {
                let current = read.clone();
                async move { Ok(current.load(Ordering::Acquire)) }
            },
            move || async move {
                advance.store(4, Ordering::Release);
                Ok(())
            },
        )
        .await
        .expect("a newer source target needs another reconcile");
        assert_eq!(deferred.code, "application_runtime_deferred");
        assert!(deferred.retryable);
        assert!(deferred.message.contains("after runtime apply"));
    }

    #[tokio::test]
    async fn unverified_source_version_fails_closed_before_any_apply() {
        let applies = Arc::new(AtomicUsize::new(0));
        let writes = applies.clone();
        let deferred = apply_committed(
            "clash",
            1,
            || async { anyhow::bail!("source actor cannot be read") },
            move || async move {
                writes.fetch_add(1, Ordering::AcqRel);
                Ok(())
            },
        )
        .await
        .expect("version-read failure must defer");
        assert_eq!(applies.load(Ordering::Acquire), 0);
        assert!(deferred.retryable);
        assert!(deferred.message.contains("before runtime apply"));
    }

    #[tokio::test]
    async fn runtime_apply_error_preserves_committed_source() {
        let deferred = apply_committed(
            "application",
            12,
            || async { Ok(12) },
            || async { anyhow::bail!("injected core failure") },
        )
        .await
        .expect("apply error reported as a deferred runtime");
        assert!(deferred.message.contains("injected core failure"));
        assert_eq!(deferred.code, "application_runtime_deferred");
    }

    #[tokio::test]
    async fn fresh_committed_target_applies_once_and_converges() {
        let calls = Arc::new(AtomicUsize::new(0));
        let apply_calls = calls.clone();
        let deferred = apply_committed(
            "clash",
            7,
            || async { Ok(7) },
            move || async move {
                apply_calls.fetch_add(1, Ordering::AcqRel);
                Ok(())
            },
        )
        .await;
        assert!(deferred.is_none());
        assert_eq!(calls.load(Ordering::Acquire), 1);
    }
}

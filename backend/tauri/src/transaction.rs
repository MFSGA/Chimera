use std::future::Future;

use anyhow::Result;

/// Preserve the primary failure while attaching a failed compensation attempt.
pub(crate) fn preserve_primary_failure(
    context: &str,
    primary: anyhow::Error,
    compensation: Result<()>,
) -> anyhow::Error {
    match compensation {
        Ok(()) => primary,
        Err(compensation) => {
            anyhow::anyhow!("{context}: {primary:#}; runtime compensation failed: {compensation:#}")
        }
    }
}

/// Persist a prepared change and compensate its runtime effects when persistence fails.
pub(crate) async fn persist_with_compensation<Persist, Compensate, CompensateFuture>(
    persist: Persist,
    compensate: Compensate,
) -> Result<()>
where
    Persist: FnOnce() -> Result<()>,
    Compensate: FnOnce() -> CompensateFuture,
    CompensateFuture: Future<Output = Result<()>>,
{
    match persist() {
        Ok(()) => Ok(()),
        Err(primary) => Err(preserve_primary_failure(
            "configuration persistence failed",
            primary,
            compensate().await,
        )),
    }
}

/// Final state of a transaction after its primary operation and optional rollback.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TransactionOutcome<E> {
    Committed,
    RolledBack { primary_error: E },
    RollbackFailed { primary_error: E, rollback_error: E },
}

/// Commit a prepared change, apply it to runtime state, and roll back if applying fails.
pub(crate) async fn commit_then_apply_with_rollback<
    Commit,
    Apply,
    ApplyFuture,
    Rollback,
    RollbackFuture,
    E,
>(
    commit: Commit,
    apply: Apply,
    rollback: Rollback,
) -> std::result::Result<TransactionOutcome<E>, E>
where
    Commit: FnOnce() -> std::result::Result<(), E>,
    Apply: FnOnce() -> ApplyFuture,
    ApplyFuture: Future<Output = std::result::Result<(), E>>,
    Rollback: FnOnce() -> RollbackFuture,
    RollbackFuture: Future<Output = std::result::Result<(), E>>,
{
    commit()?;

    match apply().await {
        Ok(()) => Ok(TransactionOutcome::Committed),
        Err(primary_error) => match rollback().await {
            Ok(()) => Ok(TransactionOutcome::RolledBack { primary_error }),
            Err(rollback_error) => Ok(TransactionOutcome::RollbackFailed {
                primary_error,
                rollback_error,
            }),
        },
    }
}

/// Apply a runtime change, commit it, and roll back runtime state if the commit fails.
pub(crate) async fn apply_then_commit_with_rollback<
    Apply,
    ApplyFuture,
    Commit,
    CommitFuture,
    Rollback,
    RollbackFuture,
    E,
>(
    apply: Apply,
    commit: Commit,
    rollback: Rollback,
) -> std::result::Result<TransactionOutcome<E>, E>
where
    Apply: FnOnce() -> ApplyFuture,
    ApplyFuture: Future<Output = std::result::Result<(), E>>,
    Commit: FnOnce() -> CommitFuture,
    CommitFuture: Future<Output = std::result::Result<(), E>>,
    Rollback: FnOnce() -> RollbackFuture,
    RollbackFuture: Future<Output = std::result::Result<(), E>>,
{
    apply().await?;

    match commit().await {
        Ok(()) => Ok(TransactionOutcome::Committed),
        Err(primary_error) => match rollback().await {
            Ok(()) => Ok(TransactionOutcome::RolledBack { primary_error }),
            Err(rollback_error) => Ok(TransactionOutcome::RollbackFailed {
                primary_error,
                rollback_error,
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        TransactionOutcome, apply_then_commit_with_rollback, commit_then_apply_with_rollback,
        persist_with_compensation,
    };

    fn push(events: &Arc<Mutex<Vec<&'static str>>>, event: &'static str) {
        events.lock().expect("transaction event lock").push(event);
    }

    #[tokio::test]
    async fn persistence_failure_runs_compensation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let persist_events = Arc::clone(&events);
        let compensate_events = Arc::clone(&events);

        let error = persist_with_compensation(
            move || {
                push(&persist_events, "persist");
                Err(anyhow::anyhow!("persist failed"))
            },
            move || async move {
                push(&compensate_events, "compensate");
                Ok(())
            },
        )
        .await
        .expect_err("persistence failure must be returned");

        assert!(error.to_string().contains("persist failed"));
        assert_eq!(
            *events.lock().expect("transaction event lock"),
            vec!["persist", "compensate"]
        );
    }

    #[tokio::test]
    async fn committed_apply_success_reports_committed_state() {
        let outcome = commit_then_apply_with_rollback(
            || Ok::<(), &'static str>(()),
            || async { Ok(()) },
            || async { panic!("rollback must not run after a successful apply") },
        )
        .await
        .expect("commit and apply must succeed");

        assert_eq!(outcome, TransactionOutcome::Committed);
    }

    #[tokio::test]
    async fn committed_apply_failure_reports_restored_state() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let commit_events = Arc::clone(&events);
        let apply_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let outcome = commit_then_apply_with_rollback(
            move || {
                push(&commit_events, "commit");
                Ok::<(), &'static str>(())
            },
            move || async move {
                push(&apply_events, "apply");
                Err("apply failed")
            },
            move || async move {
                push(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect("commit succeeds");

        assert!(matches!(
            outcome,
            TransactionOutcome::RolledBack {
                primary_error: "apply failed"
            }
        ));
        assert_eq!(
            *events.lock().expect("transaction event lock"),
            vec!["commit", "apply", "rollback"]
        );
    }

    #[tokio::test]
    async fn committed_apply_failure_reports_failed_rollback() {
        let outcome = commit_then_apply_with_rollback(
            || Ok::<(), &'static str>(()),
            || async { Err("apply failed") },
            || async { Err("rollback failed") },
        )
        .await
        .expect("commit succeeds");

        assert!(matches!(
            outcome,
            TransactionOutcome::RollbackFailed {
                primary_error: "apply failed",
                rollback_error: "rollback failed"
            }
        ));
    }

    #[tokio::test]
    async fn commit_failure_runs_runtime_rollback() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let apply_events = Arc::clone(&events);
        let commit_events = Arc::clone(&events);
        let rollback_events = Arc::clone(&events);

        let outcome = apply_then_commit_with_rollback(
            move || async move {
                push(&apply_events, "apply");
                Ok::<(), &'static str>(())
            },
            move || async move {
                push(&commit_events, "commit");
                Err("commit failed")
            },
            move || async move {
                push(&rollback_events, "rollback");
                Ok(())
            },
        )
        .await
        .expect("runtime apply succeeds");

        assert_eq!(
            outcome,
            TransactionOutcome::RolledBack {
                primary_error: "commit failed"
            }
        );
        assert_eq!(
            *events.lock().expect("transaction event lock"),
            vec!["apply", "commit", "rollback"]
        );
    }
}

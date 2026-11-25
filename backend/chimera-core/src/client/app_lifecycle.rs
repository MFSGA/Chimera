//! Application lifecycle, following ref's root token and tracked owner drain.
//! The composition root supplies the already-created Core owner; no new owner
//! or synthetic shutdown success is created here.
use crate::{
    actor_v2::router::CoreStatusProjection,
    client::{
        core_lifecycle::ports::CoreLifecyclePort,
        profiles::ProfilesClient,
        reconciler::startup_policy::{StartupOutcome, StartupReport},
    },
};
use std::future::Future;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct CompletedLifecycleOperation {
    pub result: anyhow::Result<()>,
    pub report: StartupReport,
}

pub fn startup_report(completed: anyhow::Result<CompletedLifecycleOperation>) -> StartupReport {
    let report = match completed {
        Ok(completed) => {
            if let Err(error) = completed.result {
                tracing::warn!(
                    operation_id = completed.report.operation_id,
                    %error,
                    "startup reconciliation did not reach ready"
                );
            }
            completed.report
        }
        Err(error) => StartupReport {
            operation_id: 0,
            observation: None,
            outcome: StartupOutcome::Unsettled {
                reason: format!("startup workflow did not return a terminal reply: {error:#}"),
            },
        },
    };

    report
}

pub async fn start_background_sources(profiles: Option<&ProfilesClient>) -> anyhow::Result<()> {
    if let Some(profiles) = profiles {
        profiles.start_producers().await?;
    }
    Ok(())
}

pub fn request_shutdown(shutdown: &CancellationToken, tasks: &TaskTracker) {
    shutdown.cancel();
    tasks.close();
}

pub fn shutdown_child_token(shutdown: &CancellationToken) -> CancellationToken {
    shutdown.child_token()
}

/// Register before handing a producer future to the caller (ref boundary API).
pub fn track_until_shutdown(
    tasks: &TaskTracker,
    token: &CancellationToken,
    producer: impl Future<Output = ()> + Send + 'static,
) -> impl Future<Output = ()> + Send + 'static {
    chimera_owner_support::shutdown::track_until_shutdown(tasks, token, producer)
}

pub fn spawn_tracked(
    shutdown: &CancellationToken,
    tasks: &TaskTracker,
    task: impl Future<Output = ()> + Send + 'static,
) -> bool {
    if shutdown.is_cancelled() {
        return false;
    }
    tasks.spawn(task);
    true
}

pub async fn wait_shutdown(
    tasks: &TaskTracker,
    core: &dyn CoreLifecyclePort,
    stop: impl Future<Output = anyhow::Result<()>>,
    routed_status: impl FnOnce() -> Option<CoreStatusProjection>,
) -> anyhow::Result<()> {
    tracing::debug!(
        remaining = tasks.len(),
        "waiting for tracked shutdown tasks"
    );
    tasks.wait().await;
    tracing::debug!("tracked shutdown tasks drained; stopping core lifecycle");
    let stopped = stop.await;
    tracing::debug!(
        success = stopped.is_ok(),
        "core lifecycle shutdown returned; draining core executor"
    );
    // Always drain the router and local executor, even when ordinary
    // stop has failed. Preserve the original stop uncertainty as the
    // error returned to the updater/exit boundary.
    let drained = core.finish_shutdown().await;
    tracing::debug!(success = drained.is_ok(), "core executor shutdown returned");
    // A compatibility adapter that claimed to finish shutdown but left
    // CoreActor connected cannot be treated as an updater-safe stop.
    // In tests constructed without ref actors this check is skipped.
    let actor_closed = routed_status().map(|status| {
        anyhow::ensure!(
            matches!(
                status.connectivity,
                crate::actor_v2::router::EndpointConnectivity::ShutDown
            ),
            "the ref Core Actor did not finish its shutdown"
        );
        Ok(())
    });
    stopped?;
    drained?;
    actor_closed.transpose()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    /// Flags its own drop, which is how a cancelled producer ends.
    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn the_cancel_ends_a_running_producer_and_the_wait_sees_it_gone() {
        let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let task = tokio::spawn(track_until_shutdown(&tasks, &token, async move {
            let _guard = guard;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        }));
        started_rx.await.unwrap();

        request_shutdown(&token, &tasks);
        tasks.wait().await;

        assert!(dropped.load(Ordering::SeqCst));
        task.await.expect("a cancelled producer ends normally");
    }

    #[tokio::test]
    async fn a_producer_tracked_after_the_cancel_never_runs() {
        let (token, tasks) = (CancellationToken::new(), TaskTracker::new());
        token.cancel();

        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        tokio::spawn(track_until_shutdown(&tasks, &token, async move {
            flag.store(true, Ordering::SeqCst);
        }))
        .await
        .unwrap();
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[test]
    fn lost_startup_reply_never_claims_ready() {
        let report = startup_report(Err(anyhow::anyhow!("lost reply")));
        assert_eq!(report.operation_id, 0);
        assert!(
            matches!(report.outcome, StartupOutcome::Unsettled { reason } if reason.contains("lost reply"))
        );
    }

    #[test]
    fn known_startup_refusal_preserves_first_report() {
        let report = startup_report(Ok(CompletedLifecycleOperation {
            result: Err(anyhow::anyhow!("transport")),
            report: StartupReport {
                operation_id: 7,
                observation: None,
                outcome: StartupOutcome::Unsettled {
                    reason: "owner unknown".into(),
                },
            },
        }));
        assert_eq!(report.operation_id, 7);
        assert!(
            matches!(report.outcome, StartupOutcome::Unsettled { reason } if reason == "owner unknown")
        );
    }

    #[tokio::test]
    async fn shutdown_refuses_new_tracked_tasks_and_cancels_children() {
        let token = CancellationToken::new();
        let tasks = TaskTracker::new();
        let child = shutdown_child_token(&token);
        request_shutdown(&token, &tasks);
        request_shutdown(&token, &tasks);
        assert!(child.is_cancelled());
        assert!(!spawn_tracked(&token, &tasks, async {
            panic!("late producer ran");
        }));
        tasks.wait().await;
    }
}

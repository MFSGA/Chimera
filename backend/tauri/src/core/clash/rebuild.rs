use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::{mpsc, oneshot};

const COALESCE_WINDOW: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct RebuildNotifier {
    dirty_tx: mpsc::Sender<()>,
    active: Arc<AtomicBool>,
}

impl RebuildNotifier {
    /// Mark the runtime product dirty without blocking the caller.
    pub fn request_rebuild(&self) {
        if self.active.load(Ordering::Acquire) {
            // A full capacity-one channel already represents the dirty state.
            let _ = self.dirty_tx.try_send(());
        }
    }
}

struct WorkerControl {
    shutdown_tx: oneshot::Sender<()>,
    done_rx: oneshot::Receiver<()>,
}

struct CoordinatorControl {
    dirty_rx: Option<mpsc::Receiver<()>>,
    worker: Option<WorkerControl>,
}

/// Coalesces background-only rebuild requests while synchronous transactions continue to call
/// `CoreManager` directly and receive their exact result.
pub struct RebuildCoordinator {
    dirty_tx: mpsc::Sender<()>,
    active: Arc<AtomicBool>,
    control: Mutex<CoordinatorControl>,
}

impl RebuildCoordinator {
    pub fn new() -> Self {
        let (dirty_tx, dirty_rx) = mpsc::channel(1);
        Self {
            dirty_tx,
            active: Arc::new(AtomicBool::new(true)),
            control: Mutex::new(CoordinatorControl {
                dirty_rx: Some(dirty_rx),
                worker: None,
            }),
        }
    }

    pub fn notifier(&self) -> RebuildNotifier {
        RebuildNotifier {
            dirty_tx: self.dirty_tx.clone(),
            active: self.active.clone(),
        }
    }

    /// Start exactly one worker. Once a rebuild starts it runs to completion to avoid cancelling
    /// after candidate promotion or while a core restart is in progress.
    pub fn start_worker<F, Fut>(&self, rebuild: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let mut control = self.control.lock().expect("rebuild coordinator");
        let Some(rx) = control.dirty_rx.take() else {
            tracing::warn!("rebuild coordinator worker already started or shut down");
            return;
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (done_tx, done_rx) = oneshot::channel();
        spawn_worker(rx, shutdown_rx, done_tx, self.active.clone(), rebuild);
        control.worker = Some(WorkerControl {
            shutdown_tx,
            done_rx,
        });
    }

    pub async fn shutdown(&self) {
        self.active.store(false, Ordering::Release);
        let worker = {
            let mut control = self.control.lock().expect("rebuild coordinator");
            control.dirty_rx.take();
            control.worker.take()
        };
        if let Some(worker) = worker {
            let _ = worker.shutdown_tx.send(());
            let _ = worker.done_rx.await;
        }
    }
}

impl Default for RebuildCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RebuildCoordinator {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let control = self.control.get_mut().expect("rebuild coordinator");
        control.dirty_rx.take();
        if let Some(worker) = control.worker.take() {
            let _ = worker.shutdown_tx.send(());
        }
    }
}

fn spawn_worker<F, Fut>(
    mut dirty_rx: mpsc::Receiver<()>,
    mut shutdown_rx: oneshot::Receiver<()>,
    done_tx: oneshot::Sender<()>,
    active: Arc<AtomicBool>,
    rebuild: F,
) where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let worker = async move {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => break,
                dirty = dirty_rx.recv() => {
                    let Some(()) = dirty else { break };
                    tokio::select! {
                        biased;
                        _ = &mut shutdown_rx => break,
                        _ = tokio::time::sleep(COALESCE_WINDOW) => {}
                    }
                    if !active.load(Ordering::Acquire) {
                        break;
                    }
                    while dirty_rx.try_recv().is_ok() {}
                    if let Err(error) = rebuild().await {
                        tracing::warn!(%error, "coalesced background rebuild failed");
                    }
                }
            }
        }
        let _ = done_tx.send(());
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(worker);
    } else {
        tauri::async_runtime::spawn(worker);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn burst_requests_are_coalesced_into_one_rebuild() {
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        coordinator.start_worker(move || {
            let observed = observed.clone();
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        let notifier = coordinator.notifier();

        for _ in 0..20 {
            notifier.request_rebuild();
        }
        tokio::time::sleep(COALESCE_WINDOW + Duration::from_millis(50)).await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        coordinator.shutdown().await;
    }

    #[tokio::test]
    async fn requests_after_the_window_start_a_later_rebuild() {
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        coordinator.start_worker(move || {
            let observed = observed.clone();
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        let notifier = coordinator.notifier();

        notifier.request_rebuild();
        tokio::time::sleep(COALESCE_WINDOW + Duration::from_millis(50)).await;
        notifier.request_rebuild();
        tokio::time::sleep(COALESCE_WINDOW + Duration::from_millis(50)).await;

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        coordinator.shutdown().await;
    }

    #[tokio::test]
    async fn requests_during_a_rebuild_coalesce_into_one_follow_up() {
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let observed = calls.clone();
        let worker_started = started.clone();
        let worker_release = release.clone();
        coordinator.start_worker(move || {
            let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
            let worker_started = worker_started.clone();
            let worker_release = worker_release.clone();
            async move {
                if call == 1 {
                    worker_started.notify_one();
                    worker_release.notified().await;
                }
                Ok(())
            }
        });
        let notifier = coordinator.notifier();

        notifier.request_rebuild();
        started.notified().await;
        for _ in 0..20 {
            notifier.request_rebuild();
        }
        release.notify_one();
        tokio::time::sleep(COALESCE_WINDOW + Duration::from_millis(75)).await;

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        coordinator.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_waits_for_an_inflight_rebuild_to_finish() {
        let coordinator = Arc::new(RebuildCoordinator::new());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let finished = Arc::new(AtomicBool::new(false));
        let worker_started = started.clone();
        let worker_release = release.clone();
        let worker_finished = finished.clone();
        coordinator.start_worker(move || {
            let worker_started = worker_started.clone();
            let worker_release = worker_release.clone();
            let worker_finished = worker_finished.clone();
            async move {
                worker_started.notify_one();
                worker_release.notified().await;
                worker_finished.store(true, Ordering::SeqCst);
                Ok(())
            }
        });
        coordinator.notifier().request_rebuild();
        started.notified().await;

        let shutdown = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.shutdown().await })
        };
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        assert!(!finished.load(Ordering::SeqCst));

        release.notify_one();
        shutdown.await.unwrap();
        assert!(finished.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn shutdown_cancels_a_pending_window_and_disables_notifier() {
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        coordinator.start_worker(move || {
            let observed = observed.clone();
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        let notifier = coordinator.notifier();
        notifier.request_rebuild();
        coordinator.shutdown().await;
        notifier.request_rebuild();
        tokio::time::sleep(Duration::from_millis(25)).await;

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn starting_twice_keeps_the_original_worker() {
        let coordinator = RebuildCoordinator::new();
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let observed_first = first_calls.clone();
        coordinator.start_worker(move || {
            let observed_first = observed_first.clone();
            async move {
                observed_first.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        let observed_second = second_calls.clone();
        coordinator.start_worker(move || {
            let observed_second = observed_second.clone();
            async move {
                observed_second.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });

        coordinator.notifier().request_rebuild();
        tokio::time::sleep(COALESCE_WINDOW + Duration::from_millis(50)).await;

        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(Ordering::SeqCst), 0);
        coordinator.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_waits_for_an_inflight_rebuild_without_starting_a_follow_up() {
        let coordinator = Arc::new(RebuildCoordinator::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let observed = calls.clone();
        let observed_started = started.clone();
        let observed_release = release.clone();
        coordinator.start_worker(move || {
            let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
            let observed_started = observed_started.clone();
            let observed_release = observed_release.clone();
            async move {
                if call == 1 {
                    observed_started.notify_one();
                    observed_release.notified().await;
                }
                Ok(())
            }
        });

        let notifier = coordinator.notifier();
        notifier.request_rebuild();
        started.notified().await;
        notifier.request_rebuild();

        let shutdown = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.shutdown().await })
        };
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        release.notify_one();
        shutdown.await.unwrap();
        tokio::time::sleep(COALESCE_WINDOW + Duration::from_millis(25)).await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

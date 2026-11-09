//! Main-thread work boundary for global shortcut registration.
//!
//! This follows the pinned Nyanpasu ref: callers await a one-shot response,
//! never block the async hotkey actor, and are released when the UI event loop
//! can no longer execute their queued work.

use tokio_util::sync::CancellationToken;

pub type MainThreadTask = Box<dyn FnOnce() + Send + 'static>;

/// Work processed by a main-thread shutdown taker after the UI event loop
/// has stopped (for example, when the panic handler owns that thread).
pub enum MainThreadWork {
    Run(MainThreadTask),
    Done,
}

#[derive(Default)]
pub struct MainThreadHandoff {
    taker: parking_lot::Mutex<Option<std::sync::mpsc::Sender<MainThreadWork>>>,
    lost: CancellationToken,
}

impl MainThreadHandoff {
    pub fn take_over(
        &self,
    ) -> (
        std::sync::mpsc::Sender<MainThreadWork>,
        std::sync::mpsc::Receiver<MainThreadWork>,
    ) {
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.taker.lock() = Some(sender.clone());
        // Previously scheduled event-loop callbacks can no longer complete.
        self.lost.cancel();
        (sender, receiver)
    }

    pub fn hand_over(&self, task: MainThreadTask) -> Result<(), MainThreadTask> {
        match &*self.taker.lock() {
            Some(taker) => taker.send(MainThreadWork::Run(task)).map_err(|error| {
                let MainThreadWork::Run(task) = error.0 else {
                    unreachable!("only Run work is sent")
                };
                task
            }),
            None => Err(task),
        }
    }

    pub fn event_loop_lost(&self) -> CancellationToken {
        self.lost.clone()
    }

    /// Called when Tauri's run loop is over. Wake outstanding callers rather
    /// than leaving them awaiting an event loop which will never run again.
    pub fn mark_event_loop_lost(&self) {
        self.lost.cancel();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MainThreadError {
    #[error("the event loop refused the task: {source}")]
    HandOffToEventLoop {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("the event loop dropped the task before it finished: {source}")]
    AwaitTaskResult {
        #[source]
        source: tokio::sync::oneshot::error::RecvError,
    },
    #[error("the event loop stopped before the task finished")]
    EventLoopLost,
}

pub trait MainThreadExecutor: Send + Sync + 'static {
    fn execute(&self, task: MainThreadTask) -> Result<(), MainThreadError>;

    fn event_loop_lost(&self) -> CancellationToken {
        CancellationToken::new()
    }
}

impl dyn MainThreadExecutor {
    pub async fn run<T: Send + 'static>(
        &self,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, MainThreadError> {
        let lost = self.event_loop_lost();
        let (sender, result) = tokio::sync::oneshot::channel();
        self.execute(Box::new(move || {
            let _ = sender.send(task());
        }))?;
        tokio::select! {
            biased;
            result = result => result.map_err(|source| MainThreadError::AwaitTaskResult { source }),
            () = lost.cancelled() => Err(MainThreadError::EventLoopLost),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    struct Inline;
    impl MainThreadExecutor for Inline {
        fn execute(&self, task: MainThreadTask) -> Result<(), MainThreadError> {
            task();
            Ok(())
        }
    }

    struct Dropped;
    impl MainThreadExecutor for Dropped {
        fn execute(&self, task: MainThreadTask) -> Result<(), MainThreadError> {
            drop(task);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Stranded {
        queue: Mutex<Vec<MainThreadTask>>,
        lost: CancellationToken,
    }
    impl MainThreadExecutor for Stranded {
        fn execute(&self, task: MainThreadTask) -> Result<(), MainThreadError> {
            self.queue.lock().unwrap().push(task);
            Ok(())
        }
        fn event_loop_lost(&self) -> CancellationToken {
            self.lost.clone()
        }
    }

    #[tokio::test]
    async fn inline_main_thread_returns_the_actual_result() {
        let executor: Arc<dyn MainThreadExecutor> = Arc::new(Inline);
        assert_eq!(executor.run(|| 42).await.unwrap(), 42);
    }

    #[tokio::test]
    async fn a_dropped_callback_reports_an_error() {
        let executor: Arc<dyn MainThreadExecutor> = Arc::new(Dropped);
        assert!(matches!(
            executor.run(|| 42).await,
            Err(MainThreadError::AwaitTaskResult { .. })
        ));
    }

    #[tokio::test]
    async fn a_stalled_event_loop_does_not_hold_an_actor_forever() {
        let executor = Arc::new(Stranded::default());
        let run: Arc<dyn MainThreadExecutor> = executor.clone();
        let waiting = tokio::spawn(async move { run.run(|| 42).await });
        while executor.queue.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        executor.lost.cancel();
        assert!(matches!(
            waiting.await.unwrap(),
            Err(MainThreadError::EventLoopLost)
        ));
    }

    #[test]
    fn shutdown_handoff_takes_queued_tasks_without_running_them_early() {
        let handoff = MainThreadHandoff::default();
        let ran = Arc::new(AtomicBool::new(false));
        let marker = ran.clone();
        let task: MainThreadTask = Box::new(move || marker.store(true, Ordering::SeqCst));
        let task = handoff.hand_over(task).expect_err("no taker yet");
        let (_done, receiver) = handoff.take_over();
        assert!(handoff.event_loop_lost().is_cancelled());
        assert!(handoff.hand_over(task).is_ok(), "taker accepted task");
        let MainThreadWork::Run(task) = receiver.try_recv().unwrap() else {
            panic!("expected Run");
        };
        task();
        assert!(ran.load(Ordering::SeqCst));
    }
}

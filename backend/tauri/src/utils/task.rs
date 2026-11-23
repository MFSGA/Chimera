use std::future::Future;

use tokio_util::task::TaskTracker;

/// Starts a tracked task on Tauri's runtime from synchronous setup callbacks.
pub(crate) fn spawn_tracked_on_tauri_runtime(
    tasks: &TaskTracker,
    task: impl Future<Output = ()> + Send + 'static,
) {
    tasks.spawn_on(task, tauri::async_runtime::handle().inner());
}

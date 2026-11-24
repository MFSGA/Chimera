//! Ref CoreActor projection -> Chimera UI and runtime Effects bridge.
//!
//! The router is the sole owner of host/generation state. Its status pump may
//! publish an unchanged frame every two seconds; these notifications must NOT
//! cause periodic OS proxy mutations or gratuitous UI refreshes.

use chimera_core::client::core_events::{runtime_binding_changed, runtime_materially_changed};

use super::ChimeraClient;

impl ChimeraClient {
    /// Start one tracked CoreActor watch after the Client is fully composed.
    /// Call from inside the Tauri runtime: TaskTracker::spawn needs a reactor.
    /// The observer never owns or re-selects an endpoint and exits when the
    /// parent Client's shutdown token is canceled.
    pub(crate) fn start_core_status_observer(&self) -> bool {
        let Some(core) = &self.inner.core_api else {
            // Test-only legacy clients do not synthesize an extra actor.
            return true;
        };
        let mut updates = core.subscribe();
        let shutdown = self.shutdown_child_token();
        let client = self.clone();
        self.spawn_tracked(async move {
            let mut previous = updates.borrow_and_update().clone();
            loop {
                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break,
                    changed = updates.changed() => {
                        if changed.is_err() { break; }
                        let current = updates.borrow_and_update().clone();
                        let ui_changed = runtime_materially_changed(&previous, &current);
                        let binding_changed = runtime_binding_changed(&previous, &current);
                        previous = current.clone();
                        if shutdown.is_cancelled() {
                            break;
                        }
                        if ui_changed {
                            // UI state is a notification, not an independent
                            // poll or a cached authority. The typed Actor
                            // projection remains the authoritative state.
                            client.inner.ui_sink.core_status_changed(current.clone().into());
                            client.inner.ui_sink.refresh_clash();
                        }
                        if binding_changed {
                            #[cfg(not(any(target_os = "android", target_os = "ios")))]
                            client.publish_runtime_port_binding().await;
                        }
                    }
                }
            }
        })
    }
}

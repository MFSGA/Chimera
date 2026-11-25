//! Ref CoreActor projection -> Chimera UI and runtime Effects bridge.
//!
//! The router is the sole owner of host/generation state. Its status pump may
//! publish an unchanged frame every two seconds; these notifications must NOT
//! cause periodic OS proxy mutations or gratuitous UI refreshes.

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
        let updates = core.subscribe();
        let shutdown = self.shutdown_child_token();
        let client = self.clone();
        self.spawn_tracked(async move {
            chimera_core::client::core_events::observe_core_status(
                updates,
                shutdown,
                |current| {
                    client.inner.ui_sink.core_status_changed(current.into());
                    client.inner.ui_sink.refresh_clash();
                },
                || async {
                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                    client.publish_runtime_port_binding().await;
                },
            )
            .await;
        })
    }
}

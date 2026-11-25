//! The application lifecycle owned by the composed client.
//!
//! Follows ref/backend/nyanpasu-core/src/client/app_lifecycle.rs: the
//! successful client becomes the shutdown owner; partial startup remains
//! owned by the Tauri composition root.

use super::{ChimeraClient, application_workflow::startup::StartupReport};

impl ChimeraClient {
    /// Reconcile the committed runtime once per session, through the shared
    /// application workflow. Like ref, always return a startup report, even
    /// if the older Chimera CoreLifecycle transport loses its reply.
    pub(crate) async fn startup_reconcile(&self) -> StartupReport {
        let report = chimera_core::client::app_lifecycle::startup_report(
            self.inner.core_lifecycle.startup_reconcile().await,
        );
        // The ref actors, not LegacyCoreBridge, own the current host and
        // Service phase. Surface those typed projections while the
        // application-level startup report retains Chimera's extra details.
        if let Some(status) = self.routed_core_status() {
            tracing::debug!(
                host = ?status.host,
                generation = status.generation,
                connectivity = ?status.connectivity,
                "ref Core Actor startup projection"
            );
        }
        if let Some(status) = self.service_actor_status() {
            tracing::debug!(phase = ?status.phase, "ref Service Actor startup projection");
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        self.publish_full_effects().await;

        report
    }

    /// Start subscription refreshes, file watchers and journal recovery only
    /// after the one-shot startup reconcile has returned, even if it reports
    /// an unsettled runtime. Updates queue through the lifecycle actor.
    ///
    /// Chimera's Profiles actor currently uses an async RPC for this release,
    /// whereas ref's ProfilesClient accepts the command synchronously.
    pub(crate) async fn start_background_sources(&self) -> anyhow::Result<()> {
        chimera_core::client::app_lifecycle::start_background_sources(
            self.inner.typed_profiles.as_ref(),
        )
        .await
    }

    /// Stops admission to all client-owned tasks. Repeated requests are safe.
    pub(crate) fn request_shutdown(&self) {
        chimera_core::client::app_lifecycle::request_shutdown(
            &self.inner.shutdown,
            &self.inner.tasks,
        );
    }

    /// Drains every client owner before stopping the core control plane.
    /// Unlike ref, retain the fallible shutdown result: Chimera's updater must
    /// not install a new binary if core shutdown is unconfirmed.
    pub(crate) async fn wait_shutdown(&self) -> anyhow::Result<()> {
        chimera_core::client::app_lifecycle::wait_shutdown(
            &self.inner.tasks,
            self.inner.core.as_ref(),
            self.shutdown_core(),
            || self.routed_core_status(),
        )
        .await
    }
}

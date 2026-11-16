//! The application lifecycle owned by the composed client.
//!
//! Follows ref/backend/nyanpasu-core/src/client/app_lifecycle.rs: the
//! successful client becomes the shutdown owner; partial startup remains
//! owned by the Tauri composition root.

use super::{
    ChimeraClient,
    application_workflow::startup::{StartupOutcome, StartupReport},
};

impl ChimeraClient {
    /// Reconcile the committed runtime once per session, through the shared
    /// application workflow. Like ref, always return a startup report, even
    /// if the older Chimera CoreLifecycle transport loses its reply.
    pub(crate) async fn startup_reconcile(&self) -> StartupReport {
        let report = match self.inner.core_lifecycle.startup_reconcile().await {
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
        if let Some(profiles) = &self.inner.typed_profiles {
            profiles.start_producers().await?;
        }
        Ok(())
    }

    /// Stops admission to all client-owned tasks. Repeated requests are safe.
    pub(crate) fn request_shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tasks.close();
    }

    /// Drains every client owner before stopping the core control plane.
    /// Unlike ref, retain the fallible shutdown result: Chimera's updater must
    /// not install a new binary if core shutdown is unconfirmed.
    pub(crate) async fn wait_shutdown(&self) -> anyhow::Result<()> {
        self.inner.tasks.wait().await;
        self.shutdown_core().await
    }
}

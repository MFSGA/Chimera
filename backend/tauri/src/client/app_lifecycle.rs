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
        let stopped = self.shutdown_core().await;
        // Always drain the router and local executor, even when ordinary
        // stop has failed. Preserve the original stop uncertainty as the
        // error returned to the updater/exit boundary.
        let drained = self.inner.core.finish_shutdown().await;
        // A compatibility adapter that claimed to finish shutdown but left
        // CoreActor connected cannot be treated as an updater-safe stop.
        // In tests constructed without ref actors this check is skipped.
        let actor_closed = self.routed_core_status().map(|status| {
            anyhow::ensure!(
                matches!(
                    status.connectivity,
                    chimera_core::actor_v2::router::EndpointConnectivity::ShutDown
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
}

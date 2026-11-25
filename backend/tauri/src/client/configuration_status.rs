//! Desktop adapter of the shared ref configuration projection.
use super::ChimeraClient;
use chimera_core::client::{
    configuration::{ConfigurationStatus, SourceVersions},
    configuration_status::configuration_status,
};

impl ChimeraClient {
    pub(crate) fn configuration_status(&self) -> ConfigurationStatus {
        let status = self.inner.core_lifecycle.reconciler_status();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = self.inner.effects.snapshot();
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let effects = EffectsSnapshot::default();
        let sources = self
            .inner
            .typed_profiles
            .as_ref()
            .map(|profiles| profiles.sources())
            .unwrap_or_default();
        let source_versions = SourceVersions {
            application: self.inner.application.snapshot().version,
            clash: self.inner.clash_config.snapshot().version,
            session: self.inner.session_state.snapshot().version,
            profiles: self
                .inner
                .typed_profiles
                .as_ref()
                .map(|profiles| profiles.snapshot().revision())
                .unwrap_or_default(),
        };
        configuration_status(&status, &effects, sources, source_versions)
    }

    pub(crate) async fn retry_configuration_runtime(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.inner.shutdown.is_cancelled(),
            "client is shutting down"
        );
        self.inner.core_lifecycle.reconcile().await
    }

    pub(crate) fn retry_configuration_effect(
        &self,
        kind: chimera_core::client::configuration::EffectKind,
    ) -> anyhow::Result<()> {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            self.inner.effects.retry_now(kind)
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = kind;
            anyhow::bail!("desktop effects are unavailable")
        }
    }

    pub(crate) fn start_configuration_status_observer(&self) -> bool {
        let Some(reconciler) = self.inner.core_lifecycle.subscribe_reconciler() else {
            return true;
        };
        let Some(profiles) = self.inner.typed_profiles.as_ref() else {
            return true;
        };
        let sources = profiles.subscribe_sources();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = Some(self.inner.effects.subscribe());
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let effects: Option<tokio::sync::watch::Receiver<EffectsSnapshot>> = None;
        let shutdown = self.shutdown_child_token();
        let client = self.clone();
        self.spawn_tracked(async move {
            chimera_core::client::configuration_status::observe_configuration_status(
                reconciler,
                sources,
                effects,
                shutdown,
                || client.configuration_status(),
                |summary| {
                    client
                        .inner
                        .ui_sink
                        .configuration_status_summary_changed(summary)
                },
            )
            .await;
        })
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
use chimera_core::client::effects::actor::EffectsSnapshot;

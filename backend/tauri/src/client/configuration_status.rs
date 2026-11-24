//! Desktop adapter of the shared ref configuration projection.
use super::ChimeraClient;
use chimera_core::client::{
    configuration::{ConfigurationStatus, SourceVersions},
    configuration_observation::ConfigurationStatusSummary,
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
        let Some(mut reconciler) = self.inner.core_lifecycle.subscribe_reconciler() else {
            return true;
        };
        let Some(profiles) = self.inner.typed_profiles.as_ref() else {
            return true;
        };
        let mut sources = profiles.subscribe_sources();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let mut effects = Some(self.inner.effects.subscribe());
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let mut effects: Option<tokio::sync::watch::Receiver<EffectsSnapshot>> = None;
        let shutdown = self.shutdown_child_token();
        let client = self.clone();
        self.spawn_tracked(async move {
            // Consume initial values before listening; the first emitted
            // frame is a complete snapshot, not a partially populated delta.
            reconciler.borrow_and_update();
            sources.borrow_and_update();
            if let Some(effects) = effects.as_mut() {
                effects.borrow_and_update();
            }
            client.inner.ui_sink.configuration_status_summary_changed(
                ConfigurationStatusSummary::from(&client.configuration_status()),
            );
            loop {
                let changed = tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break,
                    result = reconciler.changed() => result,
                    result = sources.changed() => result,
                    result = async {
                        match effects.as_mut() {
                            Some(effects) => effects.changed().await,
                            None => std::future::pending().await,
                        }
                    } => result,
                };
                if changed.is_err() || shutdown.is_cancelled() {
                    break;
                }
                let status = client.configuration_status();
                // Ordinary windows get only the closed, diagnostic-free summary.
                client.inner.ui_sink.configuration_status_summary_changed(
                    ConfigurationStatusSummary::from(&status),
                );
            }
        })
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
use chimera_core::client::effects::actor::EffectsSnapshot;

//! Configuration mutation port for the composed host. The host retains its
//! existing transaction, mirror persistence and effect synchronization.
use chimera_config::{
    application::ChimeraAppConfigPatch,
    clash::config::{
        ClashConfigPatch,
        overrides::{ClashGuardOverridesPatch, Mode},
    },
};
use chimera_contracts::runtime::MutationOutcome;

#[async_trait::async_trait]
pub trait ConfigurationMutations: Send + Sync {
    async fn patch_application(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<MutationOutcome<()>>;
    async fn patch_clash(&self, patch: ClashConfigPatch) -> anyhow::Result<MutationOutcome<()>>;
    async fn patch_overrides(
        &self,
        patch: chimera_config::clash::config::overrides::ClashGuardOverridesPatch,
    ) -> anyhow::Result<MutationOutcome<()>>;
}

impl super::ChimeraClient {
    /// Ordinary preferences and system settings share the committed owner.
    pub async fn patch_app_config(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        self.configuration_mutations()?
            .patch_application(patch)
            .await
    }

    /// Retain host authorization and transactional runtime validation.
    pub async fn patch_clash_config(
        &self,
        patch: ClashConfigPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        self.configuration_mutations()?.patch_clash(patch).await
    }

    /// Update typed overrides, then refresh their dependent observations.
    pub async fn patch_runtime_overrides(
        &self,
        patch: ClashGuardOverridesPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        anyhow::ensure!(!self.shutdown().is_cancelled(), "client is shutting down");
        let current = self.get_clash_config()?;
        let outcome = if patch
            .mode
            .is_some_and(|mode| mode == current.overrides.mode())
            && {
                use struct_patch::Status as _;
                let mut remaining = patch.clone();
                remaining.mode = None;
                remaining.is_empty()
            } {
            let failure = self.runtime_reconciler()?.interrupt_connections().await?;
            MutationOutcome::from_parts((), failure.into_iter().collect())
        } else {
            self.configuration_mutations()?
                .patch_overrides(patch)
                .await?
        };
        self.refresh_stream_log_level().await;
        self.request_proxy_refresh();
        Ok(outcome)
    }

    /// A proxy-mode permission cannot write unrelated overrides.
    pub async fn set_proxy_mode(&self, mode: Mode) -> anyhow::Result<MutationOutcome<()>> {
        self.patch_runtime_overrides(mode.into()).await
    }

    pub async fn refresh_stream_log_level(&self) {
        match self.get_clash_config() {
            Ok(config) => {
                if let Err(error) = self
                    .streams()
                    .set_log_level(config.overrides.log_level())
                    .await
                {
                    tracing::warn!(%error, "failed to update the Clash logs WebSocket level");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "failed to read Clash log level after config update");
            }
        }
    }
}

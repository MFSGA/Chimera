//! Configuration mutation port for the composed host. The host retains its
//! existing transaction, mirror persistence and effect synchronization.
use chimera_config::{application::ChimeraAppConfigPatch, clash::config::ClashConfigPatch};
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

use std::sync::Arc;

use super::{
    super::{application::ApplicationClient, runtime::RuntimePaths},
    ports::{BinaryInstaller, CoreLifecyclePort, PreparedCoreBinary},
};
use crate::config::chimera::ClashCore;

pub(super) enum Command {
    StopCore,
    SelectCore(ClashCore),
    RecoverCore,
    ReplaceCoreBinary(PreparedCoreBinary),
}

/// Serialized lifecycle command workflow.
///
/// The lower-level port remains the compatibility boundary for the legacy
/// CoreManager while command admission moves toward REF's actor-owned
/// core-lifecycle model.
pub(super) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    core: Arc<dyn CoreLifecyclePort>,
    installer: Arc<dyn BinaryInstaller>,
    runtime_paths: RuntimePaths,
}

impl CoreLifecycleWorkflow {
    pub(super) fn new(
        application: ApplicationClient,
        core: Arc<dyn CoreLifecyclePort>,
        installer: Arc<dyn BinaryInstaller>,
        runtime_paths: RuntimePaths,
    ) -> Self {
        Self {
            application,
            core,
            installer,
            runtime_paths,
        }
    }

    pub(super) async fn execute(&self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::RecoverCore => self.core.recover().await,
            Command::StopCore => {
                let mut lease = self.core.begin().await?;
                lease.stop().await
            }
            Command::SelectCore(core) => {
                let mut lease = self.core.begin().await?;
                lease.change_core(core).await
            }
            Command::ReplaceCoreBinary(artifact) => self.replace_binary(artifact).await,
        }
    }

    async fn replace_binary(&self, artifact: PreparedCoreBinary) -> anyhow::Result<()> {
        let current_core =
            crate::bridge::verge::legacy_core_from_typed(self.application.get_typed().core);
        let current_run_type = self.core.status().await?.run_type;

        if current_core == artifact.target {
            let mut lease = self.core.begin().await?;
            lease.stop().await?;
            self.installer.install(&artifact).await?;
            artifact.progress.restarting();
            lease
                .run_core_from(self.runtime_paths.product(), current_core, current_run_type)
                .await
        } else {
            self.installer.install(&artifact).await
        }
    }
}

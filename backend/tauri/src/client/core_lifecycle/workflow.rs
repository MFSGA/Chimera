use std::sync::Arc;

use crate::config::chimera::ClashCore;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    ports::{BinaryInstaller, CoreLifecyclePort, PreparedCoreBinary},
};

pub(super) enum Command {
    StopCore,
    SelectCore(ClashCore),
    RecoverCore,
    Reconcile,
    ReplaceCoreBinary(PreparedCoreBinary),
}

/// Serialize lifecycle mutations while execution remains behind the legacy port.
pub(super) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    clash: ClashConfigClient,
    core: Arc<dyn CoreLifecyclePort>,
    installer: Arc<dyn BinaryInstaller>,
    runtime_paths: RuntimePaths,
}

impl CoreLifecycleWorkflow {
    pub(super) fn new(
        application: ApplicationClient,
        clash: ClashConfigClient,
        core: Arc<dyn CoreLifecyclePort>,
        installer: Arc<dyn BinaryInstaller>,
        runtime_paths: RuntimePaths,
    ) -> Self {
        Self {
            application,
            clash,
            core,
            installer,
            runtime_paths,
        }
    }

    pub(super) async fn execute(&self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::RecoverCore => self.core.recover().await,
            Command::Reconcile => self.reconcile().await,
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

    async fn reconcile(&self) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let target_core = crate::bridge::verge::legacy_core_from_typed(app.core);
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        let mut lease = self.core.begin().await?;
        lease
            .rebuild_running_config(clash, target_core, run_type)
            .await
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

use std::sync::Arc;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    ports::{BinaryInstaller, CoreLifecyclePort, PreparedCoreBinary, ServiceLifecyclePort},
};
use crate::config::chimera::ClashCore;

pub(super) enum Command {
    StopCore,
    SelectCore(ClashCore),
    RecoverCore,
    Reconcile,
    ReplaceCoreBinary(PreparedCoreBinary),
    InstallService,
    UninstallService,
    UpdateService,
    StartService,
    RestartService,
    StopService,
}

/// Serialized lifecycle command workflow.
///
/// The lower-level port remains the compatibility boundary for the legacy
/// CoreManager while command admission moves toward REF's actor-owned
/// core-lifecycle model.
pub(super) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    clash: ClashConfigClient,
    core: Arc<dyn CoreLifecyclePort>,
    installer: Arc<dyn BinaryInstaller>,
    runtime_paths: RuntimePaths,
    service: Arc<dyn ServiceLifecyclePort>,
}

impl CoreLifecycleWorkflow {
    pub(super) fn new(
        application: ApplicationClient,
        clash: ClashConfigClient,
        core: Arc<dyn CoreLifecyclePort>,
        installer: Arc<dyn BinaryInstaller>,
        runtime_paths: RuntimePaths,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> Self {
        Self {
            application,
            clash,
            core,
            installer,
            runtime_paths,
            service,
        }
    }

    pub(super) async fn probe_service(
        &self,
    ) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.service.probe().await
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
            Command::InstallService => self.install_service().await,
            Command::UninstallService => self.uninstall_service().await,
            Command::UpdateService => self.update_service().await,
            Command::StartService => self.start_service(false).await,
            Command::RestartService => self.start_service(true).await,
            Command::StopService => self.stop_service().await,
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

    async fn install_service(&self) -> anyhow::Result<()> {
        let mut transition = self.service.begin_transition().await?;
        transition.install_daemon().await
    }

    async fn uninstall_service(&self) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

        let mut transition = self.service.begin_transition().await?;
        transition.uninstall_daemon().await?;
        transition.confirm_stopped().await?;

        if !self.application.get_typed().enable_service_mode {
            return Ok(());
        }

        let before = self.core.status().await?;
        if !matches!(before.state, CoreState::Running)
            || before.run_type == crate::core::RunType::Service
        {
            self.reconcile().await?;
        }

        let after = self.core.status().await?;
        anyhow::ensure!(
            matches!(after.state, CoreState::Running)
                && after.run_type != crate::core::RunType::Service,
            "core did not recover to the local host after Service uninstall"
        );
        Ok(())
    }

    async fn update_service(&self) -> anyhow::Result<()> {
        let mut transition = self.service.begin_transition().await?;
        transition.update_daemon().await
    }

    async fn start_service(&self, restart: bool) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

        let mut transition = self.service.begin_transition().await?;
        if restart {
            transition.restart_daemon().await?;
        } else {
            transition.start_daemon().await?;
        }

        if !self.application.get_typed().enable_service_mode {
            return Ok(());
        }

        transition
            .confirm_ready(std::time::Duration::from_secs(8))
            .await?;
        let before = self.core.status().await?;
        if !matches!(before.state, CoreState::Running)
            || before.run_type != crate::core::RunType::Service
        {
            self.reconcile().await?;
        }

        transition
            .confirm_ready(std::time::Duration::from_secs(5))
            .await?;
        let after = self.core.status().await?;
        anyhow::ensure!(
            matches!(after.state, CoreState::Running)
                && after.run_type == crate::core::RunType::Service,
            "core did not reach the Chimera Service host"
        );
        Ok(())
    }

    async fn stop_service(&self) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

        let mut transition = self.service.begin_transition().await?;
        transition.stop_daemon().await?;
        transition.confirm_stopped().await?;

        if !self.application.get_typed().enable_service_mode {
            return Ok(());
        }

        let before = self.core.status().await?;
        if !matches!(before.state, CoreState::Running)
            || before.run_type == crate::core::RunType::Service
        {
            self.reconcile().await?;
        }

        let after = self.core.status().await?;
        anyhow::ensure!(
            matches!(after.state, CoreState::Running)
                && after.run_type != crate::core::RunType::Service,
            "core did not recover to the local host after Service stop"
        );
        Ok(())
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

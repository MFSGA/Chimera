use std::sync::Arc;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient,
        profiles::ProfilesReadPort, runtime::RuntimePaths,
    },
    ports::{BinaryInstaller, CoreLifecyclePort, PreparedCoreBinary, ServiceLifecyclePort},
};
use crate::config::chimera::ClashCore;

pub(super) enum Command {
    Shutdown,
    #[cfg(test)]
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
    ServiceEndpointDown,
}

/// Serialized lifecycle command workflow.
///
/// The lower-level port remains the compatibility boundary for Chimera's
/// staged `core::actor_v2::CoreFacade` while command admission follows REF's
/// actor-owned core-lifecycle model.
pub(super) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    clash: ClashConfigClient,
    core: Arc<dyn CoreLifecyclePort>,
    profiles: Arc<dyn ProfilesReadPort>,
    installer: Arc<dyn BinaryInstaller>,
    service: Arc<dyn ServiceLifecyclePort>,
}

impl CoreLifecycleWorkflow {
    pub(super) fn new(
        application: ApplicationClient,
        clash: ClashConfigClient,
        core: Arc<dyn CoreLifecyclePort>,
        profiles: Arc<dyn ProfilesReadPort>,
        installer: Arc<dyn BinaryInstaller>,
        _runtime_paths: RuntimePaths,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> Self {
        Self {
            application,
            clash,
            core,
            profiles,
            installer,
            service,
        }
    }

    pub(super) fn outcome_uncertain(&self) -> bool {
        self.core.outcome_uncertain()
    }

    pub(super) async fn execute(&self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::RecoverCore | Command::Reconcile => self.reconcile().await,
            Command::Shutdown => self.core.stop().await,
            #[cfg(test)]
            Command::StopCore => self.core.stop().await,
            Command::SelectCore(core) => {
                self.core.change_core(self.profiles.snapshot()?, core).await
            }
            Command::ReplaceCoreBinary(artifact) => self.replace_binary(artifact).await,
            Command::InstallService => self.install_service().await,
            Command::UninstallService => self.uninstall_service().await,
            Command::UpdateService => self.update_service().await,
            Command::StartService => self.start_service(false).await,
            Command::RestartService => self.start_service(true).await,
            Command::StopService => self.stop_service().await,
            Command::ServiceEndpointDown => self.service.report_endpoint_down().await,
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
        let profiles = self.profiles.snapshot()?;
        self.core
            .reconcile(clash, profiles, target_core, run_type)
            .await
    }

    async fn install_service(&self) -> anyhow::Result<()> {
        let mut transition = self.service.begin_transition().await?;
        transition.install_daemon().await
    }

    async fn uninstall_service(&self) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

        let mut transition = self.service.begin_transition().await?;
        let before = self.core.status().await?;

        if matches!(before.state, CoreState::Running)
            && before.run_type == crate::core::RunType::Service
        {
            transition.stop_daemon().await?;
            transition.confirm_stopped().await?;
            self.reconcile().await?;

            let handed_off = self.core.status().await?;
            anyhow::ensure!(
                matches!(handed_off.state, CoreState::Running)
                    && handed_off.run_type != crate::core::RunType::Service,
                "core did not hand off to the local host before Service uninstall"
            );
        }

        transition.uninstall_daemon().await?;
        transition.confirm_stopped().await?;

        if !self.application.get_typed().enable_service_mode {
            return Ok(());
        }

        let after_uninstall = self.core.status().await?;
        if !matches!(after_uninstall.state, CoreState::Running)
            || after_uninstall.run_type == crate::core::RunType::Service
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
        if current_core == artifact.target {
            self.core.stop().await?;
            self.installer.install(&artifact).await?;
            artifact.progress.restarting();
            self.reconcile().await
        } else {
            self.installer.install(&artifact).await
        }
    }
}

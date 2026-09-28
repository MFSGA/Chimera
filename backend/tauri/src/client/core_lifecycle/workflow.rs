use std::sync::Arc;

use crate::client::application_workflow::workflow::ProfileRuntime;
use crate::config::chimera::ClashCore;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    ports::{
        BinaryInstaller, CoreLifecyclePort, PreparedCoreBinary, ServiceLifecyclePort,
        ServiceTransitionLease,
    },
};

pub(super) enum Command {
    ProfileMutation(Box<crate::client::application_workflow::mutation::MutationRequest>),
    Shutdown,
    #[cfg(test)]
    StopCore,
    SelectCore(ClashCore),
    RecoverCore,
    Reconcile,
    ReconcileProfiles {
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    },
    ReplaceCoreBinary(PreparedCoreBinary),
    InstallService(Box<dyn ServiceTransitionLease>),
    UninstallService(Box<dyn ServiceTransitionLease>),
    UpdateService(Box<dyn ServiceTransitionLease>),
    StartService {
        transition: Box<dyn ServiceTransitionLease>,
        ready_timeout: std::time::Duration,
    },
    RestartService {
        transition: Box<dyn ServiceTransitionLease>,
        ready_timeout: std::time::Duration,
    },
    StopService(Box<dyn ServiceTransitionLease>),
    ServiceEndpointDown(Box<dyn ServiceTransitionLease>),
}

/// The lower-level port remains the compatibility boundary for Chimera's
/// staged `core::actor_v2::CoreFacade` while command admission follows REF's
/// actor-owned core-lifecycle model.
pub(super) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    clash: ClashConfigClient,
    profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
    core: Arc<dyn CoreLifecyclePort>,
    installer: Arc<dyn BinaryInstaller>,
    service: Arc<dyn ServiceLifecyclePort>,
}

impl CoreLifecycleWorkflow {
    pub(super) fn new(
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
        core: Arc<dyn CoreLifecyclePort>,
        installer: Arc<dyn BinaryInstaller>,
        _runtime_paths: RuntimePaths,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> Self {
        Self {
            application,
            clash,
            profiles,
            core,
            installer,
            service,
        }
    }

    pub(super) async fn probe_service(
        &self,
    ) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.service.probe().await
    }

    pub(super) fn outcome_uncertain(&self) -> bool {
        self.core.outcome_uncertain()
    }

    pub(super) fn service_restart_policy(
        &self,
    ) -> crate::core::actor_v2::facade::ServiceRestartPolicySnapshot {
        self.service.restart_policy()
    }

    pub(super) async fn execute(&self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::ProfileMutation(_) => {
                anyhow::bail!("Profile mutations must run through ApplicationWorkflowActor")
            }
            Command::RecoverCore | Command::Reconcile => self.reconcile().await,
            Command::ReconcileProfiles {
                profiles,
                staged_content,
            } => self.reconcile_profiles(profiles, staged_content).await,
            Command::Shutdown => self.core.stop().await,
            #[cfg(test)]
            Command::StopCore => self.core.stop().await,
            Command::SelectCore(core) => self.select_core(core).await,
            Command::ReplaceCoreBinary(artifact) => self.replace_binary(artifact).await,
            Command::InstallService(mut transition) => transition.install_daemon().await,
            Command::UninstallService(mut transition) => {
                self.uninstall_service(transition.as_mut()).await
            }
            Command::UpdateService(mut transition) => transition.update_daemon().await,
            Command::StartService {
                mut transition,
                ready_timeout,
            } => {
                self.start_service(transition.as_mut(), false, ready_timeout)
                    .await
            }
            Command::RestartService {
                mut transition,
                ready_timeout,
            } => {
                self.start_service(transition.as_mut(), true, ready_timeout)
                    .await
            }
            Command::StopService(mut transition) => self.stop_service(transition.as_mut()).await,
            Command::ServiceEndpointDown(mut transition) => {
                self.service.report_endpoint_down(transition.as_mut()).await
            }
        }
    }

    async fn uninstall_service(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

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

    async fn start_service(
        &self,
        transition: &mut dyn ServiceTransitionLease,
        restart: bool,
        ready_timeout: std::time::Duration,
    ) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

        if restart {
            transition.restart_daemon().await?;
        } else {
            transition.start_daemon().await?;
        }
        if !self.application.get_typed().enable_service_mode {
            return Ok(());
        }

        transition.confirm_ready(ready_timeout).await?;
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

    async fn stop_service(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

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

    async fn reconcile(&self) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let target_core = crate::bridge::verge::legacy_core_from_typed(app.core);
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        if let Some(profiles) = &self.profiles {
            self.core
                .reconcile_profiles(
                    clash,
                    target_core,
                    run_type,
                    Arc::new(profiles.load().state.clone()),
                    app,
                    Default::default(),
                )
                .await
        } else {
            self.core.reconcile(clash, target_core, run_type).await
        }
    }

    async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let target_core = crate::bridge::verge::legacy_core_from_typed(app.core);
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .reconcile_profiles(clash, target_core, run_type, profiles, app, staged_content)
            .await
    }

    async fn select_core(&self, core: ClashCore) -> anyhow::Result<()> {
        let Some(profiles) = &self.profiles else {
            return self.core.change_core(core).await;
        };

        let previous_app = self.application.get_typed();
        let mut candidate_app = previous_app.clone();
        candidate_app.core = crate::bridge::verge::typed_core_from_legacy(core);
        let profile_snapshot = Arc::new(profiles.load().state.clone());
        let clash = self.clash.get()?;
        let run_type = crate::core::RunType::classify(
            candidate_app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );

        self.core
            .reconcile_profiles(
                clash.clone(),
                core,
                run_type,
                profile_snapshot.clone(),
                candidate_app.clone(),
                Default::default(),
            )
            .await?;

        if let Err(error) = self.application.patch_core(candidate_app.core).await {
            let previous_core = crate::bridge::verge::legacy_core_from_typed(previous_app.core);
            let rollback = self
                .core
                .reconcile_profiles(
                    clash,
                    previous_core,
                    run_type,
                    profile_snapshot,
                    previous_app,
                    Default::default(),
                )
                .await;
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(anyhow::anyhow!(
                    "failed to persist selected core: {error}; failed to restore the previous Profile runtime: {rollback_error}"
                )),
            };
        }
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

#[async_trait::async_trait]
impl ProfileRuntime for CoreLifecycleWorkflow {
    fn status(&self) -> crate::client::core_lifecycle::CoreLifecycleStatus {
        crate::client::core_lifecycle::CoreLifecycleStatus {
            uncertain: self.core.outcome_uncertain(),
            ..Default::default()
        }
    }

    async fn core_status(&self) -> anyhow::Result<super::ports::CoreStatusSnapshot> {
        self.core.status().await
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        self.core.observe_runtime_baseline().await
    }

    async fn restore_runtime_baseline(
        &self,
        baseline: &crate::client::application_workflow::mutation::KnownRuntimeState,
    ) -> anyhow::Result<()> {
        self.core.restore_runtime_baseline(baseline).await
    }

    async fn validate_profile_runtime(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let target_core = crate::bridge::verge::legacy_core_from_typed(app.core);
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .validate_profile_runtime(clash, target_core, run_type, profiles, app, staged_content)
            .await
    }

    async fn prepare_profile_runtime(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
        operation_id: &chimera_core_manager::OperationId,
    ) -> anyhow::Result<(
        crate::client::application_workflow::mutation::AppliedCandidate,
        crate::client::application_workflow::mutation::CheckRecord,
    )> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let target_core = crate::bridge::verge::legacy_core_from_typed(app.core);
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .prepare_profile_runtime(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                staged_content,
                operation_id.clone(),
            )
            .await
    }

    async fn confirm_profile_runtime(
        &self,
        operation_id: &chimera_core_manager::OperationId,
        candidate: crate::client::application_workflow::mutation::AppliedCandidate,
    ) -> anyhow::Result<()> {
        self.core
            .confirm_profile_runtime(operation_id.clone(), candidate)
            .await
    }

    async fn discard_profile_runtime(&self, operation_id: &chimera_core_manager::OperationId) {
        self.core
            .discard_profile_runtime(operation_id.clone())
            .await
    }

    async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        CoreLifecycleWorkflow::reconcile_profiles(self, profiles, staged_content).await
    }

    fn request_runtime_rebuild(&self) {
        // The owning ApplicationWorkflowActor schedules dirty work after this
        // mutation releases the serialized execution domain.
    }
}

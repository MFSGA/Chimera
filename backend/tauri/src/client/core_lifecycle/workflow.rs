use std::sync::Arc;

use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use struct_patch::Patch;

use crate::client::application_workflow::workflow::ProfileRuntime;
use crate::config::chimera::ClashCore;
use crate::enhance::FsProfileContentSource;

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
    PatchApplicationConfig {
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    },
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    DisableStaleSystemProxy {
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: crate::client::system_proxy::ports::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    },
    SelectCore(ClashCore),
    RecoverCore,
    Reconcile,
    RetryDeferredProfiles {
        digest: String,
    },
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

    pub(super) async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        let receipt = match self.core.active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::debug!(%error, "no active runtime receipt is available after lifecycle work");
                return None;
            }
        };
        crate::client::application::resolved_effect_port_binding(receipt.as_deref())
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: crate::client::system_proxy::ports::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        use chimera_ipc::api::status::CoreState;

        let status = self.core.status().await.map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
        })?;
        if !matches!(status.state, CoreState::Stopped(_))
            || status.state_changed_at != expected_core_state_changed_at
        {
            return Err(crate::client::system_proxy::SystemProxyActionError::StateChanged);
        }
        let clash = self.clash.get().map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
        })?;
        let receipt = self.core.last_confirmed_runtime_receipt();
        let effective_mixed_port =
            crate::client::ports::effective_mixed_port(receipt.as_deref(), &clash);
        if effective_mixed_port != Some(expected_port) {
            return Err(crate::client::system_proxy::SystemProxyActionError::StateChanged);
        }
        let previous = self.application.get().await.map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
        })?;
        if previous.state.enable_system_proxy != desired_before {
            return Err(crate::client::system_proxy::SystemProxyActionError::StateChanged);
        }

        if let Err(error) = system_proxy.disable_stale_proxy(expected_port).await {
            return Err(self
                .recover_stale_proxy_action(
                    &system_proxy,
                    original,
                    desired_before,
                    notifications.as_ref(),
                    error,
                )
                .await);
        }
        if !desired_before {
            notifications.runtime_bound(None, false);
            return Ok(());
        }

        let mut candidate = previous.state.clone();
        candidate.enable_system_proxy = false;
        let clash = match self.clash.get() {
            Ok(clash) => clash,
            Err(error) => {
                return Err(self
                    .recover_stale_proxy_action(
                        &system_proxy,
                        original,
                        desired_before,
                        notifications.as_ref(),
                        crate::client::system_proxy::SystemProxyActionError::Failed(
                            error.to_string(),
                        ),
                    )
                    .await);
            }
        };
        let prepared = match self.application.prepare_replace(candidate.clone()).await {
            Ok(prepared) => prepared,
            Err(error) => {
                return Err(self
                    .recover_stale_proxy_action(
                        &system_proxy,
                        original,
                        desired_before,
                        notifications.as_ref(),
                        crate::client::system_proxy::SystemProxyActionError::Failed(
                            error.to_string(),
                        ),
                    )
                    .await);
            }
        };
        match self
            .application
            .replace_prepared_if_version(previous.version, prepared)
            .await
        {
            Ok(crate::state::ConditionalReplaceResult::Replaced(_)) => {
                let persist_error = persist_legacy_mirror().err();
                let fields = crate::client::effects::plan::ApplicationEffectInputs::project(
                    &candidate, &clash, None,
                )
                .app;
                notifications.application_committed(
                    fields,
                    vec![crate::client::effects::plan::EffectKind::SystemProxy],
                );
                notifications.runtime_bound(None, false);
                match persist_error {
                    Some(error) => Err(
                        crate::client::system_proxy::SystemProxyActionError::PartialApply(format!(
                            "the proxy preference was committed, but the legacy mirror could not be persisted: {error:#}"
                        )),
                    ),
                    None => Ok(()),
                }
            }
            Ok(crate::state::ConditionalReplaceResult::Conflict { .. }) => Err(self
                .publish_latest_proxy_preference(notifications.as_ref())
                .await
                .err()
                .unwrap_or(crate::client::system_proxy::SystemProxyActionError::StateChanged)),
            Err(error) => Err(self
                .recover_stale_proxy_action(
                    &system_proxy,
                    original,
                    desired_before,
                    notifications.as_ref(),
                    crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string()),
                )
                .await),
        }
    }

    async fn publish_latest_proxy_preference(
        &self,
        notifications: &dyn crate::client::effects::ports::CommitNotifications,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        let latest = self.application.get().await.map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::PartialApply(format!(
                "could not read the latest application preference: {error:#}"
            ))
        })?;
        let clash = self.clash.get().map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::PartialApply(error.to_string())
        })?;
        let fields = crate::client::effects::plan::ApplicationEffectInputs::project(
            &latest.state,
            &clash,
            None,
        )
        .app;
        notifications.application_committed(
            fields,
            vec![crate::client::effects::plan::EffectKind::SystemProxy],
        );
        notifications.runtime_bound(None, false);
        Ok(())
    }

    async fn recover_stale_proxy_action(
        &self,
        system_proxy: &crate::client::system_proxy::SystemProxyClient,
        original: crate::client::system_proxy::ports::OsProxyConfig,
        desired_before: bool,
        notifications: &dyn crate::client::effects::ports::CommitNotifications,
        error: crate::client::system_proxy::SystemProxyActionError,
    ) -> crate::client::system_proxy::SystemProxyActionError {
        match self.application.get().await {
            Ok(latest) if latest.state.enable_system_proxy != desired_before => {
                match self.publish_latest_proxy_preference(notifications).await {
                    Ok(()) => crate::client::system_proxy::SystemProxyActionError::StateChanged,
                    Err(reconcile_error) => reconcile_error,
                }
            }
            Ok(_) => rollback_stale_proxy_action(system_proxy, original, error.to_string()).await,
            Err(read_error) => {
                crate::client::system_proxy::SystemProxyActionError::PartialApply(format!(
                    "{error}; could not determine the latest preference before rollback: {read_error:#}"
                ))
            }
        }
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
            Command::RetryDeferredProfiles { digest } => {
                self.retry_deferred_profiles(&digest).await
            }
            Command::ReconcileProfiles {
                profiles,
                staged_content,
            } => self.reconcile_profiles(profiles, staged_content).await,
            Command::Shutdown => self.core.stop().await,
            #[cfg(test)]
            Command::StopCore => self.core.stop().await,
            Command::PatchApplicationConfig {
                patch,
                service_connected,
            } => {
                self.patch_application_config(patch, service_connected)
                    .await
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Command::DisableStaleSystemProxy {
                system_proxy,
                expected_port,
                expected_core_state_changed_at,
                desired_before,
                original,
                notifications,
                persist_legacy_mirror,
            } => self
                .disable_stale_system_proxy(
                    system_proxy,
                    expected_port,
                    expected_core_state_changed_at,
                    desired_before,
                    original,
                    notifications,
                    persist_legacy_mirror,
                )
                .await
                .map_err(anyhow::Error::new),
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
        let target_core = app.core;
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

    pub(super) async fn patch_application_config(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        if !crate::client::application_workflow::impact::application_patch_requires_reconcile(
            &patch,
            service_connected,
        ) {
            self.application.patch_typed(patch).await?;
            return Ok(());
        }

        let previous = self.application.get().await?;
        let mut candidate = previous.state.clone();
        candidate.apply(patch);
        let forward = self.application.prepare_replace(candidate.clone()).await?;

        // Runtime-affecting application settings must be reconciled against
        // their candidate before the source config commits. This keeps core,
        // builtin enhancement, and Service Mode changes atomic with runtime.
        if let Err(candidate_error) = self
            .reconcile_application(candidate, service_connected)
            .await
        {
            let latest = match self.application.get().await {
                Ok(latest) => latest.state,
                Err(read_error) => {
                    return Err(anyhow::anyhow!(
                        "{candidate_error:#}; failed to read the latest application config for runtime recovery: {read_error:#}"
                    ));
                }
            };
            return match self
                .reconcile_application(latest, service_connected)
                .await
            {
                Ok(()) => Err(candidate_error.context(
                    "failed to reconcile application runtime candidate; restored the latest persisted runtime",
                )),
                Err(restore_error) => Err(anyhow::anyhow!(
                    "{candidate_error:#}; failed to restore the latest persisted application runtime: {restore_error:#}"
                )),
            };
        }

        let commit = self
            .application
            .replace_prepared_if_version(previous.version, forward)
            .await;
        let commit_error = match commit {
            Ok(crate::state::ConditionalReplaceResult::Replaced(_)) => return Ok(()),
            Ok(crate::state::ConditionalReplaceResult::Conflict { actual_version }) => {
                anyhow::anyhow!(
                    "application config version conflict: expected {}, actual {actual_version}",
                    previous.version
                )
            }
            Err(error) => error.context("failed to commit runtime-affecting application config"),
        };

        let restore =
            match self.application.get().await {
                Ok(latest) => {
                    self.reconcile_application(latest.state, service_connected)
                        .await
                }
                Err(error) => Err(error
                    .context("failed to read the latest application config for runtime recovery")),
            };
        if let Err(rollback_error) = restore {
            return Err(anyhow::anyhow!(
                "{commit_error:#}; failed to restore the latest persisted application runtime: {rollback_error:#}"
            ));
        }
        Err(commit_error)
    }

    async fn reconcile_application(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            if service_connected {
                crate::core::service::ipc::IpcState::Connected
            } else {
                crate::core::service::ipc::IpcState::Disconnected
            },
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
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .reconcile_profiles(clash, target_core, run_type, profiles, app, staged_content)
            .await
    }

    async fn retry_deferred_profiles(&self, expected_digest: &str) -> anyhow::Result<()> {
        let profiles = self
            .profiles
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("Profile state is unavailable for deferred runtime retry")
            })?
            .load()
            .state
            .clone();
        let profiles = Arc::new(profiles);
        let inputs = self
            .capture_profile_inputs(profiles, Default::default())
            .await?;
        if inputs.target_key()? != expected_digest {
            return Ok(());
        }
        let (app, clash, profiles, profile_content) =
            inputs.clone().into_parts().map_err(|error| {
                anyhow::Error::new(
                    crate::client::application_workflow::mutation::RuntimeCheckFailure::
                        candidate_unavailable(error.to_string()),
                )
            })?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .validate_profile_runtime(
                clash.clone(),
                target_core,
                run_type,
                profiles.clone(),
                app.clone(),
                profile_content.clone(),
            )
            .await?;
        self.core
            .reconcile_profiles(clash, target_core, run_type, profiles, app, profile_content)
            .await
    }

    async fn select_core(&self, core: ClashCore) -> anyhow::Result<()> {
        let Some(profiles) = &self.profiles else {
            return self.core.change_core(core).await;
        };

        let previous_app = self.application.get_typed();
        let mut candidate_app = previous_app.clone();
        candidate_app.core = core;
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
            let previous_core = previous_app.core;
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
        let current_core = self.application.get_typed().core;
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn rollback_stale_proxy_action(
    system_proxy: &crate::client::system_proxy::SystemProxyClient,
    original: crate::client::system_proxy::ports::OsProxyConfig,
    error: String,
) -> crate::client::system_proxy::SystemProxyActionError {
    match system_proxy.restore_after_failed_action(original).await {
        Ok(()) => crate::client::system_proxy::SystemProxyActionError::Failed(error),
        Err(rollback_error) => crate::client::system_proxy::SystemProxyActionError::PartialApply(
            format!("{error}; rollback failed: {rollback_error}"),
        ),
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

    async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        CoreLifecycleWorkflow::effect_port_binding(self).await
    }

    async fn capture_profile_inputs(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<crate::client::application_workflow::inputs::RuntimeInputs> {
        let profiles_dir = crate::utils::dirs::app_profiles_dir()?;
        let source = FsProfileContentSource::new(profiles_dir);
        let content =
            crate::client::application_workflow::inputs::FrozenProfileContent::capture_content(
                &profiles,
                &source,
                staged_content,
            );
        Ok(crate::client::application_workflow::inputs::RuntimeInputs {
            app: self.application.get_typed(),
            clash: self.clash.get()?,
            profiles,
            content,
        })
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
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
        let (app, clash, profiles, profile_content) = inputs.into_parts().map_err(|error| {
            anyhow::Error::new(
                crate::client::application_workflow::mutation::RuntimeCheckFailure::
                    candidate_unavailable(error.to_string()),
            )
        })?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .validate_profile_runtime(clash, target_core, run_type, profiles, app, profile_content)
            .await
    }

    async fn prepare_profile_runtime(
        &self,
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
        operation_id: &chimera_core_manager::OperationId,
    ) -> anyhow::Result<(
        crate::client::application_workflow::mutation::AppliedCandidate,
        crate::client::application_workflow::mutation::CheckRecord,
    )> {
        let (app, clash, profiles, profile_content) = inputs.into_parts().map_err(|error| {
            anyhow::Error::new(
                crate::client::application_workflow::mutation::RuntimeCheckFailure::
                    candidate_unavailable(error.to_string()),
            )
        })?;
        let target_core = app.core;
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
                profile_content,
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

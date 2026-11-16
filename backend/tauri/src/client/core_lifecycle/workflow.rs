use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_core::actor_v2::service_status::ServiceHostStatus;
use chimera_core_manager::CoreKind;
use chimera_ipc::api::status::CoreState;
use struct_patch::Patch;
use tokio::sync::watch;

use crate::client::application_workflow::startup::{
    ServiceEvidence, StartupOwnerFailure, startup_stop_error_is_idempotent,
};
#[cfg(test)]
use crate::client::application_workflow::startup::{
    StartupOutcome, StartupReport, StartupRetryTarget, startup_failure_is_retryable,
    startup_failure_waits_for_dependency, update_startup_retry_target,
};
#[cfg(test)]
use crate::client::application_workflow::workflow::ApplicationWorkflow;
use crate::client::application_workflow::{
    inputs::{FrozenProfileContent, RuntimeInputs},
    workflow::ProfileRuntime,
};
use crate::config::chimera::ClashCore;
use crate::enhance::FsProfileContentSource;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    ports::{
        BinaryInstaller, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
        ServiceLifecyclePort, ServiceTransitionLease,
    },
};

pub(in crate::client) enum Command {
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
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    },
    SelectCore(ClashCore),
    RecoverCore,
    StartupReconcile {
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
    },
    RecoverStartup {
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
    },
    RetryStartup {
        operation_id: u64,
        identity: String,
        transition: Box<dyn ServiceTransitionLease>,
    },
    CacheStartupUnsettled {
        operation_id: u64,
        reason: String,
    },
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
fn manager_core_kind(core: ClashCore) -> CoreKind {
    match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::ChimeraClient | ClashCore::ChimeraClientAlpha => CoreKind::ChimeraClient,
    }
}

#[derive(Clone)]
pub(in crate::client) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    clash: ClashConfigClient,
    profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
    core: Arc<dyn CoreLifecyclePort>,
    installer: Arc<dyn BinaryInstaller>,
    service: Arc<dyn ServiceLifecyclePort>,
    startup_owner_unproven: Arc<AtomicBool>,
    #[cfg(test)]
    startup_report: Arc<parking_lot::Mutex<Option<StartupReport>>>,
    service_failed_generation_shared: Arc<AtomicU64>,
    service_status: Option<watch::Receiver<ServiceHostStatus>>,
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
            startup_owner_unproven: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            startup_report: Arc::new(parking_lot::Mutex::new(None)),
            service_failed_generation_shared: Arc::new(AtomicU64::new(0)),
            service_status: None,
        }
    }

    pub(super) fn with_service_status(
        mut self,
        service_status: watch::Receiver<ServiceHostStatus>,
    ) -> Self {
        self.service_status = Some(service_status);
        self
    }

    pub(in crate::client) fn startup_owner_unproven_handle(&self) -> Arc<AtomicBool> {
        self.startup_owner_unproven.clone()
    }

    pub(super) fn service_failed_generation_handle(&self) -> Arc<AtomicU64> {
        self.service_failed_generation_shared.clone()
    }

    pub(super) fn startup_owner_unproven(&self) -> bool {
        self.startup_owner_unproven.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn startup_report(&self) -> Option<StartupReport> {
        self.startup_report.lock().clone()
    }

    #[cfg(test)]
    pub(super) fn cache_startup_report_for_tests(&self, report: &StartupReport) {
        let mut startup_report = self.startup_report.lock();
        if startup_report.is_none() {
            *startup_report = Some(report.clone());
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
        original: chimera_contracts::system_proxy::OsProxyConfig,
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
        original: chimera_contracts::system_proxy::OsProxyConfig,
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

    pub(in crate::client) fn outcome_uncertain(&self) -> bool {
        self.core.outcome_uncertain()
    }

    pub(in crate::client) fn startup_application_config(&self) -> ChimeraAppConfig {
        self.application.get_typed()
    }

    pub(in crate::client) async fn startup_core_status(
        &self,
    ) -> anyhow::Result<CoreStatusSnapshot> {
        self.core.status().await
    }

    pub(in crate::client) async fn startup_service_command_settled(&self) -> bool {
        self.service.command_settled().await
    }

    pub(in crate::client) fn startup_failed_generation(&self) -> u64 {
        self.service_failed_generation_shared
            .load(Ordering::Acquire)
    }

    pub(in crate::client) fn set_startup_failed_generation(&self, generation: u64) {
        self.service_failed_generation_shared
            .store(generation, Ordering::Release);
    }

    pub(in crate::client) async fn startup_select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        self.core.select_execution_host(host).await
    }

    pub(in crate::client) async fn startup_reconcile_application(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        self.reconcile_application(app, service_connected).await
    }

    pub(in crate::client) async fn startup_reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
    ) -> anyhow::Result<()> {
        self.reconcile_application_after_service_release(app).await
    }

    pub(super) fn service_restart_policy(
        &self,
    ) -> chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
        self.service.restart_policy()
    }

    pub(in crate::client) async fn execute(
        &self,
        command: Command,
        #[cfg(test)] startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> anyhow::Result<()> {
        #[cfg(test)]
        let refresh_startup_retry_identity =
            matches!(&command, Command::PatchApplicationConfig { .. });
        #[cfg(test)]
        let refresh_startup_target = match &command {
            Command::Reconcile
            | Command::RecoverCore
            | Command::RetryDeferredProfiles { .. }
            | Command::ReconcileProfiles { .. }
            | Command::SelectCore(_) => true,
            Command::PatchApplicationConfig {
                patch,
                service_connected,
            } => crate::client::application_workflow::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            ),
            _ => false,
        };
        let result = match command {
            Command::ProfileMutation(_) => {
                anyhow::bail!("Profile mutations must run through ApplicationWorkflowActor")
            }
            Command::RecoverCore | Command::Reconcile => self.reconcile().await,
            #[cfg(test)]
            Command::StartupReconcile {
                operation_id,
                transition,
            } => {
                let report = self
                    .startup_reconcile(operation_id, transition, startup_retry_target)
                    .await;
                match report.outcome.failure_reason() {
                    Some(reason) => Err(anyhow::anyhow!(reason.to_owned())),
                    None => Ok(()),
                }
            }
            #[cfg(test)]
            Command::RecoverStartup {
                operation_id,
                transition,
            } => {
                let report = self
                    .recover_startup(operation_id, transition, startup_retry_target)
                    .await;
                match report.outcome.failure_reason() {
                    Some(reason) => Err(anyhow::anyhow!(reason.to_owned())),
                    None => Ok(()),
                }
            }
            #[cfg(test)]
            Command::RetryStartup {
                operation_id,
                identity,
                transition,
            } => {
                let report = self
                    .retry_startup(operation_id, identity, transition, startup_retry_target)
                    .await;
                match report.outcome.failure_reason() {
                    Some(reason) => Err(anyhow::anyhow!(reason.to_owned())),
                    None => Ok(()),
                }
            }
            #[cfg(not(test))]
            Command::StartupReconcile { .. }
            | Command::RecoverStartup { .. }
            | Command::RetryStartup { .. } => {
                anyhow::bail!("startup commands must be handled by ApplicationWorkflow")
            }
            Command::CacheStartupUnsettled { .. } => {
                anyhow::bail!("an unsettled startup report must be cached by ApplicationWorkflow")
            }
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
        };
        #[cfg(test)]
        {
            if refresh_startup_target {
                self.refresh_startup_retry_target_after_reconcile(startup_retry_target, &result)
                    .await;
            } else if refresh_startup_retry_identity && result.is_ok() {
                self.reschedule_startup_retry_after_committed_config_patch(startup_retry_target)
                    .await;
            }
        }
        result
    }

    /// A source-only application patch can still change the committed startup
    /// target. Ref's `confirm_saved_inactive` wakes an open reestablish target
    /// for a save and opens a fresh retry budget only when its identity moves.
    #[cfg(test)]
    async fn reschedule_startup_retry_after_committed_config_patch(
        &self,
        startup_retry_target: &mut Option<StartupRetryTarget>,
    ) {
        let Some(identity_before) = startup_retry_target
            .as_ref()
            .map(|target| target.identity.clone())
        else {
            return;
        };

        let identity = match self.startup_target_identity().await {
            Ok(identity) => identity,
            Err(_) => match self.unresolved_startup_target_identity().await {
                Some(identity) => identity,
                None => return,
            },
        };
        let Some(target) = startup_retry_target.as_mut() else {
            return;
        };
        if target.identity != identity_before
            || target.health == crate::client::convergence::ConvergenceHealth::RecoveryRequired
        {
            return;
        }

        if target.identity != identity {
            let allow_receipt_reuse = target.allow_receipt_reuse;
            *target = StartupRetryTarget::new(identity, allow_receipt_reuse);
        } else if target.budget.remaining == 0 {
            return;
        }
        target.health = crate::client::convergence::ConvergenceHealth::RetryScheduled;
        target.next_attempt = Some(tokio::time::Instant::now());
    }

    #[cfg(test)]
    async fn refresh_startup_retry_target_after_reconcile(
        &self,
        startup_retry_target: &mut Option<StartupRetryTarget>,
        result: &anyhow::Result<()>,
    ) {
        if startup_retry_target.is_none() {
            return;
        }
        if result.is_ok() {
            *startup_retry_target = None;
            return;
        }

        let identity = match self.startup_target_identity().await {
            Ok(identity) => Some(identity),
            Err(_) => self.unresolved_startup_target_identity().await,
        };
        if let Some(identity) = identity {
            if startup_retry_target
                .as_ref()
                .is_none_or(|target| target.identity != identity)
            {
                let allow_receipt_reuse = startup_retry_target
                    .as_ref()
                    .is_some_and(|target| target.allow_receipt_reuse);
                *startup_retry_target =
                    Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
            }
        }

        let error = result
            .as_ref()
            .expect_err("failed reconciliation has an error");
        let operation_id = 0;
        let outcome = if self.core.outcome_uncertain() {
            StartupOutcome::RecoveryRequired {
                reason: format!("runtime target reconciliation is uncertain: {error:#}"),
            }
        } else if startup_failure_waits_for_dependency(error) {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                reason: format!(
                    "runtime target reconciliation is waiting for a dependency: {error:#}"
                ),
            }
        } else if startup_failure_is_retryable(error) {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                reason: format!("runtime target reconciliation failed transiently: {error:#}"),
            }
        } else {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                reason: format!("runtime target reconciliation failed: {error:#}"),
            }
        };
        let report = StartupReport {
            operation_id,
            observation: None,
            outcome,
        };
        let allow_receipt_reuse = startup_retry_target
            .as_ref()
            .is_some_and(|target| target.allow_receipt_reuse);
        update_startup_retry_target(
            self,
            report,
            false,
            allow_receipt_reuse,
            startup_retry_target,
        )
        .await;
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
            self.reconcile_profiles(Arc::new(profiles.load().state.clone()), Default::default())
                .await
        } else {
            self.core.reconcile(clash, target_core, run_type).await
        }
    }

    /// Test-only Direct-client bridge. Startup execution itself delegates to
    /// ApplicationWorkflow, matching the production actor path.
    #[cfg(test)]
    pub(in crate::client) async fn startup_reconcile(
        &self,
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
        startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> StartupReport {
        let mut application = ApplicationWorkflow::with_lifecycle(self.clone());
        if let Some(target) = startup_retry_target.as_ref().cloned() {
            application.set_startup_retry_target_for_tests(target);
        }
        let report = application
            .startup_reconcile(operation_id, transition)
            .await;
        *startup_retry_target = application.startup_retry_target_for_tests();
        self.cache_startup_report_for_tests(&report);
        report
    }

    #[cfg(test)]
    pub(in crate::client) async fn recover_startup(
        &self,
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
        startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> StartupReport {
        let mut application = ApplicationWorkflow::with_lifecycle(self.clone());
        if let Some(target) = startup_retry_target.as_ref().cloned() {
            application.set_startup_retry_target_for_tests(target);
        }
        let report = application.recover_startup(operation_id, transition).await;
        *startup_retry_target = application.startup_retry_target_for_tests();
        self.cache_startup_report_for_tests(&report);
        report
    }

    #[cfg(test)]
    pub(in crate::client) async fn retry_startup(
        &self,
        operation_id: u64,
        identity: String,
        transition: Box<dyn ServiceTransitionLease>,
        startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> StartupReport {
        let mut application = ApplicationWorkflow::with_lifecycle(self.clone());
        if let Some(target) = startup_retry_target.as_ref().cloned() {
            application.set_startup_retry_target_for_tests(target);
        }
        let report = application
            .retry_startup(operation_id, identity, transition)
            .await;
        *startup_retry_target = application.startup_retry_target_for_tests();
        self.cache_startup_report_for_tests(&report);
        report
    }

    pub(in crate::client) async fn recovery_receipt_matches(
        &self,
        current: &CoreStatusSnapshot,
        desired: crate::core::actor_v2::control_endpoint::ExecutionHost,
        allow_receipt_reuse: bool,
    ) -> Result<bool, String> {
        if !allow_receipt_reuse || !matches!(current.state, CoreState::Running) {
            return Ok(false);
        }
        let receipt = match self.core.active_runtime_receipt().await {
            Ok(Some(receipt)) => receipt,
            Ok(None) => return Ok(false),
            Err(error) => {
                tracing::debug!(%error, "startup recovery cannot verify an active runtime receipt");
                return Ok(false);
            }
        };
        let identity = match self.startup_target_identity().await {
            Ok(identity) => identity,
            Err(error) => {
                tracing::debug!(%error, "startup recovery cannot identify the committed runtime target");
                return Ok(false);
            }
        };
        if receipt.host != desired
            || receipt.run_intent
                != crate::client::application_workflow::policy::CoreRunIntent::Running
            || receipt.target.as_deref() != Some(identity.as_str())
        {
            return Ok(false);
        }

        let observed = self.core.status().await.map_err(|error| {
            format!("could not recheck the runtime after receipt verification: {error:#}")
        })?;
        if !same_core_status(current, &observed) {
            return Err(
                "the Core owner changed while the recovery receipt was being verified".into(),
            );
        }
        // `CoreStatusSnapshot` intentionally carries only lifecycle state and
        // timestamp. A shared Service Core can receive an in-place revision
        // update without changing either, so the final proof must query the
        // host again through the adapter that validates the complete applied
        // revision, source hash, epoch, and core kind.
        let revalidated =
            self.core.active_runtime_receipt().await.map_err(|error| {
                format!("could not revalidate the running Core receipt: {error:#}")
            })?;
        if !revalidated
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&receipt, current))
        {
            return Ok(false);
        }
        Ok(true)
    }

    pub(in crate::client) async fn confirm_local_owner_stopped(
        &self,
        current: &CoreStatusSnapshot,
    ) -> Result<(), StartupOwnerFailure> {
        let before_stop = self.core.status().await.map_err(|error| {
            StartupOwnerFailure::WaitingDependency(format!(
                "could not refresh the Local Core owner: {error:#}"
            ))
        })?;
        if !same_core_status(current, &before_stop)
            || !matches!(before_stop.state, CoreState::Running)
        {
            return Err(StartupOwnerFailure::WaitingDependency(
                "the Local Core owner changed before it could be stopped".into(),
            ));
        }
        let stop_result = self.core.stop().await;
        if self.core.outcome_uncertain() {
            return Err(StartupOwnerFailure::RecoveryRequired(
                "the Local Core stop outcome is uncertain".into(),
            ));
        }
        let stopped = match self.core.status().await {
            Ok(stopped) => stopped,
            Err(error) => {
                return Err(match stop_result {
                    Err(stop_error) => StartupOwnerFailure::stop_rejected(
                        stop_error,
                        self.core.outcome_uncertain(),
                    ),
                    Ok(()) => StartupOwnerFailure::WaitingDependency(format!(
                        "could not confirm the Local Core stop: {error:#}"
                    )),
                });
            }
        };
        let stop_confirmed = stopped.run_type == current.run_type
            && stopped.state_changed_at > 0
            && matches!(&stopped.state, CoreState::Stopped(_));
        if stop_confirmed {
            if let Err(error) = stop_result
                && !startup_stop_error_is_idempotent(&error)
            {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
        } else {
            if let Err(error) = stop_result {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
            return Err(StartupOwnerFailure::WaitingDependency(format!(
                "the Local Core did not publish a confirmed stopped state: {stopped:?}"
            )));
        }
        self.confirm_local_stopped_baseline(&stopped)
            .await
            .map_err(StartupOwnerFailure::WaitingDependency)
    }

    pub(in crate::client) async fn confirm_local_stopped_baseline(
        &self,
        stopped: &CoreStatusSnapshot,
    ) -> Result<(), String> {
        if stopped.state_changed_at == 0 {
            return Ok(());
        }
        let baseline = self
            .core
            .observe_runtime_baseline()
            .await
            .map_err(|error| {
                format!("the Local Core stop lacks authoritative state evidence: {error:#}")
            })?;
        if !matches!(
            baseline,
            crate::client::application_workflow::mutation::KnownRuntimeState::Stopped
        ) {
            return Err(format!(
                "the Local Core runtime is not proven stopped after retirement: {baseline:?}"
            ));
        }
        let confirmed = self
            .core
            .status()
            .await
            .map_err(|error| format!("could not recheck the stopped Local Core: {error:#}"))?;
        if !same_core_status(stopped, &confirmed)
            || !matches!(&confirmed.state, CoreState::Stopped(_))
        {
            return Err("the Local Core state changed after its stop was confirmed".into());
        }
        Ok(())
    }

    pub(in crate::client) async fn recover_service_error_stop(
        &self,
        service: ServiceEvidence,
        previous_generation: u64,
    ) -> Result<(ServiceEvidence, u64), StartupOwnerFailure> {
        if !matches!(
            &service,
            ServiceEvidence::CoreStoppedWithReason {
                runtime_owned: true,
                ..
            }
        ) {
            return Ok((service, previous_generation));
        }

        self.core
            .recover_service_quarantine()
            .await
            .map_err(|error| {
                StartupOwnerFailure::stop_rejected(error, self.core.outcome_uncertain())
            })?;

        let (service, fresh_generation) = self.service_evidence().await;
        if fresh_generation != previous_generation || !service.proves_runtime_owner() {
            return Err(StartupOwnerFailure::RecoveryRequired(
                "the Service runtime identity changed during quarantine recovery; startup will not hand off another host".into(),
            ));
        }
        let service = match service {
            ServiceEvidence::CoreStoppedWithReason {
                ready,
                phase,
                runtime_owned: true,
                ..
            } => {
                // Recover returns only after the manager has cleared its
                // quarantined epochs; some harmless stop reasons remain in
                // the status snapshot when there was no quarantine.
                ServiceEvidence::CoreStopped {
                    ready,
                    phase,
                    runtime_owned: true,
                }
            }
            service => service,
        };
        Ok((service, fresh_generation))
    }

    pub(in crate::client) async fn retire_service_startup_owner(
        &self,
        service: &ServiceEvidence,
        require_ready_stopped: bool,
    ) -> Result<ServiceEvidence, StartupOwnerFailure> {
        let expected_owner = service.expected_owner().map_err(|reason| {
            StartupOwnerFailure::RecoveryRequired(format!(
                "the Service Core owner cannot be fenced for retirement: {reason}"
            ))
        })?;
        let stop_result = self.core.retire_service_core(expected_owner).await;
        if self.core.outcome_uncertain() {
            return Err(StartupOwnerFailure::RecoveryRequired(
                "the Service Core stop outcome is uncertain".into(),
            ));
        }
        let (after_stop, _) = self.service_evidence().await;
        let confirmed = if require_ready_stopped {
            matches!(
                &after_stop,
                ServiceEvidence::CoreStopped { ready: true, .. }
            )
        } else {
            after_stop.proves_no_owned_core()
        };
        if confirmed {
            if let Err(error) = stop_result
                && !startup_stop_error_is_idempotent(&error)
            {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
            return Ok(after_stop);
        }
        if matches!(&after_stop, ServiceEvidence::Unreadable { .. }) {
            if let Err(error) = stop_result {
                return Err(StartupOwnerFailure::stop_rejected(
                    error,
                    self.core.outcome_uncertain(),
                ));
            }
            return Err(StartupOwnerFailure::WaitingDependency(format!(
                "the Service Core stop could not be confirmed by a fresh probe: {after_stop:?}"
            )));
        }
        if let Err(error) = stop_result {
            return Err(StartupOwnerFailure::stop_rejected(
                error,
                self.core.outcome_uncertain(),
            ));
        }
        Err(StartupOwnerFailure::WaitingDependency(format!(
            "the Service Core stop was accepted but a fresh probe did not confirm release: {after_stop:?}"
        )))
    }

    async fn capture_reconcile_target(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<(
        String,
        ChimeraAppConfig,
        chimera_config::clash::config::ClashConfig,
        Arc<chimera_config::profile::Profiles>,
        std::collections::BTreeMap<String, String>,
    )> {
        let inputs = self
            .capture_profile_inputs_for(app, clash, profiles, staged_content)
            .await?;
        let target = inputs.target_key()?;
        let (app, clash, profiles, staged_content) = inputs.into_parts().map_err(|error| {
            anyhow::Error::new(
                crate::client::application_workflow::mutation::RuntimeCheckFailure::
                    candidate_unavailable(error.to_string()),
            )
        })?;
        Ok((target, app, clash, profiles, staged_content))
    }

    async fn capture_profile_inputs_for(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<RuntimeInputs> {
        let profiles_dir = crate::utils::dirs::app_profiles_dir()?;
        let source = FsProfileContentSource::new(profiles_dir);
        let content = FrozenProfileContent::capture_content(&profiles, &source, staged_content);
        Ok(RuntimeInputs {
            app,
            clash,
            profiles,
            content,
        })
    }

    pub(in crate::client) async fn startup_target_identity(&self) -> anyhow::Result<String> {
        if let Some(profiles) = &self.profiles {
            let profiles = Arc::new(profiles.load().state.clone());
            let inputs = self
                .capture_profile_inputs_for(
                    self.application.get_typed(),
                    self.clash.get()?,
                    profiles,
                    Default::default(),
                )
                .await?;
            return inputs.target_key();
        }
        self.startup_source_identity(None)
    }

    pub(in crate::client) async fn unresolved_startup_target_identity(&self) -> Option<String> {
        let profiles = self
            .profiles
            .as_ref()
            .map(|profiles| Arc::new(profiles.load().state.clone()));
        let identity = self.startup_source_identity(profiles.as_ref()).ok()?;
        Some(format!("profile-content-unavailable:{identity}"))
    }

    fn startup_source_identity(
        &self,
        profiles: Option<&Arc<chimera_config::profile::Profiles>>,
    ) -> anyhow::Result<String> {
        let app = self.application.get_typed();
        let clash = self.clash.get()?;
        let app_target = crate::client::application_workflow::impact::application_target(&app)
            .ok_or_else(|| anyhow::anyhow!("application runtime identity cannot be serialized"))?;
        let clash_target = crate::client::application_workflow::impact::clash_target(&clash)
            .ok_or_else(|| anyhow::anyhow!("clash runtime identity cannot be serialized"))?;
        let profile_target = match profiles {
            Some(profiles) => crate::client::application_workflow::impact::profiles_target(
                profiles,
            )
            .ok_or_else(|| anyhow::anyhow!("profiles runtime identity cannot be serialized"))?,
            None => "no-profile-runtime".into(),
        };
        let projection = serde_json::to_vec(&(
            app_target,
            clash_target,
            profile_target,
            "profile-content-unavailable",
        ))?;
        Ok(crate::state::profiles::payload_digest(&projection))
    }

    pub(in crate::client) async fn release_service_owner(&self) -> Result<ServiceEvidence, String> {
        let (service, _) = self.service_evidence().await;
        match &service {
            ServiceEvidence::CoreRunning { .. } => {
                let expected_owner = service.expected_owner()?;
                self.core
                    .retire_service_core(expected_owner)
                    .await
                    .map_err(|error| format!("conditional Service Core stop failed: {error:#}"))?;
                if self.core.outcome_uncertain() {
                    return Err("the Service Core stop outcome is uncertain".into());
                }
                let (after_stop, _) = self.service_evidence().await;
                if after_stop.proves_no_owned_core() {
                    Ok(after_stop)
                } else {
                    Err(format!(
                        "a fresh Service probe did not confirm release: {after_stop:?}"
                    ))
                }
            }
            ServiceEvidence::Absent { .. }
            | ServiceEvidence::CoreStopped { .. }
            | ServiceEvidence::ForeignCore { .. } => Ok(service),
            ServiceEvidence::CoreStoppedWithReason { .. } | ServiceEvidence::Unreadable { .. } => {
                Err("the fresh Service owner probe was unreadable".into())
            }
        }
    }

    pub(in crate::client) async fn service_evidence(&self) -> (ServiceEvidence, u64) {
        let Ok(info) = self.service.probe().await else {
            let generation = self
                .service_status
                .as_ref()
                .map(|status| status.borrow().ready_generation)
                .unwrap_or_default();
            return (
                ServiceEvidence::Unreadable {
                    phase: chimera_core::actor_v2::service_policy::ServicePhase::Unknown,
                },
                generation,
            );
        };
        let runtime_owned = self.service.runtime_owned(&info);
        let previous = self
            .service_status
            .as_ref()
            .map(|status| status.borrow().clone())
            .unwrap_or_else(ServiceHostStatus::probing);
        let status =
            previous.observe_probe(info, runtime_owned, Some(self.service.restart_policy()));
        let generation = status.ready_generation;
        (ServiceEvidence::from_status(&status), generation)
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
        let Some(profiles) = &self.profiles else {
            let target_core = app.core;
            let run_type = crate::core::RunType::classify(
                app.enable_service_mode,
                if service_connected {
                    crate::core::service::ipc::IpcState::Connected
                } else {
                    crate::core::service::ipc::IpcState::Disconnected
                },
            );
            return self.core.reconcile(clash, target_core, run_type).await;
        };
        let (target, app, clash, profiles, staged_content) = self
            .capture_reconcile_target(
                app,
                clash,
                Arc::new(profiles.load().state.clone()),
                Default::default(),
            )
            .await?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            if service_connected {
                crate::core::service::ipc::IpcState::Connected
            } else {
                crate::core::service::ipc::IpcState::Disconnected
            },
        );
        self.core
            .reconcile_profiles(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                staged_content,
                Some(target),
            )
            .await
    }

    async fn reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let Some(profiles) = &self.profiles else {
            return self
                .core
                .reconcile_after_service_release(clash, app.core)
                .await;
        };
        let (target, app, clash, profiles, staged_content) = self
            .capture_reconcile_target(
                app,
                clash,
                Arc::new(profiles.load().state.clone()),
                Default::default(),
            )
            .await?;
        self.core
            .reconcile_profiles_after_service_release(
                clash,
                app.core,
                profiles,
                app,
                staged_content,
                Some(target),
            )
            .await
    }

    async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let (target, app, clash, profiles, staged_content) = self
            .capture_reconcile_target(app, clash, profiles, staged_content)
            .await?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        self.core
            .reconcile_profiles(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                staged_content,
                Some(target),
            )
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
            .reconcile_profiles(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                profile_content,
                Some(expected_digest.to_owned()),
            )
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
        let (candidate_target, candidate_app, candidate_clash, candidate_profiles, staged_content) =
            self.capture_reconcile_target(
                candidate_app,
                clash.clone(),
                profile_snapshot.clone(),
                Default::default(),
            )
            .await?;
        let run_type = crate::core::RunType::classify(
            candidate_app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );

        self.core
            .reconcile_profiles(
                candidate_clash,
                core,
                run_type,
                candidate_profiles,
                candidate_app.clone(),
                staged_content,
                Some(candidate_target),
            )
            .await?;

        if let Err(error) = self.application.patch_core(candidate_app.core).await {
            let previous_core = previous_app.core;
            let (previous_target, previous_app, previous_clash, previous_profiles, staged_content) =
                self.capture_reconcile_target(
                    previous_app,
                    clash,
                    profile_snapshot,
                    Default::default(),
                )
                .await?;
            let rollback = self
                .core
                .reconcile_profiles(
                    previous_clash,
                    previous_core,
                    run_type,
                    previous_profiles,
                    previous_app,
                    staged_content,
                    Some(previous_target),
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
        let desired_core = self.application.get_typed().core;

        // The selected core is desired state, not proof of what is currently
        // running. A confirmed live receipt identifies the active executable
        // even when an older host omits its applied-kind field. If the host is
        // running but cannot provide that proof, stop conservatively before
        // replacing a binary that may be in use.
        let active_receipt = match self.core.active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "could not verify the active core before binary replacement; treating its identity as unknown"
                );
                None
            }
        };
        let status = self.core.status().await?;
        // Service status failures are projected to Stopped with timestamp 0;
        // that compatibility value is unknown, not proof that the process exited.
        let not_proven_stopped = active_receipt.is_some()
            || status.state_changed_at == 0
            || !matches!(status.state, CoreState::Stopped(_));
        let target_kind = manager_core_kind(artifact.target);
        let active_kind = active_receipt.map(|receipt| receipt.core_spec.kind);
        let needs_stop =
            not_proven_stopped && active_kind.is_none_or(|active_kind| active_kind == target_kind);
        let restart = desired_core == artifact.target || needs_stop;

        if needs_stop {
            self.core.stop().await?;
        }

        let request = chimera_contracts::core_install::CoreBinaryInstallRequest {
            target: artifact.target,
            source: artifact.source.clone(),
            destination: artifact.destination.clone(),
            staging: artifact.staging.clone(),
            // Chimera's currently distributed core archives contain no companion
            // directory. Keep the upstream optional contract without inventing
            // Meow-specific or unsupported release contents.
            companion: None,
        };
        self.installer.install(&request).await?;
        if restart {
            artifact.progress.restarting();
            self.reconcile().await?;
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn rollback_stale_proxy_action(
    system_proxy: &crate::client::system_proxy::SystemProxyClient,
    original: chimera_contracts::system_proxy::OsProxyConfig,
    error: String,
) -> crate::client::system_proxy::SystemProxyActionError {
    match system_proxy.restore_after_failed_action(original).await {
        Ok(()) => crate::client::system_proxy::SystemProxyActionError::Failed(error),
        Err(rollback_error) => crate::client::system_proxy::SystemProxyActionError::PartialApply(
            format!("{error}; rollback failed: {rollback_error}"),
        ),
    }
}

fn same_core_status(left: &CoreStatusSnapshot, right: &CoreStatusSnapshot) -> bool {
    let same_state = match (&left.state, &right.state) {
        (CoreState::Running, CoreState::Running) => true,
        (CoreState::Stopped(left_reason), CoreState::Stopped(right_reason)) => {
            left_reason == right_reason
        }
        _ => false,
    };
    left.instance_id == right.instance_id
        && same_state
        && left.state_changed_at == right.state_changed_at
        && left.run_type == right.run_type
}

#[async_trait::async_trait]
impl ProfileRuntime for CoreLifecycleWorkflow {
    fn status(&self) -> crate::client::core_lifecycle::CoreLifecycleStatus {
        crate::client::core_lifecycle::CoreLifecycleStatus {
            uncertain: self.core.outcome_uncertain() || self.startup_owner_unproven(),
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
        self.capture_profile_inputs_for(
            self.application.get_typed(),
            self.clash.get()?,
            profiles,
            staged_content,
        )
        .await
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
        let target = inputs.target_key().map_err(|error| {
            anyhow::Error::new(
                crate::client::application_workflow::mutation::RuntimeCheckFailure::
                    candidate_unavailable(error.to_string()),
            )
        })?;
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
                Some(target),
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
            .await?;
        Ok(())
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

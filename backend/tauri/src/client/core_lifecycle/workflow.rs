use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_core::client::core_lifecycle::workflow::ServiceRecovery;
use chimera_core::{
    actor_v2::service_status::ServiceHostStatus, client::reconciler::plan::Trigger,
};
#[cfg(test)]
use chimera_ipc::api::status::CoreState;
use tokio::sync::watch;

#[cfg(test)]
use crate::client::application_workflow::startup::{
    ServiceEvidence, StartupOutcome, StartupReport, StartupRetryTarget,
    startup_failure_is_retryable, startup_failure_waits_for_dependency,
    update_startup_retry_target,
};
#[cfg(test)]
use crate::client::application_workflow::workflow::ApplicationWorkflow;
use crate::client::application_workflow::workflow::ProfileRuntime;
use crate::client::reconciler::committed::Selection;
#[cfg(test)]
use crate::client::reconciler::committed::SelectionChange;
#[cfg(test)]
use crate::client::reconciler::decision::service_prepared_owner_proven;
use crate::client::reconciler::state::RuntimeProgress;
#[cfg(test)]
use crate::client::reconciler::state::proven_selection_from_receipt;
use crate::config::chimera::ClashCore;
use tokio_util::sync::CancellationToken;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    ports::{
        BinaryInstaller, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
        ServiceLifecyclePort, ServiceTransitionLease,
    },
};

pub(in crate::client) use crate::client::reconciler::legacy_source::ApplicationPatchResult;

pub(in crate::client) enum Command {
    #[cfg(test)]
    ProfileMutation(Box<crate::client::application_workflow::mutation::MutationRequest>),
    Shutdown,
    /// Explicit user intent, distinct from automatic reconciliation.
    StartCore,
    StopCore,
    PatchApplicationConfig {
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
        /// Reply after the source commit; runtime failures are degradations,
        /// not evidence that the committed application.yaml rolled back.
        degradations: tokio::sync::oneshot::Sender<ApplicationPatchResult>,
        /// Captured by the old actor completion path until the ref single
        /// Reconciler owns both source-version and applied-target status.
        runtime_converged: Arc<AtomicBool>,
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
    /// Retired production entry. Legacy actor tests retain the old adapter.
    #[cfg(test)]
    SelectCore(ClashCore),
    RecoverCore,
    /// Re-evaluate the runtime owner after Service readiness changes.
    FollowService {
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
    },
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
    /// The exact upstream checked bytes, handed from the only lifecycle
    /// mailbox into the existing CoreFacade mutation executor. No rebuild.
    ApplyChecked {
        prepared: super::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        domain: super::super::reconciler::committed::SourceDomain,
        version: u64,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        interrupt_mode: bool,
    },
    /// One source notification requests a latest-committed-target round.
    ReconcileSource,
    /// Ref release-owed maintenance; no Core submit or startup handoff.
    MaintainServiceRelease,
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

// ref apply_prepared owns post-Core one-shot effects. The sole lifecycle
// Actor still decides when its final source/version fence permits completion.
pub(in crate::client) use crate::client::reconciler::apply::CheckedConnectionEffect;

#[derive(Clone)]
pub(in crate::client) struct CoreLifecycleWorkflow {
    pub(in crate::client) application: ApplicationClient,
    pub(in crate::client) clash: ClashConfigClient,
    pub(in crate::client) profiles:
        Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
    pub(in crate::client) content_reader: Option<crate::state::mutation::ProfileContentReader>,
    /// Installed once by the production Client after the canonical Core
    /// session and Profiles gate have been assembled. Shared by clones of
    /// the sole lifecycle workflow, not an independent runtime owner.
    pub(in crate::client) checked_gate:
        Arc<std::sync::OnceLock<Arc<crate::client::reconciler::check::CandidateCheck>>>,
    /// Ref's applied selection and last target/host attempt share a single
    /// runtime-progress owner. Only the current Core worker may record them,
    /// and the Actor retains final Conclusion/source-version authority.
    pub(in crate::client) progress: Arc<parking_lot::Mutex<RuntimeProgress>>,
    /// A checked source's post-apply Mode effect is held by the same
    /// workflow until its sole Actor has fenced the Completed message.
    checked_connection_effect: Arc<parking_lot::Mutex<Option<CheckedConnectionEffect>>>,
    /// A one-round observed Core terminal, shared only with the existing
    /// Reconciler policy and Actor Completed. Never records a source commit
    /// or replaces the independent Running/source convergence fence.
    pub(in crate::client) prepared_outcome:
        Arc<parking_lot::Mutex<Option<crate::client::reconciler::mutation::ApplyOutcome>>>,
    /// The exact read-only source authority already used by Checked Apply.
    /// Ordinary startup/reconcile may adopt selection only when an active
    /// Core receipt and this authority agree on the full committed target.
    pub(in crate::client) source_authority:
        Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    pub(in crate::client) core: Arc<dyn CoreLifecyclePort>,
    pub(in crate::client) installer: Arc<dyn BinaryInstaller>,
    pub(in crate::client) service: Arc<dyn ServiceLifecyclePort>,
    startup_owner_unproven: Arc<AtomicBool>,
    #[cfg(test)]
    startup_report: Arc<parking_lot::Mutex<Option<StartupReport>>>,
    pub(in crate::client) service_failed_generation_shared: Arc<AtomicU64>,
    pub(in crate::client) service_status: Option<watch::Receiver<ServiceHostStatus>>,
    pub(in crate::client) service_recovery: Arc<parking_lot::Mutex<ServiceRecovery>>,
    pub(in crate::client) service_recovery_reconnected: Arc<AtomicBool>,
    pub(in crate::client) shutdown: CancellationToken,
}

impl CoreLifecycleWorkflow {
    pub(in crate::client) fn new(
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
            content_reader: None,
            checked_gate: Arc::new(std::sync::OnceLock::new()),
            progress: Arc::new(parking_lot::Mutex::new(RuntimeProgress::default())),
            checked_connection_effect: Arc::new(parking_lot::Mutex::new(None)),
            prepared_outcome: Arc::new(parking_lot::Mutex::new(None)),
            source_authority: None,
            core,
            installer,
            service,
            startup_owner_unproven: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            startup_report: Arc::new(parking_lot::Mutex::new(None)),
            service_failed_generation_shared: Arc::new(AtomicU64::new(0)),
            service_status: None,
            service_recovery: Arc::new(parking_lot::Mutex::new(ServiceRecovery::default())),
            service_recovery_reconnected: Arc::new(AtomicBool::new(false)),
            shutdown: CancellationToken::new(),
        }
    }

    pub(in crate::client) fn install_checked_gate(
        &self,
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
    ) -> anyhow::Result<()> {
        self.checked_gate
            .set(gate)
            .map_err(|_| anyhow::anyhow!("production candidate gate already installed"))
    }

    pub(in crate::client) fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = shutdown;
        self
    }

    pub(in crate::client) fn with_profile_content_reader(
        mut self,
        reader: Option<crate::state::mutation::ProfileContentReader>,
    ) -> Self {
        self.content_reader = reader;
        self
    }

    pub(in crate::client) fn with_source_authority(
        mut self,
        authority: Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    ) -> Self {
        self.source_authority = authority;
        self
    }

    pub(in crate::client) fn with_service_status(
        mut self,
        service_status: watch::Receiver<ServiceHostStatus>,
    ) -> Self {
        self.service_status = Some(service_status);
        self
    }

    pub(in crate::client) fn invalidate_applied_selection(&self) {
        self.progress.lock().invalidate_selection();
    }

    pub(in crate::client) fn take_checked_connection_effect(
        &self,
    ) -> Option<CheckedConnectionEffect> {
        self.checked_connection_effect.lock().take()
    }

    /// Consume the observed Prepared terminal only in the owner Actor's
    /// Completed message; source commits and old Core ports have none.
    pub(in crate::client) fn take_prepared_outcome(
        &self,
    ) -> Option<crate::client::reconciler::mutation::ApplyOutcome> {
        self.prepared_outcome.lock().take()
    }
    /// Recover selection from a *proven* live Core receipt and two bounded
    /// committed source reads. RuntimeProgress, not the legacy workflow,
    /// owns the selected Mode/Profile and last attempted target/host.
    #[cfg(test)]
    pub(in crate::client) async fn recover_applied_selection_from_receipt(&self) {
        let selection =
            proven_selection_from_receipt(self.core.as_ref(), self.source_authority.as_deref())
                .await;
        self.progress.lock().set_selection(selection);
    }

    #[cfg(test)]
    pub(in crate::client) fn applied_selection_for_tests(&self) -> Option<Selection> {
        self.progress.lock().selection()
    }

    #[cfg(test)]
    pub(in crate::client) fn set_applied_selection_for_tests(&self, selection: Selection) {
        self.progress.lock().set_selection(Some(selection));
    }

    pub(in crate::client) fn startup_owner_unproven_handle(&self) -> Arc<AtomicBool> {
        self.startup_owner_unproven.clone()
    }

    pub(in crate::client) fn service_failed_generation_handle(&self) -> Arc<AtomicU64> {
        self.service_failed_generation_shared.clone()
    }

    pub(in crate::client) fn startup_owner_unproven(&self) -> bool {
        self.startup_owner_unproven.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(in crate::client) fn startup_report(&self) -> Option<StartupReport> {
        self.startup_report.lock().clone()
    }

    #[cfg(test)]
    pub(in crate::client) fn cache_startup_report_for_tests(&self, report: &StartupReport) {
        let mut startup_report = self.startup_report.lock();
        if startup_report.is_none() {
            *startup_report = Some(report.clone());
        }
    }

    pub(in crate::client) async fn probe_service(
        &self,
    ) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.service.probe().await
    }

    pub(in crate::client) async fn effect_port_binding(
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

    pub(in crate::client) fn convergence_clash(
        &self,
    ) -> anyhow::Result<chimera_config::clash::config::ClashConfig> {
        self.clash.get()
    }

    pub(in crate::client) fn convergence_profiles_version(&self) -> Option<u64> {
        self.profiles
            .as_ref()
            .map(|profiles| *profiles.load().version.as_ref())
    }

    pub(in crate::client) fn convergence_authority(
        &self,
    ) -> Option<&dyn crate::client::reconciler::authority::RuntimeSourceAuthority> {
        self.source_authority.as_deref()
    }

    pub(in crate::client) fn convergence_candidate_check(
        &self,
    ) -> Option<&crate::client::reconciler::check::CandidateCheck> {
        self.checked_gate.get().map(Arc::as_ref)
    }

    pub(in crate::client) fn stage_prepared_outcome(
        &self,
        outcome: Option<crate::client::reconciler::mutation::ApplyOutcome>,
    ) {
        *self.prepared_outcome.lock() = outcome;
    }

    pub(in crate::client) fn stage_connection_effect(&self, effect: CheckedConnectionEffect) {
        *self.checked_connection_effect.lock() = Some(effect);
    }

    pub(in crate::client) fn convergence_core(&self) -> &dyn CoreLifecyclePort {
        self.core.as_ref()
    }

    pub(in crate::client) fn convergence_progress(&self) -> &parking_lot::Mutex<RuntimeProgress> {
        self.progress.as_ref()
    }

    pub(in crate::client) fn canonical_convergence_session(&self) -> bool {
        self.profiles.is_some() && self.checked_gate.get().is_some()
    }

    pub(in crate::client) fn outcome_uncertain(&self) -> bool {
        self.core.outcome_uncertain()
    }

    pub(in crate::client) fn startup_application_config(&self) -> ChimeraAppConfig {
        self.application.get_typed()
    }

    pub(in crate::client) async fn execute(
        &self,
        command: Command,
        #[cfg(test)] startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> anyhow::Result<()> {
        // The sole worker owns one command at a time. Any effect left over
        // from a failed or superseded operation must not leak into the next.
        self.checked_connection_effect.lock().take();
        self.prepared_outcome.lock().take();
        self.progress.lock().clear_pending();
        #[cfg(test)]
        let refresh_startup_retry_identity =
            matches!(&command, Command::PatchApplicationConfig { .. });
        #[cfg(test)]
        let refresh_startup_target = match &command {
            Command::Reconcile
            | Command::ReconcileSource
            | Command::RecoverCore
            | Command::RetryDeferredProfiles { .. }
            | Command::ReconcileProfiles { .. } => true,
            #[cfg(test)]
            Command::SelectCore(_) => true,
            Command::PatchApplicationConfig {
                patch,
                service_connected,
                ..
            } => crate::client::application_workflow::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            ),
            _ => false,
        };
        // Ref converge uses the last proven selection for both explicit
        // starts and observed Core changes, not just source notifications.
        let prepared_selection_candidate =
            matches!(&command, Command::ReconcileSource | Command::Reconcile)
                || (self.profiles.is_some()
                    && self.checked_gate.get().is_some()
                    && matches!(&command, Command::StartCore | Command::RecoverCore));
        if !prepared_selection_candidate && !matches!(&command, Command::ApplyChecked { .. }) {
            self.progress.lock().invalidate_selection();
        }
        let result = match command {
            #[cfg(test)]
            Command::ProfileMutation(_) => {
                anyhow::bail!("Profile mutations must run through RuntimeReconcilerActor")
            }
            Command::RecoverCore => self.reconcile_with_trigger(Trigger::CoreChanged).await,
            Command::Reconcile => self.reconcile_with_trigger(Trigger::Retry).await,
            Command::ReconcileSource => self.reconcile_with_trigger(Trigger::SourceCommitted).await,
            Command::ApplyChecked {
                prepared,
                core_spec,
                domain,
                version,
                authority,
                interrupt_mode,
            } => {
                self.apply_checked_runtime(
                    prepared,
                    core_spec,
                    domain,
                    version,
                    authority,
                    interrupt_mode,
                )
                .await
            }
            Command::StartCore => self.reconcile_with_trigger(Trigger::Start).await,
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
            | Command::RetryStartup { .. }
            | Command::FollowService { .. } => {
                anyhow::bail!("startup commands must be handled by ApplicationWorkflow")
            }
            #[cfg(test)]
            Command::FollowService { .. } => {
                anyhow::bail!("Service readiness follow-up must run through ApplicationWorkflow")
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
            Command::Shutdown => {
                self.service_recovery.lock().suppress();
                self.core.stop().await
            }
            Command::StopCore => self.core.stop().await,
            Command::PatchApplicationConfig {
                patch,
                service_connected,
                degradations,
                runtime_converged,
            } => self
                .patch_application_config_with_result(patch, service_connected)
                .await
                .map(|result| {
                    // Notified source apply is a separate queued Core operation. The
                    // source-command completion cannot mark it Converged.
                    runtime_converged.store(
                        result.notified_version.is_none() && result.degradations.is_empty(),
                        Ordering::Release,
                    );
                    let _ = degradations.send(result);
                }),
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
            #[cfg(test)]
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
            Command::MaintainServiceRelease => self.maintain_service_release().await,
            Command::ServiceEndpointDown(mut transition) => {
                match self.report_service_endpoint_down(transition.as_mut()).await {
                    Ok(recovered) => {
                        if recovered {
                            self.service_recovery_reconnected
                                .store(true, Ordering::Release);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
        };
        if prepared_selection_candidate && result.is_err() {
            self.progress.lock().invalidate_selection();
        }
        self.refresh_service_recovery_host().await;
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

    pub(in crate::client) fn pending_runtime_product_target(&self) -> Option<String> {
        self.core.pending_runtime_product_target()
    }

    pub(in crate::client) async fn reconcile(&self) -> anyhow::Result<()> {
        self.reconcile_with_trigger(Trigger::ServiceOperation).await
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
        // A Stopped Profile TCC must validate the same exact strict product
        // that a later running Try would submit. It is read-only: no Core
        // operation, no Checked inbox entry and no source mutation.
        if let Some(gate) = self.checked_gate.get()
            && self.core.runtime_preparation_session().is_some()
        {
            return gate.validate_stopped_profile_tcc(inputs).await;
        }
        // Production must not build and validate a second runtime candidate
        // through the old CoreLifecycle port. Only the live Core-owned
        // RuntimePreparation/RuntimeGate can check stopped Profile TCC bytes.
        #[cfg(not(test))]
        {
            let _ = inputs;
            Err(crate::client::application_workflow::mutation::RuntimeCheckFailure::
                candidate_unavailable(
                    "Stopped Profile validation requires the installed Core-owned RuntimeGate and preparation session",
                )
                .into())
        }

        // Test fixtures without live Core-owned sessions retain the isolated
        // legacy validator; no production fallback is compiled.
        #[cfg(test)]
        crate::client::reconciler::legacy_check::check_candidate(
            self.core.as_ref(),
            inputs,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        )
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
        // Running Profile TCC cannot safely apply an independently generated
        // YAML/port/IPC revision. Only the live Core-owned ref CandidateCheck
        // may create the precommit product; compatibility ports fail closed.
        let gate = self.checked_gate.get().ok_or_else(|| {
            anyhow::Error::new(
                crate::client::application_workflow::mutation::RuntimeCheckFailure::
                    candidate_unavailable(
                        "Profile TCC requires the installed Core-owned strict candidate gate",
                    ),
            )
        })?;
        if self.core.runtime_preparation_session().is_none() {
            return Err(crate::client::application_workflow::mutation::RuntimeCheckFailure::
                candidate_unavailable(
                    "Profile TCC requires the active Core-owned runtime preparation session",
                )
                .into());
        }
        let run_type = crate::core::RunType::classify(
            inputs.app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        );
        let (prepared, core_spec) =
            gate.prepare_profile_tcc_candidate(inputs)
                .await
                .map_err(|error| {
                    anyhow::Error::new(
                    crate::client::application_workflow::mutation::RuntimeCheckFailure::
                        candidate_unavailable(error.to_string()),
                )
                })?;
        self.core
            .prepare_profile_runtime_prepared(prepared, core_spec, run_type, operation_id.clone())
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

    async fn try_release_service_owner(&self) -> std::result::Result<(), String> {
        CoreLifecycleWorkflow::try_release_service_owner(self)
            .await
            .map(|_| ())
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
        // The owning RuntimeReconcilerActor schedules dirty work after this
        // mutation releases the serialized execution domain.
    }
}

#[cfg(test)]
mod checked_connection_effect_tests {
    use super::*;
    use chimera_contracts::runtime::{DegradationPhase, DegradationReason, InterruptFailure};
    use chimera_core::actor_v2::api::ApiError;

    #[test]
    fn service_prepared_admission_requires_ready_matching_owner_and_real_timestamp() {
        use chimera_core::actor_v2::service_policy::ServicePhase;

        let stopped = CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 7,
            run_type: crate::core::RunType::Service,
        };
        let ready_stopped = ServiceEvidence::CoreStopped {
            ready: true,
            phase: ServicePhase::Ready,
            runtime_owned: true,
        };
        assert!(service_prepared_owner_proven(&stopped, &ready_stopped));
        assert!(!service_prepared_owner_proven(
            &CoreStatusSnapshot {
                state_changed_at: 0,
                ..stopped.clone()
            },
            &ready_stopped
        ));
        assert!(!service_prepared_owner_proven(
            &CoreStatusSnapshot {
                run_type: crate::core::RunType::Normal,
                ..stopped.clone()
            },
            &ready_stopped
        ));
        assert!(!service_prepared_owner_proven(
            &stopped,
            &ServiceEvidence::CoreStopped {
                ready: false,
                phase: ServicePhase::Unknown,
                runtime_owned: true,
            }
        ));
        assert!(!service_prepared_owner_proven(
            &stopped,
            &ServiceEvidence::CoreStopped {
                ready: true,
                phase: ServicePhase::Ready,
                runtime_owned: false,
            }
        ));
        assert!(!service_prepared_owner_proven(
            &stopped,
            &ServiceEvidence::ForeignCore {
                ready: true,
                phase: ServicePhase::Ready,
            }
        ));
        assert!(!service_prepared_owner_proven(
            &stopped,
            &ServiceEvidence::Unreadable {
                phase: ServicePhase::Unknown,
            }
        ));
        let running = CoreStatusSnapshot {
            instance_id: Some("owned-service-instance".into()),
            state: CoreState::Running,
            ..stopped
        };
        let owned = ServiceEvidence::CoreRunning {
            ready: true,
            phase: ServicePhase::Ready,
            instance: Some("owned-service-instance".into()),
            revision: None,
        };
        assert!(service_prepared_owner_proven(&running, &owned));
        assert!(!service_prepared_owner_proven(
            &running,
            &ServiceEvidence::CoreRunning {
                ready: true,
                phase: ServicePhase::Ready,
                instance: Some("foreign-service-instance".into()),
                revision: None,
            }
        ));
        assert!(!service_prepared_owner_proven(&running, &ready_stopped));
    }

    #[test]
    fn core_api_interruption_failure_has_ref_typed_reason_and_wire_code() {
        let cases = [
            (ApiError::Stale, InterruptFailure::Stale),
            (
                ApiError::Unavailable("retired".into()),
                InterruptFailure::Unavailable,
            ),
            (ApiError::Timeout, InterruptFailure::Timeout),
        ];
        for (error, expected) in cases {
            let degradation = CheckedConnectionEffect::from_error(
                SelectionChange::Mode,
                anyhow::Error::new(error),
            );
            assert!(
                matches!(degradation.reason, DegradationReason::ModeInterruptionFailed { cause } if cause == expected)
            );
            assert_eq!(degradation.phase, DegradationPhase::SystemEffect);
            assert!(!degradation.retryable);
            assert_eq!(
                crate::client::runtime::Degradation::from_shared(degradation).code,
                "mode_interruption_failed"
            );
        }
    }

    #[test]
    fn stale_profile_effect_retains_profile_reason_after_source_fence() {
        let effect = CheckedConnectionEffect {
            change: SelectionChange::Profile,
            replaced: Some(false),
            prior: Err(anyhow::Error::new(ApiError::Stale)),
        };
        let degradation = effect.deferred("newer Profiles source reached mailbox");
        assert!(matches!(
            degradation.reason,
            DegradationReason::ProfileInterruptionFailed {
                cause: InterruptFailure::Unavailable
            }
        ));
        assert_eq!(degradation.message, "newer Profiles source reached mailbox");
    }
}

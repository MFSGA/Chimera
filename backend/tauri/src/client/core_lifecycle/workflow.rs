use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_core::{
    actor_v2::service_status::ServiceHostStatus, client::reconciler::plan::Trigger,
};
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
use crate::client::application_workflow::workflow::ProfileRuntime;
use crate::client::reconciler::attempt::{
    AttemptPolicy, CommittedTargetAdvanced, ServiceFallbackDeferred,
};
use crate::client::reconciler::{
    committed::{Selection, SelectionChange},
    inputs::RuntimeInputs,
    legacy_preparation::RuntimePreparation,
};
use crate::config::chimera::ClashCore;
use crate::core::actor_v2::control_endpoint::ExecutionHost;
use crate::enhance::FsProfileContentSource;
use tokio_util::sync::CancellationToken;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    RECOVERY_INTERVAL,
    ports::{
        BinaryInstaller, CoreLifecyclePort, CoreStatusSnapshot, PreparedCoreBinary,
        ServiceLifecyclePort, ServiceTransitionLease,
    },
};

/// The Source commit is final even when the runtime is still pending in the
/// checked Core mailbox. Never report pending checked work as an inline apply.
pub(in crate::client) struct ApplicationPatchResult {
    pub(in crate::client) degradations: Vec<crate::client::runtime::Degradation>,
    /// Exact CAS version associated with the ACKed checked bytes.
    pub(in crate::client) checked_version: Option<u64>,
}

/// Use the ref checked-source handoff only for a local-host enhancement
/// update. Core selection and Service ownership changes still require the
/// specialized legacy handoff until their ref workflows migrate together.
fn checked_application_patch_eligible(
    patch: &ChimeraAppConfigPatch,
    previous: &ChimeraAppConfig,
    service_connected: bool,
) -> bool {
    patch.enable_builtin_enhanced.is_some()
        && patch.core.is_none()
        && patch.enable_service_mode.is_none()
        && !previous.enable_service_mode
        && !service_connected
}

#[cfg(test)]
mod checked_application_route_tests {
    use super::*;

    #[test]
    fn only_local_enhancement_requests_may_use_checked_apply() {
        let mut previous = ChimeraAppConfig::default();
        previous.enable_service_mode = false;
        let mut patch = ChimeraAppConfigPatch {
            enable_builtin_enhanced: Some(!previous.enable_builtin_enhanced),
            ..Default::default()
        };
        assert!(checked_application_patch_eligible(&patch, &previous, false));
        assert!(
            !checked_application_patch_eligible(&patch, &previous, true),
            "a connected Service host retains its dedicated handoff"
        );
        previous.enable_service_mode = true;
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        previous.enable_service_mode = false;
        patch.core = Some(previous.core);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.core = None;
        patch.enable_service_mode = Some(false);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_service_mode = None;
        patch.enable_builtin_enhanced = None;
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
    }
}

/// An older source generation became stale while the single runtime actor
/// was building or applying it. The actor must schedule another latest-source
/// round, but must not blindly retry arbitrary Core apply failures.
#[derive(Debug, thiserror::Error)]
#[error("Profiles source advanced while reconciling runtime: {before} -> {after}")]
pub(crate) struct ProfilesSourceAdvanced {
    before: u64,
    after: u64,
}

pub(in crate::client) enum Command {
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

/// A one-shot post-Core connection effect waiting for the Actor's final
/// source-mailbox fence and Running verification. This does not own Core and
/// cannot be executed by the worker before Completed is accepted.
pub(in crate::client) struct CheckedConnectionEffect {
    change: SelectionChange,
    replaced: Option<bool>,
    prior: anyhow::Result<Option<crate::core::actor_v2::api::ApiClient>>,
}

impl CheckedConnectionEffect {
    fn degradation(
        change: SelectionChange,
        message: String,
        cause: chimera_contracts::runtime::InterruptFailure,
    ) -> chimera_contracts::runtime::Degradation {
        use chimera_contracts::runtime::{Degradation, DegradationPhase, DegradationReason};
        Degradation {
            phase: DegradationPhase::SystemEffect,
            reason: match change {
                SelectionChange::Mode => DegradationReason::ModeInterruptionFailed { cause },
                SelectionChange::Profile => DegradationReason::ProfileInterruptionFailed { cause },
            },
            message,
            retryable: false,
        }
    }

    fn from_error(
        change: SelectionChange,
        error: anyhow::Error,
    ) -> chimera_contracts::runtime::Degradation {
        use chimera_contracts::runtime::InterruptFailure;
        use chimera_core::actor_v2::api::ApiError;
        // Keep REF's typed API failure identity; avoid parsing error strings.
        let cause = match error.downcast_ref::<ApiError>() {
            Some(ApiError::Stale) => InterruptFailure::Stale,
            Some(ApiError::Unavailable(_)) => InterruptFailure::Unavailable,
            Some(ApiError::Timeout) => InterruptFailure::Timeout,
            Some(ApiError::Protocol(_)) => InterruptFailure::Protocol,
            None => InterruptFailure::Unavailable,
        };
        Self::degradation(change, format!("{error:#}"), cause)
    }

    /// An obsolete or unverified Core result must never close the old API;
    /// preserve its non-retryable one-shot effect as typed degradation.
    pub(in crate::client) fn deferred(
        self,
        reason: &str,
    ) -> chimera_contracts::runtime::Degradation {
        Self::degradation(
            self.change,
            reason.to_owned(),
            chimera_contracts::runtime::InterruptFailure::Unavailable,
        )
    }

    pub(in crate::client) async fn finish(self) -> Option<chimera_contracts::runtime::Degradation> {
        match (self.replaced, self.prior) {
            (Some(true), _) => None, // Replaced Core already retired connections.
            (Some(false), Ok(None)) => None, // No previous running Core instance.
            (Some(false), Ok(Some(api))) => {
                let result = match self.change {
                    SelectionChange::Mode => {
                        crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change(&api, true).await
                    }
                    SelectionChange::Profile => {
                        crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(&api, true).await
                    }
                };
                result
                    .err()
                    .map(|error| Self::from_error(self.change, error))
            }
            (Some(false), Err(error)) => Some(Self::from_error(self.change, error)),
            (None, _) => Some(Self::degradation(
                self.change,
                "Core replacement outcome is unknown; connection interruption not attempted".into(),
                chimera_contracts::runtime::InterruptFailure::Unavailable,
            )),
        }
    }
}

#[derive(Clone)]
pub(in crate::client) struct CoreLifecycleWorkflow {
    application: ApplicationClient,
    clash: ClashConfigClient,
    profiles: Option<chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>>,
    content_reader: Option<crate::state::mutation::ProfileContentReader>,
    /// Installed once by the production Client after the canonical Core
    /// session and Profiles gate have been assembled. Shared by clones of
    /// the sole lifecycle workflow, not an independent runtime owner.
    checked_gate: Arc<std::sync::OnceLock<Arc<crate::client::reconciler::check::CandidateCheck>>>,
    /// A baseline exists only after an exact Checked apply and two successful
    /// authoritative source reads. Legacy rebuilds invalidate it because they
    /// do not yet hand back a confirmed applied selection.
    applied_selection: Arc<parking_lot::Mutex<Option<Selection>>>,
    /// A checked source's post-apply Mode effect is held by the same
    /// workflow until its sole Actor has fenced the Completed message.
    checked_connection_effect: Arc<parking_lot::Mutex<Option<CheckedConnectionEffect>>>,
    /// The exact read-only source authority already used by Checked Apply.
    /// Ordinary startup/reconcile may adopt selection only when an active
    /// Core receipt and this authority agree on the full committed target.
    source_authority: Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    core: Arc<dyn CoreLifecyclePort>,
    installer: Arc<dyn BinaryInstaller>,
    service: Arc<dyn ServiceLifecyclePort>,
    startup_owner_unproven: Arc<AtomicBool>,
    #[cfg(test)]
    startup_report: Arc<parking_lot::Mutex<Option<StartupReport>>>,
    service_failed_generation_shared: Arc<AtomicU64>,
    service_status: Option<watch::Receiver<ServiceHostStatus>>,
    service_recovery: Arc<parking_lot::Mutex<ServiceRecovery>>,
    attempt_policy: Arc<parking_lot::Mutex<AttemptPolicy>>,
    service_recovery_reconnected: Arc<AtomicBool>,
    shutdown: CancellationToken,
}

#[derive(Default)]
struct ServiceRecovery {
    attempts: u8,
    suppressed: bool,
    owner: Option<ExecutionHost>,
    next_attempt: Option<tokio::time::Instant>,
}

const SERVICE_RECOVERY_BUDGET: u8 = 3;

impl ServiceRecovery {
    fn armed(&self) -> bool {
        !self.suppressed && self.attempts < SERVICE_RECOVERY_BUDGET
    }

    fn rearm(&mut self) {
        self.attempts = 0;
        self.suppressed = false;
        self.next_attempt = None;
    }

    fn suppress(&mut self) {
        self.suppressed = true;
    }

    fn follow_host(&mut self, host: ExecutionHost) {
        if self.owner == Some(host) {
            return;
        }
        self.owner = Some(host);
        match host {
            ExecutionHost::Service => self.rearm(),
            ExecutionHost::Local => self.suppress(),
        }
    }

    fn due(&self, endpoint_degraded: bool, closing: bool) -> bool {
        self.owner == Some(ExecutionHost::Service)
            && !closing
            && self.armed()
            && endpoint_degraded
            && self
                .next_attempt
                .is_none_or(|at| tokio::time::Instant::now() >= at)
    }

    fn finish_attempt(&mut self, failed: bool, terminal_error: bool, endpoint_degraded: bool) {
        self.next_attempt = Some(tokio::time::Instant::now() + RECOVERY_INTERVAL);
        if terminal_error {
            self.suppressed = true;
        }
        if self.attempts >= SERVICE_RECOVERY_BUDGET && (failed || endpoint_degraded) {
            tracing::warn!(
                "service endpoint recovery budget spent; explicitly start Service to re-arm"
            );
        }
    }
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
            content_reader: None,
            checked_gate: Arc::new(std::sync::OnceLock::new()),
            applied_selection: Arc::new(parking_lot::Mutex::new(None)),
            checked_connection_effect: Arc::new(parking_lot::Mutex::new(None)),
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
            attempt_policy: Arc::new(parking_lot::Mutex::new(AttemptPolicy::default())),
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

    pub(super) fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = shutdown;
        self
    }

    pub(super) fn with_profile_content_reader(
        mut self,
        reader: Option<crate::state::mutation::ProfileContentReader>,
    ) -> Self {
        self.content_reader = reader;
        self
    }

    pub(super) fn with_source_authority(
        mut self,
        authority: Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    ) -> Self {
        self.source_authority = authority;
        self
    }

    pub(super) fn with_service_status(
        mut self,
        service_status: watch::Receiver<ServiceHostStatus>,
    ) -> Self {
        self.service_status = Some(service_status);
        self
    }

    pub(in crate::client) fn invalidate_applied_selection(&self) {
        self.applied_selection.lock().take();
    }

    pub(in crate::client) fn take_checked_connection_effect(
        &self,
    ) -> Option<CheckedConnectionEffect> {
        self.checked_connection_effect.lock().take()
    }

    /// Ref's applied_selection follows an *observed Core result*, not a
    /// successfully saved document. The legacy startup/reconcile actor only
    /// reports Result<()>; its already-confirmed live receipt and the same
    /// bounded source authority used by Checked Apply can prove the selection.
    /// Unknown, stopped, Service-handoff, or advanced sources clear the value.
    pub(in crate::client) async fn recover_applied_selection_from_receipt(&self) {
        let Some(authority) = self.source_authority.as_ref() else {
            self.applied_selection.lock().take();
            return;
        };
        let first = match self.core.active_runtime_receipt().await {
            Ok(Some(receipt)) if receipt.target.is_some() => receipt,
            Ok(_) => {
                self.applied_selection.lock().take();
                return;
            }
            Err(error) => {
                tracing::debug!(%error, "cannot prove active Core for selection recovery");
                self.applied_selection.lock().take();
                return;
            }
        };
        let target = first.target.as_deref().expect("checked above");
        let selection = match authority.selection_for_applied_target(target).await {
            Ok(selection) => selection,
            Err(error) => {
                tracing::debug!(%error, "cannot prove committed sources for selection recovery");
                None
            }
        };
        // Recheck the complete source target across the asynchronous Core
        // observation. A source commit racing this read must not seed a
        // selection that was never proven to match the running bytes.
        let latest_selection = authority
            .selection_for_applied_target(target)
            .await
            .ok()
            .flatten();
        // The source reads were asynchronous. Reject an intervening Core
        // replacement or second apply even if its document target is equal.
        let still_active = self
            .core
            .active_runtime_receipt()
            .await
            .ok()
            .flatten()
            .is_some_and(|latest| {
                latest.target == first.target
                    && latest.host == first.host
                    && latest.applied_revision == first.applied_revision
                    && latest.config_digest == first.config_digest
            });
        *self.applied_selection.lock() = if still_active && selection == latest_selection {
            selection
        } else {
            None
        };
    }

    #[cfg(test)]
    pub(super) fn applied_selection_for_tests(&self) -> Option<Selection> {
        self.applied_selection.lock().clone()
    }

    #[cfg(test)]
    pub(super) fn set_applied_selection_for_tests(&self, selection: Selection) {
        *self.applied_selection.lock() = Some(selection);
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

    /// A refused Service submission may have already applied its Core but
    /// failed *afterwards* while promoting RuntimeProduct. The startup owner
    /// may release Service only after a stable, non-synthetic Stopped state
    /// with no accepted Core receipt. This independent proof also protects
    /// other errors that arrived after a concurrent owner transition.
    pub(in crate::client) async fn startup_service_refusal_stopped(&self) -> bool {
        if self.core.outcome_uncertain() {
            return false;
        }
        let Ok(before) = self.core.status().await else {
            return false;
        };
        if before.run_type != crate::core::RunType::Service
            || before.state_changed_at == 0
            || !matches!(before.state, CoreState::Stopped(_))
        {
            return false;
        }
        if !matches!(self.core.active_runtime_receipt().await, Ok(None)) {
            return false;
        }
        let Ok(after) = self.core.status().await else {
            return false;
        };
        same_core_status(&before, &after)
            && matches!(self.core.active_runtime_receipt().await, Ok(None))
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
        self.core.select_execution_host(host).await?;
        self.follow_service_recovery_host(host);
        Ok(())
    }

    /// Startup S2/S3 established and, if necessary, retired the Local Core
    /// before reaching here. Reuse the exact committed-source preparation
    /// instead of the old inline YAML construction whenever the sole Core
    /// owner and authoritative source reader are available.
    async fn try_startup_prepared_selected(
        &self,
        app: &ChimeraAppConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        if self.profiles.is_none()
            || self.checked_gate.get().is_none()
            || self.source_authority.is_none()
        {
            return None;
        }
        let clash = match self.clash.get() {
            Ok(clash) => clash,
            Err(error) => return Some(Err(error)),
        };
        let (target, app, clash, ..) = match self
            .capture_reconcile_committed_target(app.clone(), clash)
            .await
        {
            Ok(committed) => committed,
            Err(error) => return Some(Err(error)),
        };
        match run_type {
            crate::core::RunType::Normal => {
                self.try_reconcile_prepared_local(&target, &app, &clash, run_type)
                    .await
            }
            crate::core::RunType::Service => {
                self.try_reconcile_prepared_service(&target, &app, &clash)
                    .await
            }
            crate::core::RunType::Elevated => None,
        }
    }

    pub(in crate::client) async fn startup_reconcile_application(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        let selected = if service_connected {
            crate::core::RunType::Service
        } else {
            crate::core::RunType::Normal
        };
        let result = if let Some(result) = self.try_startup_prepared_selected(&app, selected).await
        {
            result
        } else {
            self.reconcile_application(app, service_connected).await
        };
        self.refresh_service_recovery_host().await;
        result
    }

    pub(in crate::client) async fn startup_reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
    ) -> anyhow::Result<()> {
        let result = self.reconcile_application_after_service_release(app).await;
        self.refresh_service_recovery_host().await;
        result
    }

    pub(super) fn service_restart_policy(
        &self,
    ) -> chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
        self.service.restart_policy()
    }

    fn follow_service_recovery_host(&self, host: ExecutionHost) {
        self.service_recovery.lock().follow_host(host);
    }

    async fn refresh_service_recovery_host(&self) {
        let Ok(status) = self.core.status().await else {
            return;
        };
        let host = if status.run_type == crate::core::RunType::Service {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        };
        self.follow_service_recovery_host(host);
    }

    pub(in crate::client) async fn service_endpoint_recovery_due(&self) -> bool {
        if !self.service_recovery_due_now() || !self.settle_service_recovery_action().await {
            return false;
        }
        self.service_recovery_due_now()
    }

    fn service_recovery_due_now(&self) -> bool {
        if !self.core.service_endpoint_recovery_supported() {
            return false;
        }
        self.service_recovery.lock().due(
            self.core.service_endpoint_degraded(),
            self.shutdown.is_cancelled(),
        )
    }

    pub(in crate::client) fn service_recovery_blocks_handoff(&self) -> bool {
        let endpoint_degraded = self.core.service_endpoint_degraded();
        let recovery = self.service_recovery.lock();
        recovery.owner == Some(ExecutionHost::Service) && recovery.armed() && endpoint_degraded
    }

    pub(in crate::client) fn take_service_recovery_reconnected(&self) -> bool {
        self.service_recovery_reconnected
            .swap(false, Ordering::AcqRel)
    }

    async fn settle_service_recovery_action(&self) -> bool {
        match self.core.settle_service_recovery_action().await {
            Some(Ok(())) | None => true,
            Some(Err(reason)) => {
                tracing::debug!(%reason, "Service endpoint recovery deferred until the previous Core action settles");
                false
            }
        }
    }

    async fn report_service_endpoint_down(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<bool> {
        if !self.core.service_endpoint_recovery_supported() {
            self.service.report_endpoint_down(transition).await?;
            return Ok(false);
        }
        if !self.service_recovery_due_now() || !self.settle_service_recovery_action().await {
            return Ok(false);
        }
        let endpoint_degraded = self.core.service_endpoint_degraded();
        {
            let mut recovery = self.service_recovery.lock();
            if !recovery.due(endpoint_degraded, self.shutdown.is_cancelled()) {
                return Ok(false);
            }
            recovery.attempts = recovery.attempts.saturating_add(1);
        }

        let result = self
            .core
            .recover_service_endpoint(&self.shutdown)
            .await
            .unwrap_or_else(|| {
                Err(chimera_core_manager::CoreError::new(
                    chimera_core_manager::CoreErrorKind::BackendUnavailable,
                    "the shared Core router does not support Service endpoint recovery",
                    true,
                ))
            });
        let failed = result.is_err();
        let terminal_error = result.as_ref().err().is_some_and(|error| !error.retryable);
        let endpoint_degraded = self.core.service_endpoint_degraded();
        self.service_recovery
            .lock()
            .finish_attempt(failed, terminal_error, endpoint_degraded);
        if let Err(error) = result {
            tracing::warn!(%error, "Service endpoint recovery attempt failed");
            return Err(anyhow::anyhow!(error.to_string()));
        }
        Ok(true)
    }

    /// Preserve the Core's exact terminal verdict through the sole lower-host
    /// operation registry. RolledBack is a completed refusal with the old
    /// revision still running; Unknown has a Core operation ID and must not
    /// be submitted again. Legacy ports keep their explicit Unobserved result.
    async fn submit_prepared_verdict(
        &self,
        prepared: super::ports::PreparedRuntime,
        spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<super::ports::PreparedCoreSettlement> {
        let verdict = self
            .core
            .reconcile_prepared_submission(prepared, spec)
            .await?;
        let mapped = crate::client::reconciler::apply::prepared_submission_outcome(&verdict);
        match verdict {
            super::ports::PreparedSubmission::Applied(applied) => {
                if let Some(crate::client::reconciler::mutation::ApplyOutcome::Applied(candidate)) =
                    mapped
                {
                    anyhow::ensure!(
                        applied.replaced == Some(candidate.replaced)
                            && applied
                                .product
                                .as_ref()
                                .is_some_and(|product| Arc::ptr_eq(product, &candidate.product)),
                        "upstream AppliedCandidate is not the exact confirmed Core settlement"
                    );
                }
                Ok(applied)
            }
            super::ports::PreparedSubmission::RolledBack(report) => {
                let reason = report
                    .failed_apply
                    .clone()
                    .unwrap_or_else(|| "the host returned no rollback detail".into());
                Err(super::ports::PreparedRolledBack { report, reason }.into())
            }
            super::ports::PreparedSubmission::Unknown(error) => Err(error.into()),
        }
    }

    /// The sole Core owner's checked runtime execution. Carry the pre-apply
    /// source-instance API and Core replacement verdict through this one
    /// operation; never issue a second Core submit or interrupt a newly
    /// started instance.
    async fn apply_checked_runtime(
        &self,
        prepared: super::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        domain: crate::client::reconciler::committed::SourceDomain,
        version: u64,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        interrupt_mode: bool,
    ) -> anyhow::Result<()> {
        let checked_target = prepared
            .target
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("checked runtime has no source target identity"))?
            .to_owned();
        let previous = self.applied_selection.lock().clone();
        let result = async {
            let interruption = if interrupt_mode {
                // Source-owned raw Clash patch has already proved that its
                // durable transition changes mode and requests interruption.
                // Do not depend on a possibly absent startup selection:
                // the one-shot intent belongs to this exact Checked receipt.
                authority.verify(domain, version, &checked_target).await?;
                Some(SelectionChange::Mode)
            } else {
                authority
                    .verify_interruption(domain, version, &checked_target, previous.as_ref())
                    .await?
            };
            // Capture only the pre-apply instance-bound controller. A failed
            // capture is a post-commit effect degradation, not a Core refusal.
            let source = if interruption.is_some() {
                Some(self.core.capture_interruption_source().await)
            } else {
                None
            };
            let applied = self.submit_prepared_verdict(prepared, core_spec).await?;
            applied.validate_identity(&checked_target)?;
            // No stale source may be promoted into applied_selection, and
            // an obsolete Core result cannot settle current runtime effects.
            let selection = authority
                .verify_selection(domain, version, &checked_target)
                .await?;
            Ok::<_, anyhow::Error>((selection, interruption, source, applied.replaced))
        }
        .await;
        match result {
            Ok((selection, interruption, source, replaced)) => {
                *self.applied_selection.lock() = selection;
                // The worker cannot know about source notices that reached
                // the Actor after its final read. Defer the one-shot mode
                // effect to Completed; only that mailbox can settle it.
                if let Some(change) = interruption {
                    *self.checked_connection_effect.lock() = Some(CheckedConnectionEffect {
                        change,
                        replaced,
                        prior: source.unwrap_or(Ok(None)),
                    });
                }
                Ok(())
            }
            Err(error) => {
                self.applied_selection.lock().take();
                Err(error)
            }
        }
    }

    pub(in crate::client) async fn execute(
        &self,
        command: Command,
        #[cfg(test)] startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> anyhow::Result<()> {
        // The sole worker owns one command at a time. Any effect left over
        // from a failed or superseded operation must not leak into the next.
        self.checked_connection_effect.lock().take();
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
        // Only SourceCommitted/Retry may need the last proven selection to
        // compute the prepared Local Core's one-shot connection policy. All
        // other legacy commands invalidate it before their first Core action.
        let prepared_selection_candidate =
            matches!(&command, Command::ReconcileSource | Command::Reconcile);
        if !prepared_selection_candidate && !matches!(&command, Command::ApplyChecked { .. }) {
            self.applied_selection.lock().take();
        }
        let result = match command {
            Command::ProfileMutation(_) => {
                anyhow::bail!("Profile mutations must run through ApplicationWorkflowActor")
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
                    // Checked Apply is a separate queued Core operation. The
                    // source-command completion cannot mark it Converged.
                    runtime_converged.store(
                        result.checked_version.is_none() && result.degradations.is_empty(),
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
            self.applied_selection.lock().take();
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

    async fn uninstall_service(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<()> {
        use chimera_ipc::api::status::CoreState;

        self.service_recovery.lock().suppress();
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
        self.service_recovery.lock().rearm();
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

        self.service_recovery.lock().suppress();
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

    /// Independent ref release-owed maintenance: only a verified Local
    /// runtime may retire a daemon whose *stopped* Core is still owned by this
    /// instance. Running, foreign, quarantined and unreadable Service owners
    /// are never grounds for stopping the daemon.
    pub(super) async fn service_release_observation(
        &self,
    ) -> crate::client::reconciler::release::ReleaseObservation {
        use crate::client::reconciler::release::{ReleaseObservation, observation};

        if self.application.get_typed().enable_service_mode {
            return ReleaseObservation::Cleared;
        }
        let Ok(core) = self.core.status().await else {
            return ReleaseObservation::Unproven;
        };
        let verified_local = core.run_type == crate::core::RunType::Normal
            && self.verified_applied_runtime(&core).await;
        if !verified_local {
            return ReleaseObservation::Unproven;
        }
        let (service, _) = self.service_evidence().await;
        observation(false, verified_local, &service)
    }

    pub(super) async fn maintain_service_release(&self) -> anyhow::Result<()> {
        use crate::client::reconciler::release::ReleaseObservation;

        let before = self.core.status().await?;
        anyhow::ensure!(
            self.service_release_observation().await == ReleaseObservation::Owed,
            "Service daemon release is not authorized by a verified Local owner"
        );
        // Nonblocking lock acquisition preserves ServiceActor -> CoreLifecycle
        // ordering: a UI command can hold this lock while awaiting our mailbox.
        let mut lease = self.try_begin_service_transition().await?;
        anyhow::ensure!(
            self.service_release_observation().await == ReleaseObservation::Owed,
            "Service owner evidence changed after acquiring the transition lease"
        );
        // An intentionally retired daemon must not be restarted by the
        // separate endpoint-recovery loop on the next interval.
        self.service_recovery.lock().suppress();
        lease.stop_daemon().await?;
        lease.confirm_stopped().await?;
        let (after, _) = self.service_evidence().await;
        anyhow::ensure!(
            matches!(after, ServiceEvidence::Absent { .. }),
            "Service stop was acknowledged but a fresh probe did not prove daemon shutdown: {after:?}"
        );
        let current = self.core.status().await?;
        anyhow::ensure!(
            before.instance_id == current.instance_id
                && before.state_changed_at == current.state_changed_at
                && before.run_type == current.run_type
                && matches!(current.state, CoreState::Running),
            "Local Core changed while retiring the Service daemon"
        );
        self.refresh_service_recovery_host().await;
        Ok(())
    }

    pub(super) fn pending_runtime_product_target(&self) -> Option<String> {
        self.core.pending_runtime_product_target()
    }

    /// Ref's applied-target proof: the Core must report the exact receipt
    /// for the current committed inputs and selected host, then remain the
    /// same instance through a second authoritative observation.
    pub(super) async fn verified_applied_runtime(&self, observed: &CoreStatusSnapshot) -> bool {
        if !matches!(observed.state, CoreState::Running) {
            return false;
        }
        let target = match self.startup_target_identity().await {
            Ok(target) => target,
            Err(error) => {
                tracing::debug!(%error, "could not identify committed runtime target for proof");
                return false;
            }
        };
        let first = match self.core.active_runtime_receipt().await {
            Ok(Some(receipt)) => receipt,
            Ok(None) => return false,
            Err(error) => {
                tracing::debug!(%error, "could not verify active Core receipt");
                return false;
            }
        };
        if !crate::client::reconciler::proof::receipt_matches_target(
            &first,
            &target,
            crate::client::reconciler::proof::host_for(observed.run_type),
        ) {
            return false;
        }
        let after = match self.core.status().await {
            Ok(after) => after,
            Err(_) => return false,
        };
        if !same_core_status(observed, &after) {
            return false;
        }
        let still_active = match self.core.active_runtime_receipt().await {
            Ok(Some(current)) => Arc::ptr_eq(&first, &current),
            _ => false,
        };
        if !still_active {
            return false;
        }
        self.startup_target_identity()
            .await
            .is_ok_and(|latest| latest == target)
    }

    async fn reconcile(&self) -> anyhow::Result<()> {
        self.reconcile_with_trigger(Trigger::ServiceOperation).await
    }

    /// Upstream Step::FallBackToLocal, adapted to Chimera's existing host
    /// transition owner. A rejected Service target may fall back only after a
    /// stable Stopped observation and an instance-bound Service release. The
    /// Local apply re-captures committed inputs rather than replaying the old
    /// pre-commit candidate. No second host controller is created here.
    pub(super) async fn fallback_after_service_refusal(
        &self,
        target: &str,
        refusal: anyhow::Error,
    ) -> anyhow::Result<()> {
        // A Service-to-Local fallback is authorized only by a *refused*
        // Service submission. Unknown and Waiting outcomes cannot prove
        // the Service owner stopped safely and must retain the original
        // reason for the single Actor to retry or recover.
        if !matches!(
            crate::client::reconciler::attempt::legacy_error_verdict(
                &refusal,
                self.core.outcome_uncertain(),
                false,
            ),
            crate::client::reconciler::attempt::LegacyApplyVerdict::Refused(_)
        ) {
            return Err(refusal);
        }
        if !self.startup_service_refusal_stopped().await {
            return Err(refusal);
        }

        // Keep the nonblocking Service transition lease across both release
        // and the Local apply. Dropping it between those operations would let
        // another owner start Service after the release proof but before Local
        // finished, violating ref's single-owner handoff ordering.
        let _transition =
            self.try_begin_service_transition()
                .await
                .map_err(|error| ServiceFallbackDeferred {
                    reason: format!(
                        "{refusal}; cannot acquire Service transition lease: {error:#}"
                    ),
                })?;
        let (_, generation) = self.service_evidence().await;
        let released =
            self.release_service_owner()
                .await
                .map_err(|reason| ServiceFallbackDeferred {
                    reason: format!("{refusal}; {reason}"),
                })?;
        if !released.proves_no_owned_core() {
            return Err(ServiceFallbackDeferred {
                reason: format!(
                    "{refusal}; no fresh evidence that Service released its owned Core"
                ),
            }
            .into());
        }
        // Only suppress Service for this readiness generation after its Core
        // release has been proved. A failed or unknown release MUST NOT make
        // an ordinary Local start eligible.
        if generation != 0 {
            self.set_startup_failed_generation(generation);
        }
        if !self
            .startup_target_identity()
            .await
            .is_ok_and(|current| current == target)
        {
            return Err(CommittedTargetAdvanced.into());
        }

        let local = self
            .reconcile_application_after_service_release(self.application.get_typed())
            .await;
        self.refresh_service_recovery_host().await;
        self.attempt_policy.lock().record(
            target.to_owned(),
            ExecutionHost::Local,
            &local,
            self.core.outcome_uncertain(),
        );
        if !self
            .startup_target_identity()
            .await
            .is_ok_and(|current| current == target)
        {
            return Err(CommittedTargetAdvanced.into());
        }
        local
    }

    /// Treat Service failure for one readiness generation as a Local
    /// preference only after the previous Service Core was safely released.
    async fn committed_run_type(&self, enable_service_mode: bool) -> crate::core::RunType {
        let preferred = crate::core::RunType::classify(
            enable_service_mode,
            crate::core::service::ipc::get_ipc_state(),
        );
        if preferred != crate::core::RunType::Service {
            return preferred;
        }
        let failed_generation = self.startup_failed_generation();
        if failed_generation == 0 {
            return preferred;
        }
        let (_, current_generation) = self.service_evidence().await;
        if failed_generation == current_generation {
            crate::core::RunType::Normal
        } else {
            preferred
        }
    }

    /// Execute only those decisions of ref's S3/S4 plan for which Chimera
    /// has established the selected host and has a live runtime receipt.
    /// Ownership moves continue through the single startup handoff actor.
    /// Returning Some means this round has already been handled; None means
    /// the existing Core submission path should execute.
    async fn planned_committed_round(
        &self,
        target: &str,
        host: ExecutionHost,
        trigger: Trigger,
    ) -> anyhow::Result<Option<anyhow::Result<()>>> {
        use chimera_core::client::reconciler::plan::{Conclusion, Step};

        // Do not turn a possibly unknown in-flight Core operation into a
        // second stop or submit. The actor's uncertain gate remains in charge.
        if self.core.outcome_uncertain()
            || !matches!(trigger, Trigger::SourceCommitted | Trigger::Retry)
        {
            return Ok(None);
        }
        let observed = match self.core.status().await {
            Ok(observed) => observed,
            Err(_) => return Ok(None),
        };
        let receipt = self.core.active_runtime_receipt().await.ok().flatten();
        let verified = if receipt.is_some() && matches!(observed.state, CoreState::Running) {
            self.verified_applied_runtime(&observed).await
        } else {
            false
        };
        let previous = self.attempt_policy.lock().snapshot();
        // Read the same ServiceActor-owned evidence as StartupWorkflow. Its
        // epoch/readiness and the selected Core state form ONE ref Facts round
        // instead of permitting S3 to bypass a pending S2 owner transition.
        let (service, _) = self.service_evidence().await;
        // The Service probe is asynchronous. A Core instance or routing
        // generation change during that await invalidates the paired Facts
        // sample; never stop/apply based on observations from two owners.
        let resampled = self.core.status().await.ok();
        if !resampled
            .as_ref()
            .is_some_and(|current| same_core_status(&observed, current))
        {
            return Ok(Some(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "the selected Core changed while sampling Service ownership",
                )
                .into(),
            )));
        }
        if let Some(original) = receipt.as_ref()
            && !self
                .core
                .active_runtime_receipt()
                .await
                .ok()
                .flatten()
                .is_some_and(|current| Arc::ptr_eq(original, &current))
        {
            return Ok(Some(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "the accepted Core revision changed while sampling Service ownership",
                )
                .into(),
            )));
        }
        if host == ExecutionHost::Service && matches!(service, ServiceEvidence::ForeignCore { .. })
        {
            return Ok(Some(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "Service belongs to another instance; Local owner selection is required",
                )
                .into(),
            )));
        }
        let step = crate::client::reconciler::decision::plan_committed_target(
            target,
            host,
            &observed,
            crate::client::application_workflow::startup::reconciler_service_evidence(&service),
            self.service_recovery_blocks_handoff(),
            receipt.as_deref(),
            verified,
            previous,
            trigger,
        );
        match step {
            Some(Step::Idle(Conclusion::Converged)) => Ok(Some(Ok(()))),
            Some(Step::Owner(owner_plan)) => Ok(Some(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::owner(owner_plan)
                    .into(),
            ))),
            Some(Step::Idle(Conclusion::Waiting { cause })) => Ok(Some(Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    cause.to_string(),
                )
                .into(),
            ))),
            Some(Step::Stop { intent: false }) => {
                // A live Core with the wrong accepted revision must stop
                // before we submit this different committed target.
                self.core.stop().await?;
                Ok(None)
            }
            Some(Step::FallBackToLocal) => {
                let refusal =
                    anyhow::anyhow!("previously refused committed Service target: {target}");
                Ok(Some(
                    self.fallback_after_service_refusal(target, refusal).await,
                ))
            }
            _ => Ok(None),
        }
    }

    /// Ref's prepared submission for an S2-selected and S3-proven Core owner.
    /// Startup and Service handoffs still use their explicit S2 transitions:
    /// a prepared submission may *not* select or retire a Core host.
    ///
    /// This returns None only before touching Core, when the canonical
    /// session or the selected Local owner is unavailable. A proven stopped
    /// Local owner has no old connections to interrupt; a live Local owner
    /// requires both an instance-bound receipt and an applied selection. After admission,
    /// all errors are returned to the one Actor as a real apply outcome;
    /// no second legacy submit may replay the candidate.
    pub(super) async fn try_reconcile_prepared_local(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        if run_type != crate::core::RunType::Normal {
            return None;
        }
        self.try_reconcile_prepared_selected(target, app, clash, run_type)
            .await
    }

    /// The selected Service endpoint uses the same exact PreparedRuntime
    /// product and Core manager operation as Local. Only S2-proven Service
    /// readiness can authorize admission; no host selection occurs here.
    async fn try_reconcile_prepared_service(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
    ) -> Option<anyhow::Result<()>> {
        self.try_reconcile_prepared_selected(target, app, clash, crate::core::RunType::Service)
            .await
    }

    async fn try_reconcile_prepared_selected(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        if self.core.outcome_uncertain() {
            return None;
        }
        let authority = self.source_authority.as_ref()?;
        let gate = self.checked_gate.get()?;
        let before = self.core.status().await.ok()?;
        if before.run_type != run_type {
            return None;
        }
        // An unverified Service probe is not proof of a stopped Core. This
        // check is independent from the Core router's status projection.
        let service_before = if run_type == crate::core::RunType::Service {
            let (service, generation) = self.service_evidence().await;
            if !service_prepared_owner_proven(&before, &service) {
                return Some(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "Service owner is not Ready or does not match the selected Core state",
                    )
                    .into(),
                ));
            }
            Some((service, generation))
        } else {
            None
        };
        let current_receipt = self.core.active_runtime_receipt().await.ok()?;
        let (receipt, previous) = match &before.state {
            CoreState::Running => {
                let receipt = current_receipt?;
                let expected_host = match run_type {
                    crate::core::RunType::Normal => ExecutionHost::Local,
                    crate::core::RunType::Service => ExecutionHost::Service,
                    crate::core::RunType::Elevated => return None,
                };
                if receipt.host != expected_host {
                    return None;
                }
                (Some(receipt), Some(self.applied_selection.lock().clone()?))
            }
            CoreState::Stopped(_) if current_receipt.is_none() => (None, None),
            _ => return None,
        };

        let expected = match authority
            .verify_committed_prepared_target(target, previous.as_ref())
            .await
        {
            Ok(Some(expected)) => expected,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let result = async {
            let (prepared, core_spec) = gate
                .prepare_committed_target(app.clone(), clash.clone(), target)
                .await?;
            let current = authority
                .verify_committed_prepared_target(target, previous.as_ref())
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "committed source authority cannot prove the prepared Local target"
                    )
                })?;
            if current != expected {
                return Err(anyhow::Error::new(CommittedTargetAdvanced).context(
                    "committed selection or interruption policy advanced during runtime build",
                ));
            }
            let observed = self.core.status().await?;
            if let Some((service_before, generation_before)) = service_before.as_ref() {
                let (service_after, generation_after) = self.service_evidence().await;
                if generation_after != *generation_before
                    || service_after != *service_before
                    || !service_prepared_owner_proven(&observed, &service_after)
                {
                    return Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "Service readiness or Core ownership changed while building the committed product",
                        )
                        .into(),
                    );
                }
            }
            let latest_receipt = self.core.active_runtime_receipt().await?;
            let still_receipted = match (receipt.as_ref(), latest_receipt.as_ref()) {
                (Some(before), Some(after)) => Arc::ptr_eq(before, after),
                (None, None) => true,
                _ => false,
            };
            if !same_core_status(&before, &observed) || !still_receipted {
                return Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "selected Core owner changed while preparing the committed product",
                    )
                    .into(),
                );
            }
            let old_api = if expected.1.is_some() {
                Some(self.core.capture_interruption_source().await)
            } else {
                None
            };
            let applied = self.submit_prepared_verdict(prepared, core_spec)
                .await?;
            applied.validate_identity(target)?;
            let latest = authority
                .verify_committed_prepared_target(target, previous.as_ref())
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("committed source authority was lost after Core apply")
                })?;
            if latest != expected {
                return Err(anyhow::Error::new(CommittedTargetAdvanced)
                    .context("committed selection or policy advanced after prepared Core apply"));
            }
            *self.applied_selection.lock() = Some(expected.0);
            if let Some(change) = expected.1 {
                *self.checked_connection_effect.lock() = Some(CheckedConnectionEffect {
                    change,
                    replaced: applied.replaced,
                    prior: old_api.unwrap_or(Ok(None)),
                });
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            self.applied_selection.lock().take();
        }
        Some(result)
    }

    async fn reconcile_with_trigger(&self, trigger: Trigger) -> anyhow::Result<()> {
        // Ref pending-product maintenance happens before rebuilding the target.
        // A manual Retry may repair the *same* confirmed runtime publication
        // without a second Core apply. Source notifications and ordinary
        // readiness ticks never replay a stale pending snapshot.
        if trigger == Trigger::Retry
            && !self.core.outcome_uncertain()
            && let Some(pending) = self.core.pending_runtime_product_target()
            && self
                .startup_target_identity()
                .await
                .is_ok_and(|current| current == pending)
            && self
                .core
                .republish_pending_runtime_product(&pending)
                .await?
        {
            return Ok(());
        }
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let target_core = app.core;
        let run_type = self.committed_run_type(app.enable_service_mode).await;
        if let Some(profiles) = &self.profiles {
            let before = *profiles.load().version.as_ref();
            // Capture both the authoritative committed document and its managed
            // file bytes under the same publication fence, as in ref.
            let (target, app, clash, current, content) =
                self.capture_reconcile_committed_target(app, clash).await?;
            let captured = *profiles.load().version.as_ref();
            if captured != before {
                return Err(ProfilesSourceAdvanced {
                    before,
                    after: captured,
                }
                .into());
            }
            let run_type = self.committed_run_type(app.enable_service_mode).await;
            let host = match run_type {
                crate::core::RunType::Service => ExecutionHost::Service,
                crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                    ExecutionHost::Local
                }
            };
            let planned = self.planned_committed_round(&target, host, trigger).await?;
            let handled_by_plan = planned.is_some();
            let applied = if let Some(result) = planned {
                self.applied_selection.lock().take();
                result
            } else {
                let admitted = self.attempt_policy.lock().allow(&target, host, trigger);
                match admitted {
                    Ok(()) => {
                        let prepared = match run_type {
                            crate::core::RunType::Normal => {
                                self.try_reconcile_prepared_local(&target, &app, &clash, run_type)
                                    .await
                            }
                            crate::core::RunType::Service => {
                                self.try_reconcile_prepared_service(&target, &app, &clash)
                                    .await
                            }
                            crate::core::RunType::Elevated => None,
                        };
                        let applied = if let Some(prepared) = prepared {
                            prepared
                        } else {
                            self.applied_selection.lock().take();
                            self.core
                                .reconcile_profiles(
                                    clash,
                                    app.core,
                                    run_type,
                                    current,
                                    app,
                                    content,
                                    Some(target.clone()),
                                )
                                .await
                        };
                        self.attempt_policy.lock().record(
                            target.clone(),
                            host,
                            &applied,
                            self.core.outcome_uncertain(),
                        );
                        applied
                    }
                    Err(previous_refusal) => Err(previous_refusal.into()),
                }
            };
            let applied = if host == ExecutionHost::Service && !handled_by_plan {
                match applied {
                    Ok(()) => Ok(()),
                    Err(refusal) => self.fallback_after_service_refusal(&target, refusal).await,
                }
            } else {
                applied
            };
            let after = *profiles.load().version.as_ref();
            if after != captured {
                return Err(ProfilesSourceAdvanced {
                    before: captured,
                    after,
                }
                .into());
            }
            applied
        } else {
            let target = self.startup_source_identity(None)?;
            let host = match run_type {
                crate::core::RunType::Service => ExecutionHost::Service,
                crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                    ExecutionHost::Local
                }
            };
            let planned = self.planned_committed_round(&target, host, trigger).await?;
            let handled_by_plan = planned.is_some();
            let result = if let Some(result) = planned {
                self.applied_selection.lock().take();
                result
            } else {
                let admitted = self.attempt_policy.lock().allow(&target, host, trigger);
                match admitted {
                    Ok(()) => {
                        self.applied_selection.lock().take();
                        let result = self.core.reconcile(clash, target_core, run_type).await;
                        self.attempt_policy.lock().record(
                            target.clone(),
                            host,
                            &result,
                            self.core.outcome_uncertain(),
                        );
                        result
                    }
                    Err(previous_refusal) => Err(previous_refusal.into()),
                }
            };
            if host == ExecutionHost::Service && !handled_by_plan {
                match result {
                    Ok(()) => Ok(()),
                    Err(refusal) => self.fallback_after_service_refusal(&target, refusal).await,
                }
            } else {
                result
            }
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
        let target = inputs.checked_target_key()?;
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
        let source = FsProfileContentSource::new(crate::utils::dirs::app_profiles_dir()?);
        RuntimePreparation::new(
            self.content_reader.as_ref(),
            self.profiles.as_ref(),
            &source,
        )
        .capture_inputs(app, clash, profiles, staged_content)
        .await
    }

    /// Ref: sample committed Profiles and capture all managed file inputs
    /// under one fence. The old workflow otherwise could pair a promoted file
    /// with a snapshot from before its successful source commit.
    async fn capture_committed_profile_inputs_for(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
    ) -> anyhow::Result<RuntimeInputs> {
        if self.content_reader.is_none() && self.profiles.is_none() {
            anyhow::bail!("Profile state is unavailable for committed runtime capture");
        }
        let source = FsProfileContentSource::new(crate::utils::dirs::app_profiles_dir()?);
        RuntimePreparation::new(
            self.content_reader.as_ref(),
            self.profiles.as_ref(),
            &source,
        )
        .capture_committed_inputs(app, clash)
        .await
    }

    async fn capture_reconcile_committed_target(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
    ) -> anyhow::Result<(
        String,
        ChimeraAppConfig,
        chimera_config::clash::config::ClashConfig,
        Arc<chimera_config::profile::Profiles>,
        std::collections::BTreeMap<String, String>,
    )> {
        let inputs = self
            .capture_committed_profile_inputs_for(app, clash)
            .await?;
        let target = inputs.checked_target_key()?;
        let (app, clash, profiles, content) = inputs.into_parts()?;
        Ok((target, app, clash, profiles, content))
    }

    pub(in crate::client) async fn startup_target_identity(&self) -> anyhow::Result<String> {
        if self.profiles.is_some() {
            return self
                .capture_committed_profile_inputs_for(
                    self.application.get_typed(),
                    self.clash.get()?,
                )
                .await?
                .checked_target_key();
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

    pub(in crate::client) async fn try_release_service_owner(
        &self,
    ) -> Result<ServiceEvidence, String> {
        let _transition = self.service.try_begin_transition().await.map_err(|error| {
            format!("could not acquire the Service transition lease: {error:#}")
        })?;
        let result = self.release_service_owner().await;
        self.refresh_service_recovery_host().await;
        result
    }

    pub(in crate::client) async fn try_begin_service_transition(
        &self,
    ) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.service.try_begin_transition().await
    }

    pub(in crate::client) async fn service_evidence(&self) -> (ServiceEvidence, u64) {
        let probe = self.service.probe().await;
        if let Some(status) = self.service.actor_status() {
            // Production uses ref ServiceActor for both the ownership evidence
            // and the readiness generation. Do NOT derive another generation
            // from CoreLifecycle's independent probe cache; it can disagree
            // across a daemon handoff or failed startup attempt.
            let generation = status.ready_generation;
            let evidence = if probe.is_ok() {
                ServiceEvidence::from_status(&status)
            } else {
                // An unanswered ref Actor probe is not a stop or Ready proof,
                // even if a previous watch snapshot was healthy.
                ServiceEvidence::Unreadable {
                    phase: chimera_core::actor_v2::service_policy::ServicePhase::Unknown,
                }
            };
            return (evidence, generation);
        }
        let Ok(info) = probe else {
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
        // Legacy-only tests keep their injected probe/watch and restart-policy
        // semantics. No second read is performed against a production host.
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

    /// The ref source transition: a checked target is persisted before any
    /// running Core mutation. Only CoreLifecycle owns the later apply; no
    /// independent actor or second runtime writer is spawned by this path.
    #[cfg(test)]
    pub(super) async fn patch_application_config(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<Vec<crate::client::runtime::Degradation>> {
        Ok(self
            .patch_application_config_with_result(patch, service_connected)
            .await?
            .degradations)
    }

    pub(super) async fn patch_application_config_with_result(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<ApplicationPatchResult> {
        if !crate::client::application_workflow::impact::application_patch_requires_reconcile(
            &patch,
            service_connected,
        ) {
            self.application.patch_typed(patch).await?;
            return Ok(ApplicationPatchResult {
                degradations: Vec::new(),
                checked_version: None,
            });
        }

        let previous = self.application.get().await?;
        let use_checked = self.checked_gate.get().is_some()
            && checked_application_patch_eligible(&patch, &previous.state, service_connected);
        let mut candidate = previous.state.clone();
        candidate.apply(patch);
        let prepared = self.application.prepare_replace(candidate.clone()).await?;

        // The ref check ACKs *exact* prepared bytes with the one Core owner.
        // For an eligible local-host update we will notify CheckedSourceCommitted
        // after durable CAS; all host switch paths retain their old apply.
        let checked_request = self
            .check_application_candidate(candidate.clone(), service_connected, use_checked)
            .await?;

        let committed = self
            .application
            .replace_prepared_if_version(previous.version, prepared)
            .await;
        let committed_version = match committed {
            Ok(crate::state::ConditionalReplaceResult::Replaced(snapshot)) => snapshot.version,
            Ok(crate::state::ConditionalReplaceResult::Conflict { actual_version }) => {
                if let Some(request) = checked_request.as_ref() {
                    self.checked_gate
                        .get()
                        .expect("checked Application gate admitted this proposal")
                        .abort_with_receipt(
                            chimera_contracts::mutation::ConfigDomain::Application,
                            request.clone(),
                        );
                }
                anyhow::bail!(
                    "application config version conflict: expected {}, actual {actual_version}",
                    previous.version
                );
            }
            Err(error) => {
                if let Some(request) = checked_request.as_ref() {
                    self.checked_gate
                        .get()
                        .expect("checked Application gate admitted this proposal")
                        .abort_with_receipt(
                            chimera_contracts::mutation::ConfigDomain::Application,
                            request.clone(),
                        );
                }
                return Err(error.context("failed to commit checked application config"));
            }
        };

        if use_checked {
            // Like the upstream source owner, publish the exact durable CAS
            // version immediately. The one CoreLifecycle Actor receives the
            // checked notice while this command is active, and dispatches
            // ApplyChecked only after this source command finishes. No UI
            // callback is needed to make a successful source commit observable.
            let gate = self
                .checked_gate
                .get()
                .expect("checked gate admitted this commit");
            gate.committed_with_receipt(
                chimera_contracts::mutation::ConfigDomain::Application,
                chimera_core::state::Version::new(committed_version),
                checked_request.expect("admitted checked Application has a receipt"),
            );
            // Do not run the legacy inline reconcile on this same version.
            return Ok(ApplicationPatchResult {
                degradations: Vec::new(),
                checked_version: Some(committed_version),
            });
        }

        // Source publication is complete. Ref's committed-target reconciler
        // always consumes the latest version; until the full actor is ported,
        // fence this compatibility apply before and after CoreLifecycle work.
        // Failure to read the source after commit is deferred, not rolled back.
        let degradation = crate::client::reconciler::committed::apply_committed(
            "application",
            committed_version,
            || async { self.application.get().await.map(|latest| latest.version) },
            || self.reconcile_application(candidate, service_connected),
        )
        .await;
        if let Some(degradation) = degradation {
            tracing::warn!(
                code = %degradation.code,
                message = %degradation.message,
                "application source committed; runtime reconcile deferred"
            );
            Ok(ApplicationPatchResult {
                degradations: vec![degradation],
                checked_version: None,
            })
        } else {
            Ok(ApplicationPatchResult {
                degradations: Vec::new(),
                checked_version: None,
            })
        }
    }

    /// Independent ref-style advisory check. The document and managed bytes
    /// are frozen under the ProfileContentReader fence. An unsupported or
    /// unavailable checker is not proof that the candidate is valid.
    async fn check_application_candidate(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
        use_checked: bool,
    ) -> anyhow::Result<Option<String>> {
        use crate::state::mutation::{DomainChange, RuntimeCandidate};

        if self.profiles.is_none() {
            // Only isolated test adapters may omit the authoritative Profiles
            // domain. Production must never silently waive a Core check.
            #[cfg(test)]
            return Ok(None);
            #[cfg(not(test))]
            anyhow::bail!("Application preflight requires the typed Profiles source");
        }
        let candidate = RuntimeCandidate {
            change: DomainChange::Application {
                candidate: Arc::new(app.clone()),
            },
            staged: Default::default(),
        };
        if let Some(gate) = self.checked_gate.get() {
            if use_checked {
                // The ACK proves that one Core owner retained the exact
                // validated candidate before the source is made durable.
                return gate
                    .check_with_receipt(candidate)
                    .await
                    .map(Some)
                    .map_err(anyhow::Error::new);
            }
            // Core selection and Service mode still use legacy postcommit
            // transitions; avoid an orphaned Checked inbox entry for them.
            return gate
                .validate_only(candidate)
                .await
                .map(|()| None)
                .map_err(anyhow::Error::new);
        }
        #[cfg(not(test))]
        {
            let _ = service_connected;
            anyhow::bail!("Application preflight requires the installed production CandidateCheck");
        }
        // Isolated legacy workflow tests have no canonical runtime session.
        #[cfg(test)]
        crate::client::reconciler::legacy_check::check_domain_candidate(
            self.core.as_ref(),
            self.application.get_typed(),
            self.clash.get()?,
            self.profiles
                .as_ref()
                .expect("checked Profiles availability"),
            self.content_reader.as_ref(),
            candidate,
            service_connected,
        )
        .await
        .map(|()| None)
        .map_err(anyhow::Error::new)
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
        let (target, app, clash, profiles, staged_content) =
            self.capture_reconcile_committed_target(app, clash).await?;
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
        if self.profiles.is_none() {
            return self
                .core
                .reconcile_after_service_release(clash, app.core)
                .await;
        }
        let (target, app, clash, profiles, staged_content) =
            self.capture_reconcile_committed_target(app, clash).await?;

        if self.checked_gate.get().is_some()
            && let Some(authority) = self.source_authority.as_ref()
            && self.core.runtime_preparation_session().is_some()
        {
            anyhow::ensure!(
                !self.core.outcome_uncertain(),
                "Service-to-Local handback cannot begin while the Core outcome is uncertain"
            );
            // A newer committed source may have reversed the preferred host
            // while the Service release was being proved. Do not change the
            // host at all for a target that has already been superseded.
            anyhow::ensure!(
                authority
                    .verify_committed_prepared_target(&target, None)
                    .await?
                    .is_some(),
                "authoritative source cannot prove the released Service fallback target"
            );
            // The caller holds the transition lease and has *already* proved
            // that the rejected Service Core was released. Preserve this
            // evidence when selecting Local; do not submit via the router's
            // old combined host-handoff + inline-build path.
            self.core
                .select_local_after_confirmed_service_release()
                .await?;
            // Once S2 has moved the owner, missing Core/source proof must be
            // Waiting, not a license for a second legacy submit or handoff.
            return self
                .try_reconcile_prepared_local(&target, &app, &clash, crate::core::RunType::Normal)
                .await
                .unwrap_or_else(|| {
                    Err(crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "Local Core ownership was not proven after the Service release handback",
                    )
                    .into())
                });
        }

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
        let inputs = self
            .capture_committed_profile_inputs_for(self.application.get_typed(), self.clash.get()?)
            .await?;
        if inputs.checked_target_key()? != expected_digest {
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

    #[cfg(test)]
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

/// S2-proven Service ownership must also describe the *same* selected Core
/// lifecycle state before a prepared product is admitted. A synthetic
/// timestamp-zero Stopped status or a ready but foreign daemon is not proof.
fn service_prepared_owner_proven(observed: &CoreStatusSnapshot, service: &ServiceEvidence) -> bool {
    if observed.run_type != crate::core::RunType::Service || observed.state_changed_at == 0 {
        return false;
    }
    match (service, &observed.state) {
        (
            ServiceEvidence::CoreStopped {
                ready: true,
                runtime_owned: true,
                ..
            },
            CoreState::Stopped(_),
        ) => true,
        (
            ServiceEvidence::CoreRunning {
                ready: true,
                instance,
                ..
            },
            CoreState::Running,
        ) => instance
            .as_ref()
            .is_none_or(|expected| observed.instance_id.as_deref() == Some(expected.as_str())),
        _ => false,
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
        let target = inputs.checked_target_key().map_err(|error| {
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
        // The owning ApplicationWorkflowActor schedules dirty work after this
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

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
use tokio_util::sync::CancellationToken;

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, runtime::RuntimePaths,
    },
    ports::{
        BinaryInstaller, CoreLifecyclePort, CoreStatusSnapshot, ServiceLifecyclePort,
        ServiceTransitionLease,
    },
};

pub(in crate::client) use crate::client::reconciler::legacy_source::ApplicationPatchResult;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(in crate::client) type Command = chimera_core::client::core_lifecycle::command::Command<
    crate::client::system_proxy::SystemProxyClient,
>;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(in crate::client) type Command = chimera_core::client::core_lifecycle::command::Command<()>;

// ref apply_prepared owns post-Core one-shot effects. The sole lifecycle
// Actor still decides when its final source/version fence permits completion.
pub(in crate::client) use crate::client::reconciler::apply::CheckedConnectionEffect;

#[derive(Clone)]
pub(in crate::client) struct CoreLifecycleWorkflow {
    shared: chimera_core::client::core_lifecycle::state::CoreLifecycleState,
    #[cfg(test)]
    startup_report: Arc<parking_lot::Mutex<Option<StartupReport>>>,
}

impl std::ops::Deref for CoreLifecycleWorkflow {
    type Target = chimera_core::client::core_lifecycle::state::CoreLifecycleState;
    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl std::ops::DerefMut for CoreLifecycleWorkflow {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.shared
    }
}

impl CoreLifecycleWorkflow {
    pub(in crate::client) fn shared_state(
        &self,
    ) -> &chimera_core::client::core_lifecycle::state::CoreLifecycleState {
        &self.shared
    }

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
            shared: chimera_core::client::core_lifecycle::state::CoreLifecycleState::new(
                application,
                (*clash).clone(),
                profiles,
                core,
                installer,
                service,
            ),
            #[cfg(test)]
            startup_report: Arc::new(parking_lot::Mutex::new(None)),
        }
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

    /// Consume the observed Prepared terminal only in the owner Actor's
    /// Completed message; source commits and old Core ports have none.

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
        self.shared.startup_owner_unproven_handle()
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

    pub(in crate::client) async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        self.shared.effect_port_binding().await
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
        chimera_core::client::core_lifecycle::stale_proxy::StaleProxyAction {
            core: self.core.as_ref(),
            application: &self.application,
            clash: &self.clash,
        }
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
    }

    pub(in crate::client) fn convergence_clash(
        &self,
    ) -> anyhow::Result<chimera_config::clash::config::ClashConfig> {
        self.clash.get()
    }

    pub(in crate::client) fn convergence_authority(
        &self,
    ) -> Option<&dyn crate::client::reconciler::authority::RuntimeSourceAuthority> {
        self.shared.convergence_authority()
    }

    pub(in crate::client) fn convergence_candidate_check(
        &self,
    ) -> Option<&crate::client::reconciler::check::CandidateCheck> {
        self.shared.convergence_candidate_check()
    }

    pub(in crate::client) fn stage_prepared_outcome(
        &self,
        outcome: Option<crate::client::reconciler::mutation::ApplyOutcome>,
    ) {
        self.shared.stage_prepared_outcome(outcome)
    }

    pub(in crate::client) fn stage_connection_effect(&self, effect: CheckedConnectionEffect) {
        self.shared.stage_connection_effect(effect)
    }

    pub(in crate::client) fn canonical_convergence_session(&self) -> bool {
        self.shared.canonical_convergence_session()
    }

    pub(in crate::client) fn outcome_uncertain(&self) -> bool {
        self.shared.outcome_uncertain()
    }

    pub(in crate::client) fn startup_application_config(&self) -> ChimeraAppConfig {
        self.application.get_typed()
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
            } => chimera_core::state::mutation::impact::application_patch_requires_reconcile(
                patch,
                *service_connected,
            ),
            _ => false,
        };
        #[cfg(not(test))]
        let mut startup_retry_target = ();
        #[cfg(not(test))]
        let startup_retry_target = &mut startup_retry_target;
        let result = chimera_core::client::core_lifecycle::executor::execute(
            self,
            command,
            startup_retry_target,
        )
        .await;
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
        #[cfg(test)]
        if self.checked_gate.get().is_none() || self.core.runtime_preparation_session().is_none() {
            return crate::client::reconciler::legacy_check::check_candidate(
                self.core.as_ref(),
                inputs,
                crate::core::service::ipc::get_ipc_state().is_connected(),
            )
            .await;
        }
        self.shared.validate_profile_runtime(inputs).await
    }

    async fn prepare_profile_runtime(
        &self,
        inputs: crate::client::application_workflow::inputs::RuntimeInputs,
        operation_id: &chimera_core_manager::OperationId,
    ) -> anyhow::Result<(
        crate::client::application_workflow::mutation::AppliedCandidate,
        crate::client::application_workflow::mutation::CheckRecord,
    )> {
        self.shared
            .prepare_profile_runtime(inputs, operation_id, || {
                crate::core::service::ipc::get_ipc_state().is_connected()
            })
            .await
    }

    async fn confirm_profile_runtime(
        &self,
        operation_id: &chimera_core_manager::OperationId,
        candidate: crate::client::application_workflow::mutation::AppliedCandidate,
    ) -> anyhow::Result<()> {
        self.shared
            .confirm_profile_runtime(operation_id, candidate)
            .await
    }

    async fn try_release_service_owner(&self) -> std::result::Result<(), String> {
        CoreLifecycleWorkflow::try_release_service_owner(self)
            .await
            .map(|_| ())
    }

    async fn discard_profile_runtime(&self, operation_id: &chimera_core_manager::OperationId) {
        self.shared.discard_profile_runtime(operation_id).await
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

#[async_trait::async_trait]
impl chimera_core::client::core_lifecycle::executor::LifecycleHost for CoreLifecycleWorkflow {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    type SystemProxy = crate::client::system_proxy::SystemProxyClient;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    type SystemProxy = ();
    #[cfg(test)]
    type ExtensionState = Option<StartupRetryTarget>;
    #[cfg(not(test))]
    type ExtensionState = ();
    fn application_config(&self) -> ChimeraAppConfig {
        self.application.get_typed()
    }
    fn mode_interruption_enabled(&self) -> anyhow::Result<bool> {
        Ok(self.clash.get()?.break_connection.on_mode_change)
    }
    fn core(&self) -> &dyn CoreLifecyclePort {
        self.core.as_ref()
    }
    fn installer(&self) -> &dyn BinaryInstaller {
        self.installer.as_ref()
    }
    fn service_recovery(&self) -> &parking_lot::Mutex<ServiceRecovery> {
        &self.service_recovery
    }
    fn progress(&self) -> &parking_lot::Mutex<RuntimeProgress> {
        &self.progress
    }
    fn clear_round_effects(&self) {
        self.shared.clear_round_effects();
    }
    fn canonical_convergence_session(&self) -> bool {
        self.canonical_convergence_session()
    }
    async fn reconcile_with_trigger(&self, trigger: Trigger) -> anyhow::Result<()> {
        self.reconcile_with_trigger(trigger).await
    }
    async fn apply_checked_runtime(
        &self,
        prepared: super::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        domain: crate::client::reconciler::committed::SourceDomain,
        version: u64,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        interrupt_mode: bool,
    ) -> anyhow::Result<()> {
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
    async fn retry_deferred_profiles(&self, digest: &str) -> anyhow::Result<()> {
        self.retry_deferred_profiles(digest).await
    }
    async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        self.reconcile_profiles(profiles, staged_content).await
    }
    async fn patch_application_config_with_result(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<ApplicationPatchResult> {
        self.patch_application_config_with_result(patch, service_connected)
            .await
    }
    async fn maintain_service_release(&self) -> anyhow::Result<()> {
        self.maintain_service_release().await
    }
    async fn report_service_endpoint_down(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<bool> {
        self.shared_state()
            .report_service_endpoint_down(transition)
            .await
    }
    fn record_service_reconnected(&self) {
        self.service_recovery_reconnected
            .store(true, Ordering::Release);
    }
    async fn refresh_service_recovery_host(&self) {
        self.shared_state().refresh_service_recovery_host().await
    }
    async fn execute_extension(
        &self,
        command: Command,
        #[allow(unused_variables)] startup_retry_target: &mut Self::ExtensionState,
    ) -> anyhow::Result<()> {
        match command {
            #[cfg(test)]
            Command::ProfileMutation(_) => {
                anyhow::bail!("Profile mutations must run through RuntimeReconcilerActor")
            }
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

            _ => anyhow::bail!("command must be handled by the application workflow"),
        }
    }
}

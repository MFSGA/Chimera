//! Shared lifecycle owner state. The host supplies its existing Core and Service ports.
use super::{
    ports::{BinaryInstaller, CoreLifecyclePort, ServiceLifecyclePort, ServiceTransitionLease},
    workflow::ServiceRecovery,
};
use crate::{
    actor_v2::{endpoint::ExecutionHost, service_status::ServiceHostStatus},
    client::reconciler::{
        apply::CheckedConnectionEffect, service_evidence::ServiceEvidence, state::RuntimeProgress,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct CoreLifecycleState {
    pub profiles: Option<crate::state::StateSnapshot<chimera_config::profile::Profiles>>,
    pub content_reader: Option<crate::state::mutation::ProfileContentReader>,
    /// Installed once by the production Client after the canonical Core
    /// session and Profiles gate have been assembled. Shared by clones of
    /// the sole lifecycle workflow, not an independent runtime owner.
    pub checked_gate:
        Arc<std::sync::OnceLock<Arc<crate::client::reconciler::check::CandidateCheck>>>,
    /// Ref's applied selection and last target/host attempt share a single
    /// runtime-progress owner. Only the current Core worker may record them,
    /// and the Actor retains final Conclusion/source-version authority.
    pub progress: Arc<parking_lot::Mutex<RuntimeProgress>>,
    /// A checked source's post-apply Mode effect is held by the same
    /// workflow until its sole Actor has fenced the Completed message.
    checked_connection_effect: Arc<parking_lot::Mutex<Option<CheckedConnectionEffect>>>,
    /// A one-round observed Core terminal, shared only with the existing
    /// Reconciler policy and Actor Completed. Never records a source commit
    /// or replaces the independent Running/source convergence fence.
    prepared_outcome:
        Arc<parking_lot::Mutex<Option<crate::client::reconciler::mutation::ApplyOutcome>>>,
    /// The exact read-only source authority already used by Checked Apply.
    /// Ordinary startup/reconcile may adopt selection only when an active
    /// Core receipt and this authority agree on the full committed target.
    pub source_authority:
        Option<Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>>,
    pub core: Arc<dyn CoreLifecyclePort>,
    pub installer: Arc<dyn BinaryInstaller>,
    pub service: Arc<dyn ServiceLifecyclePort>,
    startup_owner_unproven: Arc<AtomicBool>,
    pub service_failed_generation_shared: Arc<AtomicU64>,
    pub service_status: Option<watch::Receiver<ServiceHostStatus>>,
    pub service_recovery: Arc<parking_lot::Mutex<ServiceRecovery>>,
    pub service_recovery_reconnected: Arc<AtomicBool>,
    pub shutdown: CancellationToken,
}

impl CoreLifecycleState {
    pub fn new(
        profiles: Option<crate::state::StateSnapshot<chimera_config::profile::Profiles>>,
        core: Arc<dyn CoreLifecyclePort>,
        installer: Arc<dyn BinaryInstaller>,
        service: Arc<dyn ServiceLifecyclePort>,
    ) -> Self {
        Self {
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
            service_failed_generation_shared: Arc::new(AtomicU64::new(0)),
            service_status: None,
            service_recovery: Arc::new(parking_lot::Mutex::new(ServiceRecovery::default())),
            service_recovery_reconnected: Arc::new(AtomicBool::new(false)),
            shutdown: CancellationToken::new(),
        }
    }
    pub fn install_checked_gate(
        &self,
        gate: Arc<crate::client::reconciler::check::CandidateCheck>,
    ) -> anyhow::Result<()> {
        self.checked_gate
            .set(gate)
            .map_err(|_| anyhow::anyhow!("production candidate gate already installed"))
    }

    pub fn invalidate_applied_selection(&self) {
        self.progress.lock().invalidate_selection();
    }

    pub fn take_checked_connection_effect(&self) -> Option<CheckedConnectionEffect> {
        self.checked_connection_effect.lock().take()
    }

    pub fn take_prepared_outcome(
        &self,
    ) -> Option<crate::client::reconciler::mutation::ApplyOutcome> {
        self.prepared_outcome.lock().take()
    }

    pub fn startup_owner_unproven_handle(&self) -> Arc<AtomicBool> {
        self.startup_owner_unproven.clone()
    }

    pub fn service_failed_generation_handle(&self) -> Arc<AtomicU64> {
        self.service_failed_generation_shared.clone()
    }

    pub fn startup_owner_unproven(&self) -> bool {
        self.startup_owner_unproven.load(Ordering::Acquire)
    }

    pub async fn probe_service(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.service.probe().await
    }

    pub async fn effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        let receipt = match self.core.active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::debug!(%error, "no active runtime receipt is available after lifecycle work");
                return None;
            }
        };
        crate::client::application_operations::resolved_effect_port_binding(receipt.as_deref())
    }

    pub fn convergence_profiles_version(&self) -> Option<u64> {
        self.profiles
            .as_ref()
            .map(|profiles| *profiles.load().version.as_ref())
    }

    pub fn convergence_authority(
        &self,
    ) -> Option<&dyn crate::client::reconciler::authority::RuntimeSourceAuthority> {
        self.source_authority.as_deref()
    }

    pub fn convergence_candidate_check(
        &self,
    ) -> Option<&crate::client::reconciler::check::CandidateCheck> {
        self.checked_gate.get().map(Arc::as_ref)
    }

    pub fn stage_prepared_outcome(
        &self,
        outcome: Option<crate::client::reconciler::mutation::ApplyOutcome>,
    ) {
        *self.prepared_outcome.lock() = outcome;
    }

    pub fn stage_connection_effect(&self, effect: CheckedConnectionEffect) {
        *self.checked_connection_effect.lock() = Some(effect);
    }

    pub fn convergence_core(&self) -> &dyn CoreLifecyclePort {
        self.core.as_ref()
    }

    pub fn convergence_progress(&self) -> &parking_lot::Mutex<RuntimeProgress> {
        self.progress.as_ref()
    }

    pub fn canonical_convergence_session(&self) -> bool {
        self.profiles.is_some() && self.checked_gate.get().is_some()
    }

    pub fn outcome_uncertain(&self) -> bool {
        self.core.outcome_uncertain()
    }

    pub fn pending_runtime_product_target(&self) -> Option<String> {
        self.core.pending_runtime_product_target()
    }

    pub fn service_restart_policy(
        &self,
    ) -> crate::actor_v2::service_policy::ServiceRestartPolicySnapshot {
        self.service.restart_policy()
    }

    pub fn follow_service_recovery_host(&self, host: ExecutionHost) {
        self.service_recovery.lock().follow_host(host);
    }

    pub async fn refresh_service_recovery_host(&self) {
        crate::client::core_lifecycle::service::refresh_service_recovery_host(
            self.core.as_ref(),
            &self.service_recovery,
        )
        .await
    }

    pub async fn service_endpoint_recovery_due(&self) -> bool {
        crate::client::core_lifecycle::service::service_endpoint_recovery_due(
            self.core.as_ref(),
            &self.service_recovery,
            &self.shutdown,
        )
        .await
    }

    pub fn service_recovery_due_now(&self) -> bool {
        crate::client::core_lifecycle::service::service_recovery_due_now(
            self.core.as_ref(),
            &self.service_recovery,
            &self.shutdown,
        )
    }

    pub fn service_recovery_blocks_handoff(&self) -> bool {
        let endpoint_degraded = self.core.service_endpoint_degraded();
        let recovery = self.service_recovery.lock();
        recovery.blocks_handoff(endpoint_degraded)
    }

    pub fn take_service_recovery_reconnected(&self) -> bool {
        self.service_recovery_reconnected
            .swap(false, Ordering::AcqRel)
    }

    pub async fn settle_service_recovery_action(&self) -> bool {
        crate::client::core_lifecycle::service::settle_service_recovery_action(self.core.as_ref())
            .await
    }

    pub async fn report_service_endpoint_down(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<bool> {
        if !self.core.service_endpoint_recovery_supported() {
            self.service.report_endpoint_down(transition).await?;
            return Ok(false);
        }
        crate::client::core_lifecycle::service::report_service_endpoint_down(
            self.core.as_ref(),
            &self.service_recovery,
            &self.shutdown,
        )
        .await
    }

    pub async fn release_service_owner(&self) -> Result<ServiceEvidence, String> {
        crate::client::core_lifecycle::ownership::release_service_owner(self.core.as_ref(), || {
            self.service_evidence()
        })
        .await
    }

    pub async fn try_release_service_owner(&self) -> Result<ServiceEvidence, String> {
        let _transition = self.service.try_begin_transition().await.map_err(|error| {
            format!("could not acquire the Service transition lease: {error:#}")
        })?;
        let result = self.release_service_owner().await;
        self.refresh_service_recovery_host().await;
        result
    }

    pub async fn try_begin_service_transition(
        &self,
    ) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.service.try_begin_transition().await
    }

    pub async fn service_evidence(&self) -> (ServiceEvidence, u64) {
        crate::client::core_lifecycle::service::service_evidence(
            self.service.as_ref(),
            self.service_status.as_ref(),
        )
        .await
    }

    /// A refused Service submission may have already applied its Core but
    /// failed *afterwards* while promoting RuntimeProduct. The startup owner
    /// may release Service only after a stable, non-synthetic Stopped state
    /// with no accepted Core receipt. This independent proof also protects
    /// other errors that arrived after a concurrent owner transition.
    pub async fn startup_service_refusal_stopped(&self) -> bool {
        crate::client::reconciler::proof::startup_service_refusal_stopped(self.core.as_ref()).await
    }

    pub async fn startup_service_command_settled(&self) -> bool {
        self.service.command_settled().await
    }

    pub fn startup_failed_generation(&self) -> u64 {
        self.service_failed_generation_shared
            .load(Ordering::Acquire)
    }

    pub fn set_startup_failed_generation(&self, generation: u64) {
        self.service_failed_generation_shared
            .store(generation, Ordering::Release);
    }

    pub async fn startup_select_execution_host(
        &self,
        host: crate::actor_v2::endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        self.core.select_execution_host(host).await?;
        self.follow_service_recovery_host(host);
        Ok(())
    }

    pub fn clear_round_effects(&self) {
        self.checked_connection_effect.lock().take();
        self.prepared_outcome.lock().take();
    }
}

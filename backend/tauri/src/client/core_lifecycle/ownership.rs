//! Conditional Core retirement and selected-host ownership operations.
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::core_lifecycle::ports::CoreStatusSnapshot;
use chimera_core::client::reconciler::service_evidence::ServiceEvidence;
use std::sync::atomic::Ordering;

#[async_trait::async_trait]
impl chimera_core::client::reconciler::startup_executor::StartupHost for CoreLifecycleWorkflow {
    type Core = dyn crate::client::core_lifecycle::ports::CoreLifecyclePort;
    fn core(&self) -> &Self::Core {
        self.core.as_ref()
    }
    fn startup_application_config(&self) -> chimera_config::application::ChimeraAppConfig {
        CoreLifecycleWorkflow::startup_application_config(self)
    }
    fn startup_failed_generation(&self) -> u64 {
        self.shared_state().startup_failed_generation()
    }
    fn set_startup_failed_generation(&self, generation: u64) {
        self.shared_state()
            .set_startup_failed_generation(generation);
    }
    fn canonical_convergence_session(&self) -> bool {
        CoreLifecycleWorkflow::canonical_convergence_session(self)
    }
    fn progress(
        &self,
    ) -> &parking_lot::Mutex<chimera_core::client::reconciler::state::RuntimeProgress> {
        self.convergence_progress()
    }
    async fn fallback_after_service_refusal(
        &self,
        target: &str,
        refusal: anyhow::Error,
    ) -> anyhow::Result<()> {
        CoreLifecycleWorkflow::fallback_after_service_refusal(self, target, refusal).await
    }
    async fn startup_service_command_settled(&self) -> bool {
        self.shared_state().startup_service_command_settled().await
    }
    async fn service_evidence(&self) -> (ServiceEvidence, u64) {
        CoreLifecycleWorkflow::service_evidence(self).await
    }
    async fn startup_select_execution_host(
        &self,
        host: chimera_core::actor_v2::endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        self.shared_state()
            .startup_select_execution_host(host)
            .await
    }
    async fn startup_target_identity(&self) -> anyhow::Result<String> {
        CoreLifecycleWorkflow::startup_target_identity(self).await
    }
    async fn recover_committed_startup(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> Option<anyhow::Result<()>> {
        CoreLifecycleWorkflow::recover_committed_startup(self, observed).await
    }
    async fn startup_reconcile_application(
        &self,
        app: chimera_config::application::ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        CoreLifecycleWorkflow::startup_reconcile_application(self, app, service_connected).await
    }
    async fn release_service_owner(&self) -> Result<ServiceEvidence, String> {
        CoreLifecycleWorkflow::release_service_owner(self).await
    }
    async fn startup_reconcile_application_after_service_release(
        &self,
        app: chimera_config::application::ChimeraAppConfig,
        expected_target: Option<&str>,
    ) -> anyhow::Result<()> {
        CoreLifecycleWorkflow::startup_reconcile_application_after_service_release(
            self,
            app,
            expected_target,
        )
        .await
    }
    fn service_recovery_blocks_handoff(&self) -> bool {
        CoreLifecycleWorkflow::service_recovery_blocks_handoff(self)
    }
}

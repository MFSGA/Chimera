//! Serialized committed convergence dispatch over the sole lifecycle owner.
use chimera_core::client::reconciler::plan::Trigger;

use crate::client::core_lifecycle::CoreLifecycleWorkflow;

pub use chimera_core::client::core_lifecycle::command::{admission_command, runtime_trigger};
pub use chimera_core::client::reconciler::workflow::{
    AdmissionCommand, CheckedSourceNoticeKind, ProfilesSourceAdvanced, RuntimeAdmission,
    RuntimeReconcilerState, checked_mode_effect_deferred, committed_target_advanced,
    deferred_mode_effect_degradation, desired_host, source_committed_during_round,
};

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn reconcile_with_trigger(
        &self,
        trigger: Trigger,
    ) -> anyhow::Result<()> {
        chimera_core::client::reconciler::convergence_executor::reconcile_with_trigger(
            self, trigger,
        )
        .await
    }
}

#[async_trait::async_trait]
impl chimera_core::client::reconciler::convergence_executor::ConvergenceHost
    for CoreLifecycleWorkflow
{
    fn convergence_clash(&self) -> anyhow::Result<chimera_config::clash::config::ClashConfig> {
        self.convergence_clash()
    }
    fn convergence_profiles_version(&self) -> Option<u64> {
        self.convergence_profiles_version()
    }
    fn legacy_service_connected(&self) -> bool {
        crate::core::service::ipc::get_ipc_state().is_connected()
    }
    async fn capture_committed_profile_inputs_for(
        &self,
        app: chimera_config::application::ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
    ) -> anyhow::Result<super::inputs::RuntimeInputs> {
        self.capture_committed_profile_inputs_for(app, clash).await
    }
    async fn try_reconcile_prepared_local(
        &self,
        target: &str,
        app: &chimera_config::application::ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        self.try_reconcile_prepared_local(target, app, clash, run_type)
            .await
    }
    async fn try_reconcile_prepared_service(
        &self,
        target: &str,
        app: &chimera_config::application::ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
    ) -> Option<anyhow::Result<()>> {
        self.try_reconcile_prepared_service(target, app, clash)
            .await
    }
    async fn try_reconcile_deferred_prepared(
        &self,
        target: &str,
        app: &chimera_config::application::ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        self.try_reconcile_deferred_prepared(target, app, clash, run_type)
            .await
    }
    async fn legacy_retry_deferred_profiles(
        &self,
        inputs: super::inputs::RuntimeInputs,
        expected_digest: &str,
    ) -> anyhow::Result<()> {
        self.legacy_retry_deferred_profiles(inputs, expected_digest)
            .await
    }
    async fn try_begin_service_transition(
        &self,
    ) -> anyhow::Result<Box<dyn crate::client::core_lifecycle::ports::ServiceTransitionLease>> {
        self.try_begin_service_transition().await
    }
    async fn refresh_service_recovery_host(&self) {
        self.refresh_service_recovery_host().await
    }
}

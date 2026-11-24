use crate::client::core_lifecycle::{CoreLifecycleWorkflow, ports::CoreStatusSnapshot};
pub use chimera_core::client::reconciler::proof::{
    host_for, receipt_matches_owner, receipt_matches_target,
};
impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn verified_applied_runtime(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> bool {
        chimera_core::client::reconciler::proof::verified_applied_runtime(
            self.core.as_ref(),
            || self.startup_target_identity(),
            observed,
        )
        .await
    }
}

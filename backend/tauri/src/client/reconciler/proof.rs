use crate::client::core_lifecycle::{CoreLifecycleWorkflow, ports::CoreStatusSnapshot};
use chimera_core::actor_v2::endpoint::ExecutionHost;
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
    pub(in crate::client) async fn recovery_receipt_matches(
        &self,
        current: &CoreStatusSnapshot,
        desired: ExecutionHost,
        allow_receipt_reuse: bool,
    ) -> Result<bool, String> {
        chimera_core::client::reconciler::proof::recovery_receipt_matches(
            self.core.as_ref(),
            || self.startup_target_identity(),
            current,
            desired,
            allow_receipt_reuse,
        )
        .await
    }
}

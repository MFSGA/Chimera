//! Read-only proof that a committed target, selected host and active Core agree.
//!
//! This deliberately does not decide host handoff, start the Core, or write
//! source state. The runtime owner must not infer Converged from a successful
//! apply call or a Running state alone.
use chimera_core::actor_v2::endpoint::ExecutionHost;

use crate::{
    client::{application_workflow::policy::CoreRunIntent, runtime::RuntimeApplyReceipt},
    core::clash::core::RunType,
};

pub(crate) fn host_for(run_type: RunType) -> ExecutionHost {
    match run_type {
        RunType::Service => ExecutionHost::Service,
        RunType::Normal | RunType::Elevated => ExecutionHost::Local,
    }
}

/// S3 proves the live receipt independently of the desired source. A verified
/// older target is safe to replace in place; it is not an unverified Core.
pub(crate) fn receipt_matches_owner(
    receipt: &RuntimeApplyReceipt,
    observed_host: ExecutionHost,
) -> bool {
    receipt.run_intent == CoreRunIntent::Running && receipt.host == observed_host
}

pub(crate) fn receipt_matches_target(
    receipt: &RuntimeApplyReceipt,
    target: &str,
    observed_host: ExecutionHost,
) -> bool {
    receipt_matches_owner(receipt, observed_host) && receipt.target.as_deref() == Some(target)
}

use crate::client::{
    core_lifecycle::{CoreLifecycleWorkflow, ports::CoreStatusSnapshot},
    reconciler::decision::same_core_status,
};
use chimera_ipc::api::status::CoreState;
use std::sync::Arc;

impl CoreLifecycleWorkflow {
    /// Ref's applied-target proof: the Core must report the exact receipt
    /// for the current committed inputs and selected host, then remain the
    /// same instance through a second authoritative observation.
    pub(in crate::client) async fn verified_applied_runtime(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> bool {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_target_and_owner_must_match_the_active_receipt() {
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.target = Some("committed-a".to_owned());

        assert!(receipt_matches_target(
            &receipt,
            "committed-a",
            ExecutionHost::Local
        ));
        assert!(receipt_matches_owner(&receipt, ExecutionHost::Local));
        assert!(!receipt_matches_target(
            &receipt,
            "committed-b",
            ExecutionHost::Local
        ));
        assert!(!receipt_matches_target(
            &receipt,
            "committed-a",
            ExecutionHost::Service
        ));
        receipt.run_intent = CoreRunIntent::StoppedByUser;
        assert!(!receipt_matches_target(
            &receipt,
            "committed-a",
            ExecutionHost::Local
        ));
    }

    #[test]
    fn host_mapping_tracks_actual_selected_execution_mode() {
        assert_eq!(host_for(RunType::Normal), ExecutionHost::Local);
        assert_eq!(host_for(RunType::Elevated), ExecutionHost::Local);
        assert_eq!(host_for(RunType::Service), ExecutionHost::Service);
    }
}

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

use super::ChimeraClient;
pub use chimera_core::client::mutation_outcome::{Degradation, MutationOutcome};

// Upstream Runtime Values own these wire-stable values. The ref's four
// runtime states and five phases are supplemented by Chimera's explicit
// pending/recovery and multi-stage commit statuses in the shared crate.
pub use chimera_contracts::runtime::{CommitReceipt, DegradationPhase, RuntimeCommitStatus};

// The shared Core owns the exact runtime receipt, allocator and snapshot store.
pub use chimera_core::client::runtime::{
    CandidateFile, CheckedPromotionError, RUNTIME_CONFIG, RUNTIME_CONFIG_DIR, RUNTIME_CONFIG_FILE,
    RuntimeApplyReceipt, RuntimeLifecycle, RuntimePaths, RuntimeRebuildGate, RuntimeRevision,
    RuntimeRevisionAllocator, RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransactionSnapshot,
    RuntimeTransformFailure, capture_runtime_transaction, check_and_promote_candidate,
    promote_candidate, restore_failed_apply, restore_optional_product, restore_product,
};

#[cfg(test)]
mod shared_runtime_values_contract_tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn desktop_receipt_status_and_phase_use_the_shared_ref_types() {
        assert_eq!(
            TypeId::of::<CommitReceipt>(),
            TypeId::of::<chimera_contracts::runtime::CommitReceipt>()
        );
        assert_eq!(
            TypeId::of::<RuntimeCommitStatus>(),
            TypeId::of::<chimera_contracts::runtime::RuntimeCommitStatus>()
        );
        assert_eq!(
            TypeId::of::<DegradationPhase>(),
            TypeId::of::<chimera_contracts::runtime::DegradationPhase>()
        );
        let receipt = CommitReceipt {
            operation_id: Some("core-lease".into()),
            domain: "clash".into(),
            source_version: 8,
            runtime: RuntimeCommitStatus::RecoveryRequired,
        };
        let outcome = MutationOutcome::from_parts((), Vec::new()).with_commit(receipt);
        let wire = serde_json::to_value(outcome).unwrap();
        assert_eq!(wire["status"], "committed");
        assert_eq!(wire["commits"][0]["runtime"], "recovery_required");
        assert_eq!(wire["commits"][0]["source_version"], 8);
    }

    #[test]
    fn typed_degradation_keeps_legacy_flat_ipc_shape() {
        use chimera_contracts::runtime as shared;

        let projected = Degradation::from_shared(shared::Degradation {
            phase: shared::DegradationPhase::ProfileMaterialization,
            reason: shared::DegradationReason::CleanupDeferred,
            message: "fenced cleanup".into(),
            retryable: true,
        });
        let wire = serde_json::to_value(projected).unwrap();
        assert_eq!(wire["phase"], "profile_materialization");
        assert_eq!(wire["code"], "cleanup_deferred");
        assert_eq!(wire["message"], "fenced cleanup");
        assert_eq!(wire["retryable"], true);
        assert!(wire.get("reason").is_none());
    }
}

impl ChimeraClient {
    pub(crate) async fn rebuild_running_config(&self) -> anyhow::Result<()> {
        let result = self.inner.core_lifecycle.reconcile().await;
        if let Err(error) = result {
            self.inner.ui_sink.refresh_runtime_transform_diagnostics();
            return Err(error);
        }
        self.inner.ui_sink.refresh_clash();
        crate::feat::update_proxies_buff(self.clone(), None);
        Ok(())
    }
}

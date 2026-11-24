use crate::client::runtime::RuntimeApplyReceipt;
use async_trait::async_trait;
use std::sync::Arc;
/// Checked Core apply result after the lower owner confirmed its revision.
/// The legacy port lacks replacement evidence: unknown must not be treated
/// as an in-place config patch when deciding whether to interrupt a Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckedApplyOutcome {
    pub replaced: Option<bool>,
}

/// The Core's result and its *separate* derived-product publication verdict.
/// `Applied` is never inferred from a successful source commit. Production
/// must return the actual accepted receipt and the exact submitted product;
/// old compatibility ports leave those fields absent rather than inventing
/// a Core/RuntimeProduct identity.
#[derive(Debug, Clone)]
pub struct PreparedCoreSettlement {
    /// A complete verified report from the Core router, when this port can
    /// prove its host/generation and accepted revision. No synthetic report
    /// is constructed for legacy ports without the owner evidence.
    pub report: Option<Box<crate::actor_v2::facade::ReconcileReport>>,
    /// Terminal outcome observed directly from this operation's Core registry.
    /// This is not a substitute for the separate router status proof.
    pub outcome: Option<chimera_ipc::api::core::v2::ReconcileOutcomeInfo>,
    pub replaced: Option<bool>,
    pub confirmed: Option<Arc<RuntimeApplyReceipt>>,
    pub product: Option<Arc<crate::client::runtime::RuntimeSnapshot>>,
    pub publication: PreparedProductPublication,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedProductPublication {
    Published,
    Pending { reason: String },
    Unobserved,
}

/// A terminal Core command may have restored its old revision, or its reply
/// may have become unobservable. Neither case is an AppliedCandidate.
#[derive(Debug, Clone)]
pub enum PreparedSubmission {
    Applied(PreparedCoreSettlement),
    RolledBack(chimera_ipc::api::core::v2::ReconcileOutcomeInfo),
    Unknown(chimera_core_manager::CoreError),
}

#[derive(Debug, thiserror::Error)]
#[error("core runtime apply was rolled back: {reason}")]
pub struct PreparedRolledBack {
    pub report: chimera_ipc::api::core::v2::ReconcileOutcomeInfo,
    pub reason: String,
}

impl PreparedCoreSettlement {
    /// The real accepted candidate carries one exact source, Core revision,
    /// product and digest. Old test/compatibility adapters are explicitly
    /// Unobserved; a half-filled receipt may never pretend to be Applied.
    pub fn validate_identity(&self, target: &str) -> anyhow::Result<()> {
        match (&self.confirmed, &self.product) {
            (None, None)
                if self.publication == PreparedProductPublication::Unobserved
                    && self.outcome.is_none()
                    && self.report.is_none() =>
            {
                Ok(())
            }
            (Some(receipt), Some(product)) => {
                anyhow::ensure!(
                    receipt.target.as_deref() == Some(target),
                    "confirmed Core receipt belongs to a different committed source"
                );
                anyhow::ensure!(
                    receipt.revision == product.revision
                        && receipt.target_core == product.target_core
                        && receipt.config_digest
                            == chimera_core_manager::payload_digest(product.product_bytes()),
                    "confirmed Core receipt does not match the accepted runtime product"
                );
                anyhow::ensure!(
                    receipt
                        .artifact
                        .as_ref()
                        .is_some_and(|saved| Arc::ptr_eq(saved, product)),
                    "confirmed Core receipt did not retain the exact submitted product"
                );
                if let Some(outcome) = &self.outcome {
                    use chimera_ipc::api::core::v2::ReconcileOutcomeKind;
                    anyhow::ensure!(
                        outcome.revision == receipt.applied_revision
                            && outcome.outcome != ReconcileOutcomeKind::RolledBack,
                        "applied candidate carried a rolled-back or mismatched Core revision"
                    );
                    anyhow::ensure!(
                        self.replaced
                            == Some(matches!(
                                outcome.outcome,
                                ReconcileOutcomeKind::Started
                                    | ReconcileOutcomeKind::Restarted
                                    | ReconcileOutcomeKind::Switched
                            )),
                        "Core replacement verdict does not match the reported transaction"
                    );
                }
                if let Some(report) = &self.report {
                    anyhow::ensure!(
                        report.applied.host == receipt.host
                            && report.applied.revision == receipt.applied_revision
                            && report.status.host == receipt.host
                            && report.status.generation == report.applied.generation
                            && receipt.confirms_projection(&report.status)
                            && self.outcome.as_ref().is_some_and(|outcome| report.output
                                == chimera_ipc::api::core::v2::OperationOutputInfo::Reconciled(
                                    outcome.clone()
                                )),
                        "Core ReconcileReport is not bound to the accepted host, revision and terminal operation"
                    );
                }
                Ok(())
            }
            _ => anyhow::bail!("prepared Core result has no coherent accepted product receipt"),
        }
    }

    pub fn require_published(self) -> anyhow::Result<CheckedApplyOutcome> {
        if let PreparedProductPublication::Pending { reason } = self.publication {
            anyhow::bail!("Core applied but RuntimeProduct publication failed: {reason}");
        }
        Ok(CheckedApplyOutcome {
            replaced: self.replaced,
        })
    }
}

// Pinned ref/backend/nyanpasu-core/src/client/core_lifecycle/ports.rs:
// Typed, read-only candidate build and publication boundary. Its prepared
// value does not apply anything to a running Core.
use super::super::runtime;
use crate::client::core_spec::CoreSpecError;
use chimera_contracts::publish::PublishRuntimeError;

/// Application-owned runtime preparation; implementations never call the workflow actor.
#[async_trait]
pub trait RuntimePreparationPort: Send + Sync {
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot)
    -> Result<(), PublishRuntimeError>;
    fn core_spec(
        &self,
        core: &chimera_config::application::ClashCore,
    ) -> Result<chimera_core_manager::CoreSpec, CoreSpecError>;
}

pub struct PreparedRuntime {
    pub snapshot: Arc<runtime::RuntimeSnapshot>,
    /// The to-be-committed bytes, serialized exactly once so the advisory
    /// check and the reconcile cannot drift apart.
    pub intent: Arc<crate::actor_v2::intent::RuntimeIntent>,
    /// The ports this candidate would bind. Inert: only the receipt of an
    /// apply that succeeded may confirm them.
    pub ports: crate::client::ports::CandidatePortBindings,
    /// The committed target the build came from (`RuntimeInputs::target_key`),
    /// when it has one.
    pub target: Option<String>,
}

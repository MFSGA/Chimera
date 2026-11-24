//! Shared source candidate and committed-content ports.
pub mod change;
pub mod ports;
pub mod profile_content;
pub use change::DomainChange;
pub use ports::{RuntimeCandidate, RuntimeGate};
pub use profile_content::{ProfileContentLock, ProfileContentReader};
/// The source owner is independent of the host's runtime orchestration.
pub trait ProfileMutationPort: Send + Sync + 'static {
    fn ensure_ready(&self) -> anyhow::Result<()>;
    fn runtime_gate(
        &self,
    ) -> Result<
        std::sync::Arc<dyn RuntimeGate>,
        std::sync::Arc<chimera_contracts::runtime_error::RuntimeError>,
    >;
    fn source_committed(
        &self,
        version: u64,
        reaches_runtime: bool,
        operation: Option<chimera_core_manager::OperationId>,
        files: Vec<chimera_config::profile::ProfileId>,
    ) -> chimera_contracts::runtime::CommitReceipt;
}

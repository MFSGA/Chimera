//! Source-owned boundaries for independent checks and committed-state notifications.

/// A candidate is checked without changing the running core or publishing source state.
pub struct RuntimeCandidate {
    pub change: super::DomainChange,
    pub staged: std::collections::BTreeMap<String, String>,
}

/// Source-owned boundary for independent checks and committed-state notifications.
#[async_trait::async_trait]
pub trait RuntimeGate: Send + Sync + 'static {
    async fn check(
        &self,
        candidate: RuntimeCandidate,
    ) -> Result<(), std::sync::Arc<chimera_contracts::runtime_error::RuntimeError>>;
    fn committed(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: crate::state::Version,
    );
}

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
    async fn check_with_receipt(
        &self,
        _candidate: RuntimeCandidate,
        _interrupt_mode: bool,
    ) -> Result<String, std::sync::Arc<chimera_contracts::runtime_error::RuntimeError>> {
        use chimera_contracts::runtime_error::{RuntimeCheckUnavailable, RuntimeError};
        Err(std::sync::Arc::new(RuntimeError::CheckUnavailable {
            reason: RuntimeCheckUnavailable::NotChecked {
                reason: "exact candidate receipts are unavailable".into(),
            },
        }))
    }
    fn committed_with_receipt(
        &self,
        _domain: chimera_contracts::mutation::ConfigDomain,
        _version: crate::state::Version,
        _receipt: String,
    ) {
        unreachable!("a gate returning a receipt must implement settlement")
    }
    fn abort_with_receipt(
        &self,
        _domain: chimera_contracts::mutation::ConfigDomain,
        _receipt: String,
    ) {
        unreachable!("a gate returning a receipt must implement cancellation")
    }
    fn source_only_committed(
        &self,
        _domain: chimera_contracts::mutation::ConfigDomain,
        _version: crate::state::Version,
    ) {
    }
    fn committed(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: crate::state::Version,
    );
}

/// Owned checked proposal. Transferred into the committing source mailbox;
/// cancellation or failed persistence discards exactly this proposal.
pub struct CheckedReceipt {
    gate: std::sync::Arc<dyn RuntimeGate>,
    domain: chimera_contracts::mutation::ConfigDomain,
    receipt: Option<String>,
}

impl std::fmt::Debug for CheckedReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedReceipt")
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

impl CheckedReceipt {
    pub(crate) async fn check(
        gate: std::sync::Arc<dyn RuntimeGate>,
        domain: chimera_contracts::mutation::ConfigDomain,
        candidate: RuntimeCandidate,
        interrupt_mode: bool,
    ) -> anyhow::Result<Self> {
        let receipt = gate
            .check_with_receipt(candidate, interrupt_mode)
            .await
            .map_err(anyhow::Error::new)?;
        Ok(Self {
            gate,
            domain,
            receipt: Some(receipt),
        })
    }

    pub(crate) fn committed(mut self, version: crate::state::Version) {
        if let Some(receipt) = self.receipt.take() {
            self.gate
                .committed_with_receipt(self.domain, version, receipt);
        }
    }
}
impl Drop for CheckedReceipt {
    fn drop(&mut self) {
        if let Some(receipt) = self.receipt.take() {
            self.gate.abort_with_receipt(self.domain, receipt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Version;
    use chimera_contracts::{mutation::ConfigDomain, runtime_error::RuntimeError};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingGate {
        events: Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl RuntimeGate for RecordingGate {
        async fn check(&self, _: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
            unreachable!("native commits require exact receipts")
        }
        async fn check_with_receipt(
            &self,
            _: RuntimeCandidate,
            _: bool,
        ) -> Result<String, Arc<RuntimeError>> {
            Ok("candidate-1".into())
        }
        fn committed(&self, _: ConfigDomain, _: Version) {
            unreachable!("native commits settle exact receipts")
        }
        fn committed_with_receipt(&self, domain: ConfigDomain, version: Version, receipt: String) {
            self.events
                .lock()
                .unwrap()
                .push(format!("commit:{domain:?}:{}:{receipt}", version.as_ref()));
        }
        fn abort_with_receipt(&self, domain: ConfigDomain, receipt: String) {
            self.events
                .lock()
                .unwrap()
                .push(format!("abort:{domain:?}:{receipt}"));
        }
    }
    async fn proposal(gate: Arc<RecordingGate>) -> CheckedReceipt {
        CheckedReceipt::check(
            gate,
            ConfigDomain::Application,
            RuntimeCandidate {
                change: super::super::DomainChange::Application {
                    candidate: Arc::new(Default::default()),
                },
                staged: Default::default(),
            },
            false,
        )
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn dropping_uncommitted_receipt_aborts_only_that_proposal() {
        let gate = Arc::new(RecordingGate::default());
        drop(proposal(gate.clone()).await);
        assert_eq!(
            *gate.events.lock().unwrap(),
            ["abort:Application:candidate-1"]
        );
    }
    #[tokio::test]
    async fn durable_receipt_settles_once_without_abort() {
        let gate = Arc::new(RecordingGate::default());
        proposal(gate.clone()).await.committed(Version::new(7));
        assert_eq!(
            *gate.events.lock().unwrap(),
            ["commit:Application:7:candidate-1"]
        );
    }
}

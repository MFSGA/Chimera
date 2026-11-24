//! Shared source candidate and committed-content ports.
pub mod change;
pub mod impact;
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

use std::sync::Arc;

use snafu::ensure;
use tokio::sync::watch;

use crate::client::effects::ports::CommitNotifications;
use chimera_contracts::mutation::{NotReady, NotReadySnafu};

#[derive(Clone)]
enum Connection {
    Pending,
    Ready {
        effects: Arc<dyn CommitNotifications>,
        runtime: Arc<dyn ports::RuntimeGate>,
    },
    #[cfg(any(test, feature = "test-support"))]
    Isolated,
}

/// Only the composition root completes this connection. Domain actors can be
/// loaded first to supply read-only snapshots, and the effects owner is
/// spawned from them; no production mutation can run before the workflow and
/// the effects owner are connected. The channel carries startup wiring, not
/// shared mutable actor state.
#[derive(Clone)]
pub struct MutationCoordinator(watch::Sender<Connection>);

impl MutationCoordinator {
    pub fn pending() -> Self {
        Self(watch::channel(Connection::Pending).0)
    }

    pub fn connect(
        &self,
        effects: Arc<dyn CommitNotifications>,
        runtime: Arc<dyn ports::RuntimeGate>,
    ) {
        assert!(matches!(*self.0.borrow(), Connection::Pending));
        self.0.send_replace(Connection::Ready { effects, runtime });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn isolated() -> Self {
        Self(watch::channel(Connection::Isolated).0)
    }

    /// The effects owner a source hands its own slice to once it committed.
    /// Every write passes [`MutationCoordinator::ensure_ready`] first, so the
    /// connection is complete by then.
    pub fn effects(&self) -> Arc<dyn CommitNotifications> {
        match &*self.0.borrow() {
            Connection::Ready { effects, .. } => effects.clone(),
            #[cfg(any(test, feature = "test-support"))]
            Connection::Isolated => {
                Arc::new(crate::client::effects::ports::NoopCommitNotifications)
            }
            Connection::Pending => unreachable!("a source commits only once connected"),
        }
    }

    pub fn runtime_gate(
        &self,
    ) -> Result<Arc<dyn ports::RuntimeGate>, Arc<chimera_contracts::runtime_error::RuntimeError>>
    {
        match &*self.0.borrow() {
            Connection::Ready { runtime, .. } => Ok(runtime.clone()),
            #[cfg(any(test, feature = "test-support"))]
            Connection::Isolated => Ok(Arc::new(IsolatedRuntimeGate)),
            Connection::Pending => Err(Arc::new(
                chimera_contracts::runtime_error::RuntimeError::CheckUnavailable {
                    reason: chimera_contracts::runtime_error::RuntimeCheckUnavailable::NotChecked {
                        reason: "the runtime reconciler is not ready".into(),
                    },
                },
            )),
        }
    }

    pub fn ensure_ready(&self) -> Result<(), NotReady> {
        ensure!(
            !matches!(*self.0.borrow(), Connection::Pending),
            NotReadySnafu
        );
        Ok(())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct IsolatedRuntimeGate;

#[cfg(any(test, feature = "test-support"))]
#[async_trait::async_trait]
impl ports::RuntimeGate for IsolatedRuntimeGate {
    async fn check(
        &self,
        _: ports::RuntimeCandidate,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        Ok(())
    }
    fn committed(&self, _: chimera_contracts::mutation::ConfigDomain, _: crate::state::Version) {}
}

impl ProfileMutationPort for MutationCoordinator {
    fn ensure_ready(&self) -> anyhow::Result<()> {
        Ok(MutationCoordinator::ensure_ready(self)?)
    }
    fn runtime_gate(
        &self,
    ) -> Result<Arc<dyn RuntimeGate>, Arc<chimera_contracts::runtime_error::RuntimeError>> {
        MutationCoordinator::runtime_gate(self)
    }
    fn source_committed(
        &self,
        version: u64,
        reaches_runtime: bool,
        operation: Option<chimera_core_manager::OperationId>,
        files: Vec<chimera_config::profile::ProfileId>,
    ) -> chimera_contracts::runtime::CommitReceipt {
        use chimera_contracts::{
            mutation::ConfigDomain,
            runtime::{CommitReceipt, RuntimeCommitStatus},
        };
        self.effects().profiles_committed(files);
        if let Ok(gate) = self.runtime_gate() {
            if reaches_runtime {
                gate.committed(ConfigDomain::Profiles, crate::state::Version::new(version));
            } else {
                gate.source_only_committed(
                    ConfigDomain::Profiles,
                    crate::state::Version::new(version),
                );
            }
        }
        CommitReceipt {
            operation_id: operation.map(|id| id.to_string()),
            domain: "profiles".into(),
            source_version: version,
            runtime: RuntimeCommitStatus::Unchanged,
        }
    }
}

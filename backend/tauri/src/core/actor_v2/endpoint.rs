//! Lower core-host control endpoint.
//!
//! This is the staged Chimera counterpart of ref `actor_v2::endpoint`: mutating
//! calls are admitted as durable operation ids, while waiting is a separate
//! read that may be repeated after a caller is cancelled.

use std::{
    collections::{HashMap, VecDeque},
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use chimera_ipc::api::status::CoreState;
use futures::FutureExt;

use crate::{
    client::runtime::{RuntimeSnapshot, RuntimeTransformFailure},
    config::{chimera::ClashCore, clash::ClashInfo},
    core::clash::core::{CoreManager, RunType},
    enhance::PostProcessingOutput,
};

const OPERATION_HISTORY: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
    pub(crate) applied: Option<AppliedRuntimeIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationId(u64);

impl OperationId {
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationPhase {
    Running,
    Succeeded,
    Failed,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppliedRuntimeIdentity {
    pub(crate) revision: u64,
    pub(crate) core: ClashCore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperationOutput {
    Reconciled(AppliedRuntimeIdentity),
    Stopped,
    CoreChanged(AppliedRuntimeIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationInfo {
    pub(crate) id: OperationId,
    pub(crate) phase: OperationPhase,
    pub(crate) output: Option<OperationOutput>,
    pub(crate) error: Option<String>,
}

fn applied_identity_for_state(
    state: &CoreState,
    identity: Option<(u64, ClashCore)>,
) -> Option<AppliedRuntimeIdentity> {
    if !matches!(state, CoreState::Running) {
        return None;
    }
    identity.map(|(revision, core)| AppliedRuntimeIdentity { revision, core })
}

impl OperationInfo {
    fn running(id: OperationId) -> Self {
        Self {
            id,
            phase: OperationPhase::Running,
            output: None,
            error: None,
        }
    }

    fn succeeded(id: OperationId, output: OperationOutput) -> Self {
        Self {
            id,
            phase: OperationPhase::Succeeded,
            output: Some(output),
            error: None,
        }
    }

    fn failed(id: OperationId, error: String) -> Self {
        Self {
            id,
            phase: OperationPhase::Failed,
            output: None,
            error: Some(error),
        }
    }

    fn uncertain(id: OperationId, error: String) -> Self {
        Self {
            id,
            phase: OperationPhase::Uncertain,
            output: None,
            error: Some(error),
        }
    }

    pub(crate) fn is_terminal(&self) -> bool {
        self.phase != OperationPhase::Running
    }
}

pub(crate) enum CoreCommand {
    Reconcile {
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    },
    Stop,
    ChangeCore(ClashCore),
}

impl CoreCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Reconcile { .. } => "core reconcile",
            Self::Stop => "core stop",
            Self::ChangeCore(_) => "core selection",
        }
    }
}

#[async_trait]
pub(crate) trait ControlEndpoint: Send + Sync {
    /// Admit a mutation. The returned operation survives cancellation of the
    /// caller waiting on this method's result.
    async fn submit(&self, command: CoreCommand) -> anyhow::Result<OperationInfo>;

    /// Wait for an admitted operation to reach a terminal state. `None`
    /// means the id is unknown/evicted or the wait budget elapsed.
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo>;

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
}

#[derive(Debug, Default)]
struct OperationRegistryState {
    records: HashMap<OperationId, tokio::sync::watch::Receiver<OperationInfo>>,
    order: VecDeque<OperationId>,
}

#[derive(Debug)]
pub(crate) struct LocalEndpoint {
    manager: Arc<CoreManager>,
    next_id: AtomicU64,
    operations: parking_lot::Mutex<OperationRegistryState>,
}

impl LocalEndpoint {
    pub(crate) fn new() -> Self {
        Self {
            manager: Arc::new(CoreManager::new()),
            next_id: AtomicU64::new(1),
            operations: parking_lot::Mutex::new(OperationRegistryState::default()),
        }
    }

    fn insert_operation(
        &self,
        id: OperationId,
        receiver: tokio::sync::watch::Receiver<OperationInfo>,
    ) {
        let mut operations = self.operations.lock();
        operations.records.insert(id, receiver);
        operations.order.push_back(id);
        while operations.records.len() > OPERATION_HISTORY {
            let Some(position) = operations.order.iter().position(|candidate| {
                operations
                    .records
                    .get(candidate)
                    .is_some_and(|receiver| receiver.borrow().is_terminal())
            }) else {
                break;
            };
            if let Some(evicted) = operations.order.remove(position) {
                operations.records.remove(&evicted);
            }
        }
    }

    fn spawn_operation<F>(&self, operation: &'static str, future: F) -> OperationInfo
    where
        F: std::future::Future<Output = anyhow::Result<OperationOutput>> + Send + 'static,
    {
        let id = OperationId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let running = OperationInfo::running(id);
        let (tx, rx) = tokio::sync::watch::channel(running.clone());
        self.insert_operation(id, rx);
        tokio::spawn(async move {
            let info = match AssertUnwindSafe(future).catch_unwind().await {
                Ok(Ok(output)) => OperationInfo::succeeded(id, output),
                Ok(Err(error)) => OperationInfo::failed(id, error.to_string()),
                Err(_) => {
                    OperationInfo::uncertain(id, format!("{operation} panicked after admission"))
                }
            };
            tx.send_replace(info);
        });
        running
    }

    async fn execute(
        manager: Arc<CoreManager>,
        command: CoreCommand,
    ) -> anyhow::Result<OperationOutput> {
        let lease = manager.begin_lifecycle().await;
        match command {
            CoreCommand::Reconcile {
                clash,
                target_core,
                run_type,
            } => {
                lease
                    .rebuild_running_config_with(clash, target_core, run_type)
                    .await?;
                let (revision, core) = manager.applied_runtime_identity().ok_or_else(|| {
                    anyhow::anyhow!("reconcile succeeded without an applied runtime identity")
                })?;
                Ok(OperationOutput::Reconciled(AppliedRuntimeIdentity {
                    revision,
                    core,
                }))
            }
            CoreCommand::Stop => {
                lease.stop_core().await?;
                Ok(OperationOutput::Stopped)
            }
            CoreCommand::ChangeCore(core) => {
                lease.change_core(core).await?;
                let (revision, core) = manager.applied_runtime_identity().ok_or_else(|| {
                    anyhow::anyhow!("core change succeeded without an applied runtime identity")
                })?;
                Ok(OperationOutput::CoreChanged(AppliedRuntimeIdentity {
                    revision,
                    core,
                }))
            }
        }
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.manager.recovery_notify()
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        self.manager.runtime_transform_output()
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.manager.promoted_runtime_snapshot()
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.manager.runtime_transform_failure()
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        self.manager.effective_clash_info()
    }
}

#[async_trait]
impl ControlEndpoint for LocalEndpoint {
    async fn submit(&self, command: CoreCommand) -> anyhow::Result<OperationInfo> {
        let manager = self.manager.clone();
        let operation = command.name();
        Ok(self.spawn_operation(
            operation,
            async move { Self::execute(manager, command).await },
        ))
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        let mut receiver = self.operations.lock().records.get(&id)?.clone();
        let wait = async move {
            loop {
                let current = receiver.borrow().clone();
                if current.is_terminal() {
                    return current;
                }
                if receiver.changed().await.is_err() {
                    return receiver.borrow().clone();
                }
            }
        };
        tokio::time::timeout(timeout, wait).await.ok()
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at, run_type) = self.manager.status().await;
        let state = state.into_owned();
        let applied = applied_identity_for_state(&state, self.manager.applied_runtime_identity());
        Ok(CoreStatusSnapshot {
            state,
            state_changed_at,
            run_type,
            applied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_status_never_exposes_stale_applied_identity() {
        let identity = Some((7, ClashCore::Mihomo));
        assert_eq!(
            applied_identity_for_state(&CoreState::Stopped(None), identity),
            None
        );
        assert_eq!(
            applied_identity_for_state(&CoreState::Running, identity),
            Some(AppliedRuntimeIdentity {
                revision: 7,
                core: ClashCore::Mihomo,
            })
        );
    }

    #[tokio::test]
    async fn admitted_operation_survives_waiter_cancellation() {
        let endpoint = Arc::new(LocalEndpoint::new());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let admitted = endpoint.spawn_operation("test mutation", {
            let started = started.clone();
            let release = release.clone();
            async move {
                started.notify_one();
                release.notified().await;
                Ok(OperationOutput::Stopped)
            }
        });

        let waiter = {
            let endpoint = endpoint.clone();
            tokio::spawn(async move {
                endpoint
                    .wait_operation(admitted.id, Duration::from_secs(5))
                    .await
            })
        };
        started.notified().await;
        waiter.abort();
        let _ = waiter.await;

        release.notify_one();
        let terminal = endpoint
            .wait_operation(admitted.id, Duration::from_secs(1))
            .await
            .expect("admitted operation should remain queryable");
        assert_eq!(terminal.phase, OperationPhase::Succeeded);
        assert_eq!(terminal.output, Some(OperationOutput::Stopped));
    }

    #[tokio::test]
    async fn submitted_stop_persists_typed_terminal_output() {
        let endpoint = LocalEndpoint::new();
        let admitted = endpoint.submit(CoreCommand::Stop).await.unwrap();
        let terminal = endpoint
            .wait_operation(admitted.id, Duration::from_secs(1))
            .await
            .expect("stop operation should reach a terminal state");
        assert_eq!(terminal.phase, OperationPhase::Succeeded);
        assert_eq!(terminal.output, Some(OperationOutput::Stopped));
    }

    #[tokio::test]
    async fn operation_panic_is_persisted_as_uncertain_terminal_state() {
        let endpoint = LocalEndpoint::new();
        let admitted = endpoint.spawn_operation("test mutation", async {
            panic!("injected lower mutation panic");
            #[allow(unreachable_code)]
            Ok(OperationOutput::Stopped)
        });

        let terminal = endpoint
            .wait_operation(admitted.id, Duration::from_secs(1))
            .await
            .expect("panicked operation should reach a terminal state");
        assert_eq!(terminal.phase, OperationPhase::Uncertain);
        assert!(
            terminal
                .error
                .as_deref()
                .is_some_and(|error| error.contains("panicked after admission"))
        );
    }

    #[tokio::test]
    async fn terminal_operation_error_is_persisted_without_uncertain_phase() {
        let endpoint = LocalEndpoint::new();
        let admitted = endpoint.spawn_operation("test mutation", async {
            anyhow::bail!("expected terminal failure")
        });

        let terminal = endpoint
            .wait_operation(admitted.id, Duration::from_secs(1))
            .await
            .expect("failed operation should reach a terminal state");
        assert_eq!(terminal.phase, OperationPhase::Failed);
        assert_eq!(terminal.error.as_deref(), Some("expected terminal failure"));
    }
}

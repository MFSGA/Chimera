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
use chimera_ipc::api::{
    core::v2::{CoreApiConnection, CoreControllerInfo},
    status::CoreState,
};
use futures::FutureExt;

use crate::{
    client::runtime::{RuntimeSnapshot, RuntimeTransformFailure},
    config::{chimera::ClashCore, clash::ClashInfo},
    core::clash::core::{CoreManager, RunType},
    enhance::PostProcessingOutput,
};

const OPERATION_HISTORY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionHost {
    Local,
    Service,
}

pub(crate) type EndpointHandle = Arc<dyn ControlEndpoint>;

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
    pub(crate) applied: Option<AppliedRuntimeIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, specta::Type)]
#[serde(transparent)]
pub(crate) struct OperationId(u64);

impl OperationId {
    pub(crate) fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationPhase {
    Running,
    Succeeded,
    Failed,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub(crate) struct AppliedRuntimeIdentity {
    pub(crate) revision: u64,
    pub(crate) core: ClashCore,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationOutput {
    Reconciled(AppliedRuntimeIdentity),
    Stopped,
    CoreChanged(AppliedRuntimeIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
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
        profiles: crate::config::profile::profiles::Profiles,
        target_core: ClashCore,
        run_type: RunType,
        expected_applied: Option<u64>,
    },
    Stop,
    ChangeCore {
        profiles: crate::config::profile::profiles::Profiles,
        core: ClashCore,
    },
}

fn ensure_expected_applied(expected: Option<u64>, actual: Option<u64>) -> anyhow::Result<()> {
    anyhow::ensure!(
        expected == actual,
        "core revision conflict: expected applied revision {expected:?}, found {actual:?}"
    );
    Ok(())
}

impl CoreCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Reconcile { .. } => "core reconcile",
            Self::Stop => "core stop",
            Self::ChangeCore { .. } => "core selection",
        }
    }
}

#[async_trait]
pub(crate) trait ControlEndpoint: Send + Sync {
    fn host(&self) -> ExecutionHost;

    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
        Ok(None)
    }

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

    pub(crate) fn operation_info(&self, id: OperationId) -> Option<OperationInfo> {
        self.operations
            .lock()
            .records
            .get(&id)
            .map(|receiver| receiver.borrow().clone())
    }

    pub(crate) fn operation_history(&self) -> Vec<OperationInfo> {
        let operations = self.operations.lock();
        operations
            .order
            .iter()
            .filter_map(|id| {
                operations
                    .records
                    .get(id)
                    .map(|receiver| receiver.borrow().clone())
            })
            .collect()
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
                Ok(Err(error)) if crate::core::service::core_host::is_outcome_uncertain(&error) => {
                    OperationInfo::uncertain(id, error.to_string())
                }
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
                profiles,
                target_core,
                run_type,
                expected_applied,
            } => {
                let (state, _, _) = manager.status().await;
                let actual_applied = if matches!(state.as_ref(), CoreState::Running) {
                    manager
                        .applied_runtime_identity()
                        .map(|(revision, _)| revision)
                } else {
                    None
                };
                ensure_expected_applied(expected_applied, actual_applied)?;
                lease
                    .rebuild_running_config_with(clash, profiles, target_core, run_type)
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
            CoreCommand::ChangeCore { profiles, core } => {
                lease.change_core(profiles, core).await?;
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
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
        let status = self.status().await?;
        if !matches!(status.state, CoreState::Running) {
            return Ok(None);
        }
        let Some(applied) = status.applied else {
            return Ok(None);
        };
        let Some(info) = self.manager.applied_clash_info() else {
            return Ok(None);
        };
        let controller = if info.server.contains("://") {
            info.server
        } else {
            format!("http://{}", info.server)
        };
        Ok(Some(CoreApiConnection {
            instance_id: format!("local-{:016x}", applied.revision),
            controller: CoreControllerInfo::Http(controller),
            secret: info.secret,
        }))
    }

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

/// Compatibility service endpoint while Chimera still speaks the legacy
/// daemon `/core/start|stop|status` wire. It shares the local transaction
/// owner and operation registry so revision/recovery state remains singular.
#[derive(Debug)]
pub(crate) struct ServiceEndpoint {
    inner: Arc<LocalEndpoint>,
}

impl ServiceEndpoint {
    pub(crate) fn new(inner: Arc<LocalEndpoint>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ControlEndpoint for ServiceEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }

    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
        self.inner.manager.service_api_connection().await
    }

    async fn submit(&self, command: CoreCommand) -> anyhow::Result<OperationInfo> {
        if matches!(
            &command,
            CoreCommand::Reconcile { run_type, .. } if *run_type != RunType::Service
        ) {
            anyhow::bail!("service endpoint rejected a non-service reconcile");
        }
        self.inner.submit(command).await
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.inner.wait_operation(id, timeout).await
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.status().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_applied_revision_rejects_stale_or_missing_authority() {
        assert!(ensure_expected_applied(None, None).is_ok());
        assert!(ensure_expected_applied(Some(7), Some(7)).is_ok());
        assert!(ensure_expected_applied(Some(7), Some(8)).is_err());
        assert!(ensure_expected_applied(Some(7), None).is_err());
        assert!(ensure_expected_applied(None, Some(7)).is_err());
    }

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
    async fn service_endpoint_has_distinct_host_identity_and_shared_registry() {
        let local = Arc::new(LocalEndpoint::new());
        let service = ServiceEndpoint::new(local.clone());

        assert_eq!(local.host(), ExecutionHost::Local);
        assert_eq!(service.host(), ExecutionHost::Service);

        let admitted = service.submit(CoreCommand::Stop).await.unwrap();
        let terminal = service
            .wait_operation(admitted.id, Duration::from_secs(1))
            .await
            .expect("service endpoint stop should reach a terminal state");
        assert_eq!(terminal.phase, OperationPhase::Succeeded);
        assert_eq!(local.operation_info(admitted.id), Some(terminal));
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
    async fn operation_history_preserves_admission_order_and_id_lookup() {
        let endpoint = LocalEndpoint::new();
        let first = endpoint.spawn_operation("first", async { Ok(OperationOutput::Stopped) });
        let second = endpoint.spawn_operation("second", async { Ok(OperationOutput::Stopped) });

        let first_terminal = endpoint
            .wait_operation(first.id, Duration::from_secs(1))
            .await
            .expect("first operation should complete");
        let second_terminal = endpoint
            .wait_operation(second.id, Duration::from_secs(1))
            .await
            .expect("second operation should complete");

        assert_eq!(
            endpoint.operation_info(first.id),
            Some(first_terminal.clone())
        );
        assert_eq!(
            endpoint.operation_info(second.id),
            Some(second_terminal.clone())
        );
        assert_eq!(
            endpoint.operation_history(),
            vec![first_terminal, second_terminal]
        );
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

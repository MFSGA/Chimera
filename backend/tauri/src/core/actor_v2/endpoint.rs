//! Lower core-host control endpoint.
//!
//! This is the Chimera counterpart of ref `actor_v2::endpoint`: Local and
//! Service are explicit endpoint handles with one submission shape. Local owns
//! the legacy CoreManager during migration; Service admission, waiting, status,
//! and API capability are authoritative daemon-v2 calls.

use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use async_trait::async_trait;
use chimera_ipc::{
    api::{
        CoreErrorKind,
        core::v2::{
            CoreApiConnection, CoreCommandInfo, CoreControllerInfo, CoreOperationReq,
            CoreSubmitReq, OperationInfo as WireOperationInfo, OperationOutputInfo,
            OperationPhase as WireOperationPhase, ReconcileOutcomeInfo, ReconcileOutcomeKind,
            payload_digest,
        },
        status::{ConfigRevisionInfo, CoreHealthInfo, CoreState, RevisionIdInfo},
    },
    client::shortcuts::Client as ServiceClient,
};
use futures::FutureExt;

#[cfg(test)]
use chimera_ipc::api::status::CoreHealthState;

use crate::{
    client::runtime::{RuntimeDocument, RuntimeSnapshot, RuntimeTransformFailure},
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

#[derive(Debug)]
pub(crate) struct EndpointRegistry {
    operations: OperationRegistry,
    local: Arc<LocalEndpoint>,
    service: Arc<ServiceEndpoint>,
}

impl EndpointRegistry {
    pub(crate) fn new(
        service_client: ServiceClient<'static>,
    ) -> (Self, crate::core::clash::core::RuntimePreparation) {
        let operations = OperationRegistry::default();
        let (local, runtime_preparation) =
            LocalEndpoint::with_registry_and_preparation(operations.clone());
        let local = Arc::new(local);
        let service = Arc::new(ServiceEndpoint::new(operations.clone(), service_client));
        (
            Self {
                operations,
                local,
                service,
            },
            runtime_preparation,
        )
    }

    pub(crate) fn submission(
        &self,
        command: CoreCommand,
        local_document: Option<RuntimeDocument>,
    ) -> CoreSubmission {
        CoreSubmission {
            envelope: CoreCommandEnvelope {
                operation_id: self.operations.next_id(),
                command,
            },
            local_document,
        }
    }

    pub(crate) fn local(&self) -> Arc<LocalEndpoint> {
        self.local.clone()
    }

    pub(crate) fn endpoint(&self, host: ExecutionHost) -> EndpointHandle {
        match host {
            ExecutionHost::Local => self.local.clone(),
            ExecutionHost::Service => self.service.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
    pub(crate) health: Option<CoreHealthInfo>,
    pub(crate) applied: Option<RevisionIdInfo>,
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
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationOutput {
    Reconciled(ReconcileOutcomeInfo),
    Stopped,
    Recovered,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub(crate) struct OperationInfo {
    pub(crate) id: OperationId,
    pub(crate) phase: OperationPhase,
    pub(crate) output: Option<OperationOutput>,
    pub(crate) error: Option<String>,
    pub(crate) error_kind: Option<String>,
    pub(crate) retryable: bool,
}

fn runtime_revision_info(snapshot: &RuntimeSnapshot) -> ConfigRevisionInfo {
    let digest = hex::encode(snapshot.product_sha256);
    ConfigRevisionInfo {
        epoch: snapshot.revision.get(),
        generation: 1,
        source_hash: digest.clone(),
        effective_hash: digest,
    }
}

fn runtime_revision_id(snapshot: &RuntimeSnapshot) -> RevisionIdInfo {
    runtime_revision_info(snapshot).id()
}

fn applied_revision_for_state(
    state: &CoreState,
    snapshot: Option<Arc<RuntimeSnapshot>>,
) -> Option<RevisionIdInfo> {
    if !matches!(state, CoreState::Running) {
        return None;
    }
    snapshot.map(|snapshot| runtime_revision_id(&snapshot))
}

impl OperationInfo {
    fn running(id: OperationId) -> Self {
        Self {
            id,
            phase: OperationPhase::Running,
            output: None,
            error: None,
            error_kind: None,
            retryable: false,
        }
    }

    fn succeeded(id: OperationId, output: OperationOutput) -> Self {
        Self {
            id,
            phase: OperationPhase::Succeeded,
            output: Some(output),
            error: None,
            error_kind: None,
            retryable: false,
        }
    }

    fn failed(id: OperationId, error: String) -> Self {
        Self {
            id,
            phase: OperationPhase::Failed,
            output: None,
            error: Some(error),
            error_kind: None,
            retryable: false,
        }
    }

    fn uncertain(id: OperationId, error: String) -> Self {
        Self {
            id,
            phase: OperationPhase::Uncertain,
            output: None,
            error: Some(error),
            error_kind: None,
            retryable: false,
        }
    }

    pub(crate) fn is_terminal(&self) -> bool {
        self.phase != OperationPhase::Running
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CoreSpec {
    pub(crate) target_core: ClashCore,
    pub(crate) run_type: RunType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigInput {
    Inline {
        bytes: Vec<u8>,
        expected_digest: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ReconcileRequest {
    pub(crate) core: CoreSpec,
    pub(crate) config: ConfigInput,
    pub(crate) expected_applied: Option<RevisionIdInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct CoreCommandEnvelope {
    pub(crate) operation_id: OperationId,
    pub(crate) command: CoreCommand,
}

#[derive(Debug, Clone)]
pub(crate) struct CoreSubmission {
    pub(crate) envelope: CoreCommandEnvelope,
    /// Local-only deterministic build artifact. Revision allocation and
    /// publication remain owned by the Local manager after admission.
    pub(crate) local_document: Option<RuntimeDocument>,
}

#[derive(Debug, Clone)]
pub(crate) enum CoreCommand {
    Reconcile(Box<ReconcileRequest>),
    Stop,
    Recover,
}

fn ensure_expected_applied(
    expected: Option<RevisionIdInfo>,
    actual: Option<RevisionIdInfo>,
) -> anyhow::Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    anyhow::ensure!(
        Some(expected.clone()) == actual,
        "core revision conflict: expected applied revision {expected:?}, found {actual:?}"
    );
    Ok(())
}

fn validate_local_document(
    request: &ReconcileRequest,
    document: &RuntimeDocument,
) -> anyhow::Result<()> {
    let ConfigInput::Inline {
        bytes,
        expected_digest,
    } = &request.config;
    if let Some(expected_digest) = expected_digest {
        anyhow::ensure!(
            expected_digest == &payload_digest(bytes),
            "local reconcile inline config digest mismatch"
        );
    }
    anyhow::ensure!(
        request.core.target_core == document.target_core(),
        "local reconcile document core does not match the envelope core"
    );
    anyhow::ensure!(
        bytes.as_slice() == document.product_bytes(),
        "local reconcile document bytes do not match the envelope bytes"
    );
    Ok(())
}

impl CoreCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Reconcile(_) => "core reconcile",
            Self::Stop => "core stop",
            Self::Recover => "core recover",
        }
    }

    fn payload_digest(&self) -> String {
        match self {
            Self::Reconcile(request) => {
                let mut payload = format!(
                    "reconcile\0{:?}\0{:?}\0",
                    request.core.target_core, request.core.run_type
                )
                .into_bytes();
                if let Some(expected) = &request.expected_applied {
                    payload.extend_from_slice(
                        format!(
                            "{}:{}:{}",
                            expected.epoch, expected.generation, expected.effective_hash
                        )
                        .as_bytes(),
                    );
                }
                payload.extend_from_slice(b"\0");
                let ConfigInput::Inline {
                    bytes,
                    expected_digest,
                } = &request.config;
                payload.extend_from_slice(bytes);
                payload.extend_from_slice(b"\0digest:");
                payload
                    .extend_from_slice(expected_digest.as_deref().unwrap_or_default().as_bytes());
                payload_digest(&payload)
            }
            Self::Stop => payload_digest(b"stop"),
            Self::Recover => payload_digest(b"recover"),
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
    async fn submit(&self, submission: CoreSubmission) -> anyhow::Result<OperationInfo>;

    /// Wait for an admitted operation to reach a terminal state. `None`
    /// means the id is unknown/evicted or the wait budget elapsed.
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo>;

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
}

#[derive(Debug)]
struct RegisteredOperation {
    digest: String,
    sender: tokio::sync::watch::Sender<OperationInfo>,
}

#[derive(Debug, Default)]
struct OperationRegistryState {
    records: HashMap<OperationId, RegisteredOperation>,
    order: VecDeque<OperationId>,
}

#[derive(Debug, Clone)]
struct OperationRegistry {
    next_id: Arc<AtomicU64>,
    state: Arc<parking_lot::Mutex<OperationRegistryState>>,
}

enum OperationAdmission {
    Existing(OperationInfo),
    Registered(tokio::sync::watch::Sender<OperationInfo>),
}

impl Default for OperationRegistry {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seed = (now as u64) ^ ((now >> 64) as u64) ^ ((std::process::id() as u64) << 32) ^ 1;
        Self {
            next_id: Arc::new(AtomicU64::new(seed)),
            state: Arc::new(parking_lot::Mutex::new(OperationRegistryState::default())),
        }
    }
}

impl OperationRegistry {
    fn next_id(&self) -> OperationId {
        OperationId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn admit(
        &self,
        id: OperationId,
        digest: String,
        info: OperationInfo,
    ) -> anyhow::Result<OperationAdmission> {
        let mut operations = self.state.lock();
        if let Some(operation) = operations.records.get(&id) {
            anyhow::ensure!(
                operation.digest == digest,
                "core operation id conflict: operation {id:?} was already admitted with different work"
            );
            return Ok(OperationAdmission::Existing(
                operation.sender.borrow().clone(),
            ));
        }

        let (sender, _receiver) = tokio::sync::watch::channel(info);
        operations.records.insert(
            id,
            RegisteredOperation {
                digest,
                sender: sender.clone(),
            },
        );
        operations.order.push_back(id);
        while operations.records.len() > OPERATION_HISTORY {
            let Some(position) = operations.order.iter().position(|candidate| {
                operations
                    .records
                    .get(candidate)
                    .is_some_and(|operation| operation.sender.borrow().is_terminal())
            }) else {
                break;
            };
            if let Some(evicted) = operations.order.remove(position) {
                operations.records.remove(&evicted);
            }
        }
        Ok(OperationAdmission::Registered(sender))
    }

    fn existing(&self, id: OperationId, digest: &str) -> anyhow::Result<Option<OperationInfo>> {
        let operations = self.state.lock();
        let Some(operation) = operations.records.get(&id) else {
            return Ok(None);
        };
        anyhow::ensure!(
            operation.digest == digest,
            "core operation id conflict: operation {id:?} was already admitted with different work"
        );
        Ok(Some(operation.sender.borrow().clone()))
    }

    fn info(&self, id: OperationId) -> Option<OperationInfo> {
        self.state
            .lock()
            .records
            .get(&id)
            .map(|operation| operation.sender.borrow().clone())
    }

    fn history(&self) -> Vec<OperationInfo> {
        let operations = self.state.lock();
        operations
            .order
            .iter()
            .filter_map(|id| {
                operations
                    .records
                    .get(id)
                    .map(|operation| operation.sender.borrow().clone())
            })
            .collect()
    }

    fn receiver(&self, id: OperationId) -> Option<tokio::sync::watch::Receiver<OperationInfo>> {
        self.state
            .lock()
            .records
            .get(&id)
            .map(|operation| operation.sender.subscribe())
    }

    fn update(&self, id: OperationId, info: OperationInfo) {
        if let Some(operation) = self.state.lock().records.get(&id) {
            operation.sender.send_replace(info);
        }
    }
}

#[derive(Debug)]
pub(crate) struct LocalEndpoint {
    manager: Arc<CoreManager>,
    operations: OperationRegistry,
}

impl LocalEndpoint {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_registry(OperationRegistry::default())
    }

    fn with_registry(operations: OperationRegistry) -> Self {
        Self::with_registry_and_preparation(operations).0
    }

    fn with_registry_and_preparation(
        operations: OperationRegistry,
    ) -> (Self, crate::core::clash::core::RuntimePreparation) {
        let manager = Arc::new(CoreManager::new());
        let runtime_preparation = manager.runtime_preparation();
        (
            Self {
                manager,
                operations,
            },
            runtime_preparation,
        )
    }

    pub(crate) fn operation_info(&self, id: OperationId) -> Option<OperationInfo> {
        self.operations.info(id)
    }

    pub(crate) fn operation_history(&self) -> Vec<OperationInfo> {
        self.operations.history()
    }

    fn spawn_operation<F>(
        &self,
        id: OperationId,
        digest: String,
        operation: &'static str,
        future: F,
    ) -> anyhow::Result<OperationInfo>
    where
        F: std::future::Future<Output = anyhow::Result<OperationOutput>> + Send + 'static,
    {
        let running = OperationInfo::running(id);
        let sender = match self.operations.admit(id, digest, running.clone())? {
            OperationAdmission::Existing(info) => return Ok(info),
            OperationAdmission::Registered(sender) => sender,
        };
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
            sender.send_replace(info);
        });
        Ok(running)
    }

    async fn execute(
        manager: Arc<CoreManager>,
        command: CoreCommand,
        local_document: Option<RuntimeDocument>,
    ) -> anyhow::Result<OperationOutput> {
        let lease = manager.begin_lifecycle().await;
        match command {
            CoreCommand::Reconcile(request) => {
                anyhow::ensure!(
                    request.core.run_type != RunType::Service,
                    "local endpoint rejected a Service reconcile"
                );
                let local_document = local_document
                    .context("local reconcile requires the prepared runtime document")?;
                let (state, _, _) = manager.status().await;
                let previous_snapshot = manager.applied_runtime_snapshot();
                let actual_applied = if matches!(state.as_ref(), CoreState::Running) {
                    previous_snapshot.as_deref().map(runtime_revision_id)
                } else {
                    None
                };
                ensure_expected_applied(request.expected_applied.clone(), actual_applied)?;
                validate_local_document(&request, &local_document)?;
                let revision = manager.allocate_runtime_revision()?;
                let snapshot = Arc::new(local_document.into_snapshot(revision));
                lease
                    .apply_runtime_snapshot(snapshot.clone(), request.core.run_type)
                    .await?;
                let outcome = if !matches!(state.as_ref(), CoreState::Running) {
                    ReconcileOutcomeKind::Started
                } else if previous_snapshot
                    .as_deref()
                    .is_some_and(|previous| previous.target_core != snapshot.target_core)
                {
                    ReconcileOutcomeKind::Switched
                } else {
                    ReconcileOutcomeKind::Restarted
                };
                Ok(OperationOutput::Reconciled(ReconcileOutcomeInfo {
                    outcome,
                    revision: runtime_revision_info(&snapshot),
                    warning: None,
                    failed_apply: None,
                }))
            }
            CoreCommand::Stop => {
                lease.stop_core().await?;
                Ok(OperationOutput::Stopped)
            }
            CoreCommand::Recover => {
                anyhow::bail!("local endpoint does not support Service recovery")
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

    pub(crate) fn discard_runtime_draft(&self) {
        self.manager.discard_runtime_draft();
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
        let Some(revision) = status.applied else {
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
            instance_id: format!("local-{:016x}-{:016x}", revision.epoch, revision.generation),
            controller: CoreControllerInfo::Http(controller),
            secret: info.secret,
        }))
    }

    async fn submit(&self, submission: CoreSubmission) -> anyhow::Result<OperationInfo> {
        let CoreSubmission {
            envelope,
            local_document,
        } = submission;
        let CoreCommandEnvelope {
            operation_id: id,
            command,
        } = envelope;
        let digest = command.payload_digest();
        let manager = self.manager.clone();
        let operation = command.name();
        self.spawn_operation(id, digest, operation, async move {
            Self::execute(manager, command, local_document).await
        })
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        let mut receiver = self.operations.receiver(id)?;
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
        let revision = applied_revision_for_state(&state, self.manager.applied_runtime_snapshot());
        Ok(CoreStatusSnapshot {
            state,
            state_changed_at,
            run_type,
            health: None,
            applied: revision,
        })
    }
}

fn wire_operation_id(id: OperationId) -> String {
    format!("{:032x}", id.get())
}

fn service_wire_command(command: &CoreCommand) -> anyhow::Result<CoreCommandInfo<'static>> {
    Ok(match command {
        CoreCommand::Reconcile(request) => {
            anyhow::ensure!(
                request.core.run_type == RunType::Service,
                "service endpoint rejected a non-service reconcile"
            );
            let ConfigInput::Inline {
                bytes,
                expected_digest,
            } = &request.config;
            if let Some(expected_digest) = expected_digest {
                anyhow::ensure!(
                    expected_digest == &payload_digest(bytes),
                    "service reconcile inline config digest mismatch"
                );
            }
            let config =
                String::from_utf8(bytes.clone()).context("prepared runtime config is not UTF-8")?;
            CoreCommandInfo::Reconcile {
                core_type: Cow::Owned((&request.core.target_core).into()),
                expected_digest: expected_digest.clone().map(Cow::Owned),
                config: Cow::Owned(config),
                expected_applied: request.expected_applied.clone(),
            }
        }
        CoreCommand::Stop => CoreCommandInfo::Stop,
        CoreCommand::Recover => CoreCommandInfo::Recover,
    })
}

fn map_wire_operation(id: OperationId, wire: WireOperationInfo) -> anyhow::Result<OperationInfo> {
    anyhow::ensure!(
        wire.id == wire_operation_id(id),
        "service operation id mismatch: expected {}, got {}",
        wire_operation_id(id),
        wire.id
    );
    let mut phase = match wire.phase {
        WireOperationPhase::Queued | WireOperationPhase::Running => OperationPhase::Running,
        WireOperationPhase::Succeeded => OperationPhase::Succeeded,
        WireOperationPhase::Failed => OperationPhase::Failed,
    };
    let (mut error, mut error_kind, mut retryable) = match wire.error {
        Some(error) => (
            Some(error.message),
            error.kind.map(|kind| kind.into_owned()),
            error.retryable,
        ),
        None => (None, None, false),
    };
    let output = match wire.output {
        Some(OperationOutputInfo::Reconciled(outcome))
            if outcome.outcome == ReconcileOutcomeKind::RolledBack =>
        {
            phase = OperationPhase::Failed;
            if error.is_none() {
                error = Some(format!(
                    "service reconcile rolled back; the desired runtime was not applied: {}",
                    outcome.failed_apply.as_deref().unwrap_or("unknown reason")
                ));
            }
            if error_kind.is_none() {
                error_kind = Some(CoreErrorKind::ApplyFailed.as_str().to_owned());
                retryable = false;
            }
            None
        }
        Some(OperationOutputInfo::Reconciled(outcome)) => {
            if let Some(warning) = outcome.warning.as_deref() {
                tracing::warn!("service reconcile completed with warning: {warning}");
            }
            Some(OperationOutput::Reconciled(outcome))
        }
        Some(OperationOutputInfo::Stopped) => Some(OperationOutput::Stopped),
        Some(OperationOutputInfo::Recovered) => Some(OperationOutput::Recovered),
        None => None,
    };
    if phase == OperationPhase::Succeeded {
        anyhow::ensure!(
            output.is_some(),
            "service operation succeeded without a terminal output"
        );
    }
    Ok(OperationInfo {
        id,
        phase,
        output,
        error,
        error_kind,
        retryable,
    })
}

fn map_service_status(
    infos: chimera_ipc::api::status::CoreInfos,
) -> anyhow::Result<CoreStatusSnapshot> {
    let revision = if matches!(infos.state, CoreState::Running) {
        Some(
            infos
                .revision
                .as_ref()
                .map(chimera_ipc::api::status::ConfigRevisionInfo::id)
                .context("running Service core did not publish an applied revision")?,
        )
    } else {
        None
    };
    Ok(CoreStatusSnapshot {
        state: infos.state,
        state_changed_at: infos.state_changed_at,
        run_type: RunType::Service,
        health: infos.health,
        applied: revision,
    })
}

/// Ref-shaped Service endpoint: admission, operation waiting, status and API
/// capability are all authoritative daemon-v2 calls. The shared registry is
/// only an app-side observation cache; it does not execute Service mutations.
pub(crate) struct ServiceEndpoint {
    operations: OperationRegistry,
    client: ServiceClient<'static>,
}

impl std::fmt::Debug for ServiceEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceEndpoint").finish_non_exhaustive()
    }
}

impl ServiceEndpoint {
    fn new(operations: OperationRegistry, client: ServiceClient<'static>) -> Self {
        Self { operations, client }
    }
}

#[async_trait]
impl ControlEndpoint for ServiceEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }

    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
        self.client
            .core_api_v2()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn submit(&self, submission: CoreSubmission) -> anyhow::Result<OperationInfo> {
        let id = submission.envelope.operation_id;
        let digest = submission.envelope.command.payload_digest();
        if let Some(existing) = self.operations.existing(id, &digest)? {
            return Ok(existing);
        }
        let request = CoreSubmitReq {
            operation_id: Cow::Owned(wire_operation_id(id)),
            command: service_wire_command(&submission.envelope.command)?,
        };
        let wire = self
            .client
            .submit_core_v2(&request)
            .await
            .map_err(crate::core::service::core_host::submit_error)?;
        let info = map_wire_operation(id, wire)?;
        match self.operations.admit(id, digest, info.clone())? {
            OperationAdmission::Existing(existing) => Ok(existing),
            OperationAdmission::Registered(_) => Ok(info),
        }
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        if let Some(info) = self.operations.info(id)
            && info.is_terminal()
        {
            return Some(info);
        }
        let request = CoreOperationReq {
            operation_id: Cow::Owned(wire_operation_id(id)),
            wait_ms: Some(timeout.as_millis().min(u128::from(u64::MAX)) as u64),
        };
        let wire = self.client.core_operation_v2(&request).await.ok()?;
        let info = map_wire_operation(id, wire).ok()?;
        self.operations.update(id, info.clone());
        Some(info)
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let status = self
            .client
            .core_status_v2()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        map_service_status(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::runtime::{RuntimeRevision, RuntimeSnapshotData};

    fn revision(epoch: u64) -> RevisionIdInfo {
        RevisionIdInfo {
            epoch,
            generation: 1,
            effective_hash: format!("hash-{epoch}"),
        }
    }

    fn snapshot(revision: u64, core: ClashCore) -> Arc<RuntimeSnapshot> {
        Arc::new(RuntimeSnapshot::new(
            RuntimeRevision::from_raw(revision),
            core,
            b"mode: rule\n".to_vec(),
            serde_yaml::Mapping::new(),
        ))
    }

    fn submission(id: u64, command: CoreCommand) -> CoreSubmission {
        CoreSubmission {
            envelope: CoreCommandEnvelope {
                operation_id: OperationId::from_raw(id),
                command,
            },
            local_document: None,
        }
    }

    fn document(runtime: &RuntimeSnapshot) -> RuntimeDocument {
        RuntimeDocument::from_data(
            runtime.target_core,
            runtime.product_bytes().to_vec().into(),
            RuntimeSnapshotData {
                config: runtime.config.clone(),
                exists_keys: runtime.exists_keys.clone(),
                postprocessing_output: runtime.postprocessing_output.clone(),
                inspection: runtime.inspection.clone(),
            },
        )
    }

    fn reconcile_command(
        runtime: &RuntimeSnapshot,
        run_type: RunType,
        expected_applied: Option<RevisionIdInfo>,
    ) -> CoreCommand {
        let bytes = runtime.product_bytes().to_vec();
        CoreCommand::Reconcile(Box::new(ReconcileRequest {
            core: CoreSpec {
                target_core: runtime.target_core,
                run_type,
            },
            config: ConfigInput::Inline {
                expected_digest: Some(payload_digest(&bytes)),
                bytes,
            },
            expected_applied,
        }))
    }

    #[test]
    fn expected_applied_revision_rejects_stale_or_missing_authority() {
        assert!(ensure_expected_applied(None, None).is_ok());
        assert!(ensure_expected_applied(None, Some(revision(7))).is_ok());
        assert!(ensure_expected_applied(Some(revision(7)), Some(revision(7))).is_ok());
        assert!(ensure_expected_applied(Some(revision(7)), Some(revision(8))).is_err());
        assert!(ensure_expected_applied(Some(revision(7)), None).is_err());
    }

    #[test]
    fn stopped_status_never_exposes_stale_applied_revision() {
        let runtime = snapshot(7, ClashCore::Mihomo);
        assert_eq!(
            applied_revision_for_state(&CoreState::Stopped(None), Some(runtime.clone())),
            None
        );
        assert_eq!(
            applied_revision_for_state(&CoreState::Running, Some(runtime.clone())),
            Some(runtime_revision_id(&runtime))
        );
    }

    #[tokio::test]
    async fn admitted_operation_survives_waiter_cancellation() {
        let endpoint = Arc::new(LocalEndpoint::new());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let admitted = endpoint
            .spawn_operation(OperationId::from_raw(1), "test".into(), "test mutation", {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(OperationOutput::Stopped)
                }
            })
            .expect("test operation should be admitted");

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

    #[test]
    fn wire_operation_id_matches_daemon_contract() {
        let id = wire_operation_id(OperationId::from_raw(0x2a));
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(id, id.to_ascii_lowercase());
    }

    #[test]
    fn service_reconcile_wire_carries_runtime_digest_and_cas() {
        let runtime = snapshot(7, ClashCore::Mihomo);
        let expected = revision(3);
        let command = reconcile_command(&runtime, RunType::Service, Some(expected.clone()));

        let CoreCommandInfo::Reconcile {
            core_type,
            config,
            expected_digest,
            expected_applied,
        } = service_wire_command(&command).unwrap()
        else {
            panic!("expected reconcile wire command");
        };

        assert_eq!(core_type.into_owned(), (&ClashCore::Mihomo).into());
        assert_eq!(config.as_bytes(), runtime.product_bytes());
        assert_eq!(
            expected_digest.as_deref(),
            Some(payload_digest(runtime.product_bytes()).as_str())
        );
        assert_eq!(expected_applied, Some(expected));
    }

    #[test]
    fn local_document_validation_fails_closed_on_core_or_bytes_divergence() {
        let runtime = snapshot(7, ClashCore::Mihomo);
        let document = document(&runtime);
        let CoreCommand::Reconcile(request) = reconcile_command(&runtime, RunType::Normal, None)
        else {
            unreachable!();
        };
        assert!(validate_local_document(&request, &document).is_ok());

        let mut bytes_mismatch = request.clone();
        let ConfigInput::Inline { bytes, .. } = &mut bytes_mismatch.config;
        *bytes = b"mode: global\n".to_vec();
        assert!(
            validate_local_document(&bytes_mismatch, &document)
                .unwrap_err()
                .to_string()
                .contains("digest mismatch")
        );

        let wrong_core = RuntimeDocument::from_data(
            ClashCore::ClashRs,
            runtime.product_bytes().to_vec().into(),
            RuntimeSnapshotData {
                config: runtime.config.clone(),
                exists_keys: runtime.exists_keys.clone(),
                postprocessing_output: runtime.postprocessing_output.clone(),
                inspection: runtime.inspection.clone(),
            },
        );
        assert!(
            validate_local_document(&request, &wrong_core)
                .unwrap_err()
                .to_string()
                .contains("document core")
        );
    }

    #[test]
    fn service_reconcile_wire_allows_unconditional_cas_and_optional_digest() {
        let runtime = snapshot(7, ClashCore::Mihomo);
        let mut command = reconcile_command(&runtime, RunType::Service, None);
        let CoreCommand::Reconcile(request) = &mut command else {
            unreachable!();
        };
        let ConfigInput::Inline {
            expected_digest, ..
        } = &mut request.config;
        *expected_digest = None;

        let CoreCommandInfo::Reconcile {
            expected_digest,
            expected_applied,
            ..
        } = service_wire_command(&command).unwrap()
        else {
            panic!("expected reconcile wire command");
        };

        assert_eq!(expected_digest, None);
        assert_eq!(expected_applied, None);
    }

    #[test]
    fn service_wire_terminal_output_maps_to_shared_operation_shape() {
        let id = OperationId::from_raw(9);
        let revision = chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 4,
            generation: 2,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        };
        let wire = WireOperationInfo::succeeded(
            wire_operation_id(id),
            OperationOutputInfo::Reconciled(chimera_ipc::api::core::v2::ReconcileOutcomeInfo {
                outcome: chimera_ipc::api::core::v2::ReconcileOutcomeKind::Restarted,
                revision: revision.clone(),
                warning: None,
                failed_apply: None,
            }),
        );

        let mapped = map_wire_operation(id, wire).unwrap();
        assert_eq!(mapped.phase, OperationPhase::Succeeded);
        assert_eq!(
            mapped.output,
            Some(OperationOutput::Reconciled(
                chimera_ipc::api::core::v2::ReconcileOutcomeInfo {
                    outcome: chimera_ipc::api::core::v2::ReconcileOutcomeKind::Restarted,
                    revision,
                    warning: None,
                    failed_apply: None,
                }
            ))
        );
    }

    #[test]
    fn service_noop_reconcile_preserves_structured_outcome_and_warning() {
        let id = OperationId::from_raw(10);
        let revision = chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 4,
            generation: 2,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        };
        let wire = WireOperationInfo::succeeded(
            wire_operation_id(id),
            OperationOutputInfo::Reconciled(chimera_ipc::api::core::v2::ReconcileOutcomeInfo {
                outcome: ReconcileOutcomeKind::Noop,
                revision: revision.clone(),
                warning: Some("durability warning".into()),
                failed_apply: None,
            }),
        );

        let mapped = map_wire_operation(id, wire).unwrap();
        assert_eq!(mapped.phase, OperationPhase::Succeeded);
        assert_eq!(
            mapped.output,
            Some(OperationOutput::Reconciled(ReconcileOutcomeInfo {
                outcome: ReconcileOutcomeKind::Noop,
                revision,
                warning: Some("durability warning".into()),
                failed_apply: None,
            }))
        );
    }

    #[test]
    fn service_terminal_error_preserves_kind_and_retryable() {
        let id = OperationId::from_raw(10);
        let wire = WireOperationInfo::failed_with_kind(
            wire_operation_id(id),
            Some(CoreErrorKind::BackendUnavailable),
            "controller endpoint is reconnecting",
            true,
        );

        let mapped = map_wire_operation(id, wire).unwrap();

        assert_eq!(mapped.phase, OperationPhase::Failed);
        assert_eq!(
            mapped.error_kind.as_deref(),
            Some(CoreErrorKind::BackendUnavailable.as_str())
        );
        assert!(mapped.retryable);
        assert_eq!(
            mapped.error.as_deref(),
            Some("controller endpoint is reconnecting")
        );
    }

    #[test]
    fn service_rolled_back_reconcile_is_not_reported_as_success() {
        let id = OperationId::from_raw(10);
        let revision = chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 4,
            generation: 2,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        };
        let wire = WireOperationInfo::succeeded(
            wire_operation_id(id),
            OperationOutputInfo::Reconciled(chimera_ipc::api::core::v2::ReconcileOutcomeInfo {
                outcome: ReconcileOutcomeKind::RolledBack,
                revision,
                warning: None,
                failed_apply: Some("desired runtime failed readiness".into()),
            }),
        );

        let mapped = map_wire_operation(id, wire).unwrap();
        assert_eq!(mapped.phase, OperationPhase::Failed);
        assert!(mapped.output.is_none());
        assert!(mapped.error.as_deref().is_some_and(|error| {
            error.contains("rolled back") && error.contains("desired runtime failed readiness")
        }));
        assert_eq!(
            mapped.error_kind.as_deref(),
            Some(CoreErrorKind::ApplyFailed.as_str())
        );
        assert!(!mapped.retryable);
    }

    #[test]
    fn service_status_uses_daemon_revision_without_trusting_type_echo() {
        let revision = chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 5,
            generation: 3,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        };
        let status = map_service_status(chimera_ipc::api::status::CoreInfos {
            r#type: None,
            state: CoreState::Running,
            state_changed_at: 42,
            config_path: None,
            health: Some(CoreHealthInfo {
                state: CoreHealthState::Healthy,
                changed_at: 41,
                consecutive_failures: 0,
                last_error: None,
                last_success_at: Some(42),
            }),
            revision: Some(revision.clone()),
        })
        .unwrap();

        assert_eq!(status.run_type, RunType::Service);
        assert_eq!(
            status.health.as_ref().map(|health| health.state),
            Some(CoreHealthState::Healthy)
        );
        assert_eq!(status.applied, Some(revision.id()));
    }

    #[test]
    fn running_service_without_revision_fails_closed() {
        let error = map_service_status(chimera_ipc::api::status::CoreInfos {
            r#type: None,
            state: CoreState::Running,
            state_changed_at: 42,
            config_path: None,
            health: None,
            revision: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("applied revision"));
    }

    #[test]
    fn local_and_service_endpoints_share_only_the_operation_observation_registry() {
        let operations = OperationRegistry::default();
        let local = LocalEndpoint::with_registry(operations.clone());
        let service = ServiceEndpoint::new(
            operations.clone(),
            ServiceClient::new(chimera_ipc::SERVICE_PLACEHOLDER),
        );
        assert_eq!(local.host(), ExecutionHost::Local);
        assert_eq!(service.host(), ExecutionHost::Service);

        let id = operations.next_id();
        let info = OperationInfo::running(id);
        let admitted = service
            .operations
            .admit(id, "test".into(), info.clone())
            .expect("shared observation should admit");
        assert!(matches!(admitted, OperationAdmission::Registered(_)));
        assert_eq!(local.operation_info(id), Some(info));
    }

    #[tokio::test]
    async fn submitted_stop_persists_typed_terminal_output() {
        let endpoint = LocalEndpoint::new();
        let admitted = endpoint
            .submit(submission(1, CoreCommand::Stop))
            .await
            .unwrap();
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
        let first = endpoint
            .spawn_operation(OperationId::from_raw(1), "first".into(), "first", async {
                Ok(OperationOutput::Stopped)
            })
            .expect("first operation should be admitted");
        let second = endpoint
            .spawn_operation(OperationId::from_raw(2), "second".into(), "second", async {
                Ok(OperationOutput::Stopped)
            })
            .expect("second operation should be admitted");

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

    #[test]
    fn concurrent_same_id_admission_registers_exactly_once() {
        let registry = OperationRegistry::default();
        let id = OperationId::from_raw(41);
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let handles = (0..16)
            .map(|_| {
                let registry = registry.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    registry
                        .admit(id, "same-work".into(), OperationInfo::running(id))
                        .expect("same work should attach or register")
                })
            })
            .collect::<Vec<_>>();

        let mut registered = 0;
        let mut existing = 0;
        for handle in handles {
            match handle.join().expect("admission thread should not panic") {
                OperationAdmission::Registered(_) => registered += 1,
                OperationAdmission::Existing(info) => {
                    existing += 1;
                    assert_eq!(info.id, id);
                }
            }
        }

        assert_eq!(registered, 1);
        assert_eq!(existing, 15);
        assert_eq!(registry.history(), vec![OperationInfo::running(id)]);
    }

    #[tokio::test]
    async fn local_submit_replays_same_operation_id_and_rejects_different_work() {
        let endpoint = LocalEndpoint::new();
        let id = 41;
        let first = endpoint
            .submit(submission(id, CoreCommand::Stop))
            .await
            .expect("first submit should be admitted");
        let replay = endpoint
            .submit(submission(id, CoreCommand::Stop))
            .await
            .expect("same envelope should replay");
        assert_eq!(replay, first);

        let error = endpoint
            .submit(submission(id, CoreCommand::Recover))
            .await
            .expect_err("same operation id with different work must conflict");
        assert!(
            error
                .to_string()
                .contains("already admitted with different work")
        );
    }

    #[tokio::test]
    async fn local_endpoint_rejects_service_lifecycle_commands() {
        let manager = Arc::new(CoreManager::new());
        let runtime = snapshot(7, ClashCore::Mihomo);

        let reconcile = LocalEndpoint::execute(
            manager.clone(),
            reconcile_command(&runtime, RunType::Service, None),
            None,
        )
        .await
        .expect_err("Local endpoint must not accept Service reconcile");
        assert!(
            reconcile
                .to_string()
                .contains("rejected a Service reconcile")
        );

        let recover = LocalEndpoint::execute(manager, CoreCommand::Recover, None)
            .await
            .expect_err("Local endpoint must not proxy Service recovery");
        assert!(
            recover
                .to_string()
                .contains("does not support Service recovery")
        );
    }

    #[test]
    fn reconcile_identity_digest_includes_cas_and_declared_digest() {
        let runtime = snapshot(7, ClashCore::Mihomo);
        let base = reconcile_command(&runtime, RunType::Normal, None);
        let with_cas = reconcile_command(&runtime, RunType::Normal, Some(revision(3)));
        assert_ne!(base.payload_digest(), with_cas.payload_digest());

        let mut corrected = base.clone();
        let CoreCommand::Reconcile(request) = &mut corrected else {
            unreachable!();
        };
        let ConfigInput::Inline {
            expected_digest, ..
        } = &mut request.config;
        *expected_digest = Some("corrected-claim".into());
        assert_ne!(base.payload_digest(), corrected.payload_digest());
    }

    #[tokio::test]
    async fn operation_panic_is_persisted_as_uncertain_terminal_state() {
        let endpoint = LocalEndpoint::new();
        let admitted = endpoint
            .spawn_operation(
                OperationId::from_raw(1),
                "panic".into(),
                "test mutation",
                async {
                    panic!("injected lower mutation panic");
                    #[allow(unreachable_code)]
                    Ok(OperationOutput::Stopped)
                },
            )
            .expect("test operation should be admitted");

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
        let admitted = endpoint
            .spawn_operation(
                OperationId::from_raw(1),
                "failure".into(),
                "test mutation",
                async { anyhow::bail!("expected terminal failure") },
            )
            .expect("test operation should be admitted");

        let terminal = endpoint
            .wait_operation(admitted.id, Duration::from_secs(1))
            .await
            .expect("failed operation should reach a terminal state");
        assert_eq!(terminal.phase, OperationPhase::Failed);
        assert_eq!(terminal.error.as_deref(), Some("expected terminal failure"));
    }
}

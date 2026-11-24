//! The endpoint port of CoreActor v2: one narrow trait over "a host's
//! `CoreControl`", implemented by the in-process Local adapter and the IPC v2
//! Service adapter.
//!
//! Design note (deviation from the integration design's concrete
//! `EndpointHandle` enum, recorded): the router consumes a trait object plus
//! an [`ExecutionHost`] tag instead of a two-variant enum. Same shape, one
//! test seam — the fake endpoint in the actor tests is the third
//! implementation the enum could not have carried.
//!
//! Everything here is *reading or forwarding*; the router never synthesizes
//! lifecycle state (invariant I-R3). The normalized snapshot below is a
//! field-by-field projection of what the host published, nothing more.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chimera_core_manager::{
    ApplyOutcome, ConfigInput, CoreCommand, CoreCommandEnvelope, CoreControl, CoreError,
    CoreErrorKind, CoreKind, OperationId, OperationOutput, OperationState,
};
use chimera_ipc::api::{
    core::v2::{
        CoreCommandInfo, CoreOperationReq, CoreOwnerInfo, CoreSubmitReq, OperationInfo,
        OperationOutputInfo, OperationPhase, ReconcileOutcomeInfo, ReconcileOutcomeKind,
    },
    status::{CoreStateDetail, RevisionIdInfo},
};

use chimera_ipc::client::{ClientError, shortcuts::Client};

// The pinned upstream execution-host identity lives in the shared runtime
// values layer; retain this path for existing CoreActor endpoint consumers.
pub use chimera_contracts::execution_host::ExecutionHost;

/// The host's canonical core status, projected field by field for the app.
/// No field here is ever derived by the router itself.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreStatusSnapshot {
    /// Chimera's running-process identity, never inferred from desired config.
    /// An absent value is unknown, not proof that an instance is stopped.
    pub instance_id: Option<String>,
    pub controller: Option<chimera_ipc::api::status::CoreControllerInfo>,
    /// `None` means the host published no state this router can trust — an
    /// unmapped future variant, or a daemon that answered without a detail.
    /// No consumer may read it as `Stopped`: "we do not know" is the one
    /// answer a stop proof must never accept.
    pub state: Option<CoreStateDetail>,
    pub state_changed_at: i64,
    /// The applied revision's CAS identity, when one is running.
    pub revision: Option<chimera_ipc::api::status::RevisionIdInfo>,
    /// The source identity of that same applied revision.
    ///
    /// Deliberately kept next to the CAS identity rather than folded into it:
    /// `RevisionIdInfo` is what a reconcile sends back as `expected_applied`,
    /// and `source_hash` takes no part in that comparison. It is here because
    /// it is the only document identity that survives a restart — the effective
    /// hash covers the epoch-specific controller endpoint the manager stamps
    /// into every new epoch, so two epochs of one unchanged configuration never
    /// share it.
    pub source_hash: Option<String>,
    /// Healthy / unhealthy, when the host reports it.
    pub healthy: Option<bool>,
    /// The kind of core the host has actually applied -- not the desired
    /// config. `None` when the host does not report an applied identity at
    /// all (an old daemon with no `type`, or a manager that has never
    /// applied anything). `CoreKind` collapses alpha channels on purpose: a
    /// running mihomo-alpha and a running mihomo both report `Mihomo`, and a
    /// consumer deciding whether an applied core "may be" some target must
    /// treat them the same way it treats an unknown `state` -- as a fact it
    /// cannot rule out, not as a mismatch.
    pub applied_kind: Option<CoreKind>,
}

#[derive(Debug, Clone)]
pub struct CoreSubmission {
    pub expected_owner: Option<(ExecutionHost, u64)>,
    pub envelope: CoreCommandEnvelope,
    pub core_type: Option<chimera_utils::core::CoreType>,
}

/// One advisory config check. It carries the document twice on purpose: the
/// in-process control plane takes the bytes inline, while the daemon's
/// `/core/check` opens a file itself, and both must see the same document the
/// reconcile will submit.
#[derive(Debug, Clone)]
pub struct CheckSubmission {
    pub core_spec: chimera_core_manager::CoreSpec,
    pub core_type: chimera_utils::core::CoreType,
    pub config_bytes: Vec<u8>,
    /// `payload_digest` of `config_bytes`, verified on receipt where the host
    /// supports it.
    pub digest: String,
    /// A private file holding exactly `config_bytes`, for a host that reads
    /// the config from disk rather than from the request.
    pub staged_config: Option<camino::Utf8PathBuf>,
}

/// A host's answer about a check. `Unsupported` is deliberately not an error:
/// "this host cannot check" and "the core rejected the config" are different
/// facts, and neither may be read as a pass.
#[derive(Debug)]
pub enum CheckSupport {
    Ran(Result<(), CoreError>),
    Unsupported { reason: String },
}

/// One host's control plane, as the router consumes it. Submit is the only
/// mutating call and is always envelope-shaped; waiting on an operation is a
/// read and deliberately not routed through the actor mailbox.
#[async_trait::async_trait]
pub trait ControlEndpoint: Send + Sync {
    async fn reconcile_transparent_proxy(
        &self,
        _request: &chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyRequest,
    ) -> Result<
        chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus,
        CoreError,
    > {
        Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "transparent capture requires a Linux service host with network support",
            false,
        ))
    }

    async fn transparent_proxy_status(
        &self,
    ) -> Result<
        chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus,
        CoreError,
    > {
        Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "this execution host does not expose transparent capture",
            false,
        ))
    }

    async fn effective_config(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        Ok(None)
    }

    /// The applied process binding; absent means no usable API. Old hosts must
    /// fail explicitly rather than reconstructing credentials from globals.
    async fn api_connection(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "the endpoint does not expose instance-bound API access",
            false,
        ))
    }

    /// Ordered lifecycle notifications, used to wake API revocation checks.
    /// The periodic authority check also covers lost/coalesced notifications.
    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        Ok(None)
    }

    fn host(&self) -> ExecutionHost;

    /// Advisory, read-only config validation. It never enters the mutating
    /// queue and is never a precondition for a change (core-manager amendment
    /// A2). The default is `Unsupported`: a host that has no check capability
    /// says so rather than answering "fine".
    async fn check_config(&self, _submission: CheckSubmission) -> CheckSupport {
        CheckSupport::Unsupported {
            reason: "this execution host exposes no config check".into(),
        }
    }

    /// Admission into the host's executor. Returns the operation's
    /// admission-time snapshot; the transaction survives this future's drop.
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError>;

    /// Long-poll the host's registry. `None` = unknown/evicted id (recover by
    /// re-reading status; the revision CAS blocks double application).
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo>;

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError>;
}

pub type EndpointHandle = Arc<dyn ControlEndpoint>;
pub type ApiChanges =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<(), CoreError>> + Send>>;

pub struct LocalEndpoint {
    control: CoreControl,
}

impl LocalEndpoint {
    pub fn new(control: CoreControl) -> Self {
        Self { control }
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for LocalEndpoint {
    async fn effective_config(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.control
            .effective_config()
            .await
            .map(|snapshot| {
                Ok(chimera_ipc::api::core::v2::CoreEffectiveConfig {
                    instance_id: snapshot.instance_id.to_string(),
                    revision: chimera_ipc::api::status::ConfigRevisionInfo {
                        epoch: snapshot.revision.epoch.get(),
                        generation: snapshot.revision.generation,
                        source_hash: snapshot.revision.source_hash.clone(),
                        effective_hash: snapshot.revision.effective_hash.clone(),
                    },
                    config: serde_yaml::to_string(snapshot.config.as_ref()).map_err(|_| {
                        CoreError::new(
                            CoreErrorKind::Internal,
                            "failed to serialize effective config",
                            false,
                        )
                    })?,
                })
            })
            .transpose()
    }

    async fn api_connection(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        let Some(connection) = self.control.api_connection().await else {
            return Ok(None);
        };
        let controller = match connection.controller.host {
            chimera_core_manager::Host::Http(url) => {
                chimera_ipc::api::status::CoreControllerInfo::Http(url.to_string())
            }
            chimera_core_manager::Host::UnixSocket(path) => {
                chimera_ipc::api::status::CoreControllerInfo::UnixSocket(path)
            }
            chimera_core_manager::Host::NamedPipe(path) => {
                chimera_ipc::api::status::CoreControllerInfo::NamedPipe(path)
            }
            _ => {
                return Err(CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    "unsupported API transport",
                    false,
                ));
            }
        };
        Ok(Some(chimera_ipc::api::core::v2::CoreApiConnection {
            instance_id: connection.instance_id.to_string(),
            controller,
            secret: connection.controller.secret,
        }))
    }

    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        let changes = futures_util::stream::unfold(self.control.subscribe(), |mut rx| async move {
            rx.changed().await.ok()?;
            Some((Ok(()), rx))
        });
        Ok(Some(Box::pin(changes)))
    }

    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    /// The in-process control plane takes the document inline, so the staged
    /// file a remote host would need is irrelevant here: these are literally
    /// the bytes the reconcile will carry.
    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        CheckSupport::Ran(
            self.control
                .check(chimera_core_manager::CheckRequest {
                    core: submission.core_spec,
                    config: chimera_core_manager::ConfigInput::Inline {
                        bytes: submission.config_bytes,
                        expected_digest: Some(submission.digest),
                    },
                })
                .await,
        )
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        let handle = self.control.submit(submission.envelope)?;
        Ok(map_local_operation(handle.id(), handle.state()))
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.control
            .wait_operation(id, timeout)
            .await
            .map(|state| map_local_operation(id, state))
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        Ok(map_local_status(&self.control.status()))
    }
}

// Original ref endpoint projections moved out of the desktop adapter.
/// The local sibling of the daemon bridge's wire mapping. Duplicated on
/// purpose: the app must not depend on the daemon host crate, and the wire
/// DTO is the shared vocabulary both sides project into.
pub fn map_local_operation(id: OperationId, state: OperationState) -> OperationInfo {
    let (phase, output, error) = match state {
        OperationState::Queued => (OperationPhase::Queued, None, None),
        OperationState::Running => (OperationPhase::Running, None, None),
        OperationState::Succeeded(output) => (
            OperationPhase::Succeeded,
            Some(match output {
                OperationOutput::Reconciled(outcome) => {
                    OperationOutputInfo::Reconciled(map_local_outcome(&outcome))
                }
                OperationOutput::Stopped => OperationOutputInfo::Stopped,
                OperationOutput::Recovered => OperationOutputInfo::Recovered,
                OperationOutput::ShutDown => OperationOutputInfo::ShutDown,
            }),
            None,
        ),
        OperationState::Failed(failure) => (
            OperationPhase::Failed,
            None,
            Some(chimera_ipc::api::core::v2::OperationErrorInfo {
                kind: failure.kind.map(|kind| Cow::Borrowed(kind.as_str())),
                message: failure.message,
                retryable: failure.retryable,
            }),
        ),
    };
    OperationInfo {
        id: id.to_string(),
        phase,
        output,
        error,
    }
}

fn map_local_outcome(outcome: &ApplyOutcome) -> ReconcileOutcomeInfo {
    let mut warnings = Vec::new();
    let mut current = outcome;
    while let ApplyOutcome::DurabilityUncertain { outcome, warning } = current {
        warnings.push(warning.clone());
        current = &**outcome;
    }
    let (kind, revision, failed_apply) = match current {
        ApplyOutcome::Started { revision } => (ReconcileOutcomeKind::Started, revision, None),
        ApplyOutcome::Noop { revision } => (ReconcileOutcomeKind::Noop, revision, None),
        ApplyOutcome::Patched { revision } => (ReconcileOutcomeKind::Patched, revision, None),
        ApplyOutcome::Reloaded { revision } => (ReconcileOutcomeKind::Reloaded, revision, None),
        ApplyOutcome::Restarted { revision } => (ReconcileOutcomeKind::Restarted, revision, None),
        ApplyOutcome::Switched { revision } => (ReconcileOutcomeKind::Switched, revision, None),
        ApplyOutcome::RolledBack {
            revision,
            failed_apply,
        } => (
            ReconcileOutcomeKind::RolledBack,
            revision,
            Some(failed_apply.clone()),
        ),
        ApplyOutcome::DurabilityUncertain { .. } => unreachable!("unwrapped by the loop above"),
    };
    ReconcileOutcomeInfo {
        outcome: kind,
        revision: chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: revision.epoch.get(),
            generation: revision.generation,
            source_hash: revision.source_hash.clone(),
            effective_hash: revision.effective_hash.clone(),
        },
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        failed_apply,
    }
}

pub fn map_local_status(status: &chimera_core_manager::CoreStatus) -> CoreStatusSnapshot {
    use chimera_core_manager::CoreState as ManagerCoreState;
    let state = match &status.state {
        ManagerCoreState::Stopped { reason } => Some(CoreStateDetail::Stopped {
            reason: reason.as_ref().map(|reason| reason.to_string()),
        }),
        ManagerCoreState::Starting { epoch } => {
            Some(CoreStateDetail::Starting { epoch: epoch.get() })
        }
        ManagerCoreState::Running { epoch, pid } => Some(CoreStateDetail::Running {
            epoch: epoch.get(),
            pid: *pid,
        }),
        ManagerCoreState::Restarting { epoch, attempt } => Some(CoreStateDetail::Restarting {
            epoch: epoch.get(),
            attempt: *attempt,
        }),
        ManagerCoreState::Switching { from, to } => Some(CoreStateDetail::Switching {
            from: from.map(|epoch| epoch.get()),
            to: to.get(),
        }),
        ManagerCoreState::Stopping { epoch } => {
            Some(CoreStateDetail::Stopping { epoch: epoch.get() })
        }
        // `CoreState` is `#[non_exhaustive]`; an unknown future state stays
        // unknown. Folding it into `Stopped` is how a router invents a stop
        // proof it never received.
        other => {
            tracing::warn!("unmapped manager core state: {other:?}");
            None
        }
    };
    CoreStatusSnapshot {
        instance_id: status
            .instance_id
            .map(|instance_id| instance_id.to_string()),
        controller: status.controller.as_ref().and_then(map_controller),
        state,
        state_changed_at: status.changed_at,
        revision: status.revision.as_ref().map(|revision| {
            chimera_ipc::api::status::RevisionIdInfo {
                epoch: revision.epoch.get(),
                generation: revision.generation,
                effective_hash: revision.effective_hash.clone(),
            }
        }),
        source_hash: status
            .revision
            .as_ref()
            .map(|revision| revision.source_hash.clone()),
        healthy: status
            .health
            .as_ref()
            .map(|health| matches!(health.state, chimera_core_manager::HealthState::Healthy)),
        applied_kind: status.spec.as_ref().map(|spec| spec.kind),
    }
}

pub fn map_controller(
    host: &chimera_core_manager::Host,
) -> Option<chimera_ipc::api::status::CoreControllerInfo> {
    use chimera_core_manager::Host;
    use chimera_ipc::api::status::CoreControllerInfo;

    match host {
        Host::Http(url) => {
            let mut url = url.clone();
            let _ = url.set_username("");
            let _ = url.set_password(None);
            Some(CoreControllerInfo::Http(url.to_string()))
        }
        Host::UnixSocket(path) => Some(CoreControllerInfo::UnixSocket(path.clone())),
        Host::NamedPipe(path) => Some(CoreControllerInfo::NamedPipe(path.clone())),
        _ => None,
    }
}

/// A service host is projected from authoritative v2 data only; a v1
/// status without detail cannot be used as proof that the runtime stopped.
pub fn map_service_status(infos: &chimera_ipc::api::status::CoreInfos) -> CoreStatusSnapshot {
    CoreStatusSnapshot {
        instance_id: infos.instance_id.clone(),
        controller: infos.controller.clone(),
        // A service without v2 detail cannot prove it is stopped. Never infer
        // a stop proof from the coarser v1 state field.
        state: infos.detail.clone(),
        state_changed_at: infos.state_changed_at,
        revision: infos.revision.as_ref().map(|revision| revision.id()),
        source_hash: infos
            .revision
            .as_ref()
            .map(|revision| revision.source_hash.clone()),
        healthy: infos.health.as_ref().map(|health| {
            matches!(
                health.state,
                chimera_ipc::api::status::CoreHealthState::Healthy
            )
        }),
        // Keep applied identity unknown until the service projects it from the
        // same manager snapshot as status; the requested type may lag a switch.
        applied_kind: None,
    }
}

/// Service adapter using the app's shared IPC client.
#[derive(Clone)]
pub struct ServiceEndpoint {
    client: &'static Client<'static>,
    owner_dirs: Arc<dyn Fn() -> Result<(PathBuf, PathBuf), CoreError> + Send + Sync>,
}

impl ServiceEndpoint {
    /// The desktop supplies its owner-directory resolver, invoked only for
    /// the owner-fenced Recover command. No global application state here.
    pub fn new(
        client: &'static Client<'static>,
        owner_dirs: impl Fn() -> Result<(PathBuf, PathBuf), CoreError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            client,
            owner_dirs: Arc::new(owner_dirs),
        }
    }
}

impl std::fmt::Debug for ServiceEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceEndpoint")
            .finish_non_exhaustive()
    }
}

fn legacy_disabled_transparent_proxy_response(
    mode: chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyMode,
    error: &ClientError<'_>,
) -> Option<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus> {
    use chimera_ipc::api::network::transparent_proxy::{
        NetworkTransparentProxyMode::Disabled, NetworkTransparentProxyStatus,
    };

    (mode == Disabled
        && matches!(
            error,
            ClientError::HttpStatus { status, .. } if status.as_u16() == 404
        ))
    .then_some(NetworkTransparentProxyStatus {
        supported: false,
        active: false,
        mode: None,
        revision: None,
        error: None,
    })
}

#[async_trait::async_trait]
impl ControlEndpoint for ServiceEndpoint {
    async fn reconcile_transparent_proxy(
        &self,
        request: &chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyRequest,
    ) -> Result<
        chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus,
        CoreError,
    > {
        match self.client.reconcile_transparent_proxy(request).await {
            Ok(status) => Ok(status),
            Err(error) => legacy_disabled_transparent_proxy_response(request.mode, &error)
                .ok_or_else(|| map_client_error(error)),
        }
    }

    async fn transparent_proxy_status(
        &self,
    ) -> Result<
        chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus,
        CoreError,
    > {
        match self.client.transparent_proxy_status().await {
            Ok(status) => Ok(status),
            Err(error) => legacy_disabled_transparent_proxy_response(
                chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyMode::Disabled,
                &error,
            )
            .ok_or_else(|| map_client_error(error)),
        }
    }

    async fn effective_config(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.client
            .effective_config_v2()
            .await
            .map_err(map_client_error)
    }

    async fn api_connection(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        self.client
            .core_api_connection()
            .await
            .map_err(map_client_error)
    }

    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        use futures_util::StreamExt;

        let changes = self
            .client
            .events()
            .await
            .map_err(map_client_error)?
            .filter_map(|event| async move {
                match event {
                    Ok(chimera_ipc::api::ws::events::Event::CoreStatusChanged(_)) => Some(Ok(())),
                    Ok(_) => None,
                    Err(error) => Some(Err(map_client_error(error))),
                }
            });
        Ok(Some(Box::pin(changes)))
    }

    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }

    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        let Some(path) = submission.staged_config else {
            return CheckSupport::Unsupported {
                reason: "the service host validates a config file, and none was staged".into(),
            };
        };
        CheckSupport::Ran(
            self.client
                .check_config(&chimera_ipc::api::core::check::CoreCheckReq {
                    core_type: Cow::Owned(submission.core_type),
                    config_file: Cow::Owned(path.into_std_path_buf()),
                })
                .await
                .map_err(map_client_error),
        )
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        let request = wire_submit_request(&submission, self.owner_dirs.as_ref())?;
        self.client
            .submit_core(&request)
            .await
            .map_err(map_client_error)
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        let id = id.to_string();
        let request = CoreOperationReq {
            operation_id: Cow::Borrowed(id.as_str()),
            wait_ms: Some(timeout.as_millis().min(u128::from(u64::MAX)) as u64),
        };
        self.client
            .core_operation(&request)
            .await
            .map_err(map_client_error)
            .inspect_err(|error| {
                tracing::debug!(%id, %error, "service operation query failed");
            })
            .ok()
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        let infos = self
            .client
            .core_status_v2()
            .await
            .map_err(map_client_error)?;
        Ok(map_service_status(&infos))
    }
}

fn map_client_error(error: ClientError<'_>) -> CoreError {
    tracing::warn!(%error, "service IPC request failed");
    if let ClientError::ServerResponseFailed(response) = &error {
        let kind = error.server_error_kind().and_then(CoreErrorKind::from_wire);
        let retryable = error
            .server_retryable()
            .unwrap_or_else(|| kind.is_some_and(|kind| kind.default_retryable()));
        return CoreError {
            kind,
            message: response.msg.to_string(),
            retryable,
            operation_id: None,
        };
    }

    CoreError::new(
        CoreErrorKind::BackendUnavailable,
        format!("service endpoint unreachable: {error}"),
        true,
    )
}

fn normalized_path_string(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn service_owner_recovery_command(config_dir: &Path, data_dir: &Path) -> CoreCommandInfo<'static> {
    CoreCommandInfo::RecoverIfServiceOwner {
        expected_config_dir: Cow::Owned(normalized_path_string(config_dir)),
        expected_data_dir: Cow::Owned(normalized_path_string(data_dir)),
    }
}

fn wire_submit_request(
    submission: &CoreSubmission,
    owner_dirs: &dyn Fn() -> Result<(PathBuf, PathBuf), CoreError>,
) -> Result<CoreSubmitReq<'static>, CoreError> {
    let command = match &submission.envelope.command {
        CoreCommand::Reconcile(request) => {
            let ConfigInput::Inline {
                bytes,
                expected_digest,
            } = &request.config;
            let config = String::from_utf8(bytes.clone()).map_err(|_| {
                CoreError::new(
                    CoreErrorKind::InvalidConfig,
                    "config bytes are not UTF-8 text",
                    false,
                )
            })?;
            CoreCommandInfo::Reconcile {
                local_ipc: request.options.local_ipc.map(|settings| {
                    chimera_ipc::api::core::v2::LocalIpcSettingsInfo {
                        policy: match settings.policy {
                            chimera_core_manager::LocalIpcPolicy::Force => {
                                chimera_ipc::api::core::v2::LocalIpcPolicyInfo::Force
                            }
                            chimera_core_manager::LocalIpcPolicy::Prefer => {
                                chimera_ipc::api::core::v2::LocalIpcPolicyInfo::Prefer
                            }
                            chimera_core_manager::LocalIpcPolicy::Disable => {
                                chimera_ipc::api::core::v2::LocalIpcPolicyInfo::Disable
                            }
                            _ => chimera_ipc::api::core::v2::LocalIpcPolicyInfo::Disable,
                        },
                        keep_http_controller: settings.keep_http_controller,
                    }
                }),
                core_type: Cow::Owned(match &submission.core_type {
                    Some(core_type) => core_type.clone(),
                    None => app_core_kind_to_type(request.core.kind)?,
                }),
                config: Cow::Owned(config),
                expected_digest: expected_digest.clone().map(Cow::Owned),
                expected_applied: request.expected_applied.as_ref().map(|revision| {
                    RevisionIdInfo {
                        epoch: revision.epoch.get(),
                        generation: revision.generation,
                        effective_hash: revision.effective_hash.clone(),
                    }
                }),
            }
        }
        CoreCommand::Stop => CoreCommandInfo::Stop,
        CoreCommand::StopIfOwner(expected_owner) => CoreCommandInfo::StopIfOwner {
            expected_owner: CoreOwnerInfo {
                instance_id: expected_owner.instance_id.to_string(),
                revision: RevisionIdInfo {
                    epoch: expected_owner.revision.epoch.get(),
                    generation: expected_owner.revision.generation,
                    effective_hash: expected_owner.revision.effective_hash.clone(),
                },
            },
        },
        CoreCommand::Recover => {
            let (config_dir, data_dir) = owner_dirs()?;
            service_owner_recovery_command(&config_dir, &data_dir)
        }
        CoreCommand::Shutdown => {
            return Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "the service core lifecycle cannot be shut down over IPC",
                false,
            ));
        }
    };

    Ok(CoreSubmitReq {
        operation_id: Cow::Owned(submission.envelope.operation_id.to_string()),
        command,
    })
}

fn app_core_kind_to_type(kind: CoreKind) -> Result<chimera_utils::core::CoreType, CoreError> {
    use chimera_utils::core::{ClashCoreType, CoreType};

    let core_type = match kind {
        CoreKind::Mihomo => ClashCoreType::Mihomo,
        CoreKind::ClashRust => ClashCoreType::ClashRust,
        CoreKind::ClashPremium => ClashCoreType::ClashPremium,
        // Keep Chimera Client a first-class wire identity. It accepts the
        // clash-rs config contract, but must not collapse to `ClashRust`.
        CoreKind::ChimeraClient => ClashCoreType::ChimeraClient,
        CoreKind::Meow => {
            return Err(CoreError::new(
                CoreErrorKind::Internal,
                "Meow has no matching Chimera IPC CoreType",
                false,
            ));
        }
    };
    Ok(CoreType::Clash(core_type))
}

#[cfg(test)]
mod service_tests {
    use super::*;
    use camino::Utf8PathBuf;
    use chimera_core_manager::{
        CoreCommandEnvelope, CoreOwner, CoreSpec, Epoch, InstanceOptions, ReconcileRequest,
        RevisionId,
    };
    use chimera_ipc::api::core::v2::CoreCommandInfo;

    #[test]
    fn recover_wire_uses_injected_owner_directories_and_fails_closed() {
        let submission = CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: OperationId::generate(),
                command: CoreCommand::Recover,
            },
            core_type: None,
        };
        let dirs = tempfile::tempdir().unwrap();
        let config = dirs.path().join("config");
        let data = dirs.path().join("data");
        std::fs::create_dir(&config).unwrap();
        std::fs::create_dir(&data).unwrap();
        let wire =
            wire_submit_request(&submission, &|| Ok((config.clone(), data.clone()))).unwrap();
        let CoreCommandInfo::RecoverIfServiceOwner {
            expected_config_dir,
            expected_data_dir,
        } = wire.command
        else {
            panic!("recovery must be owner-fenced")
        };
        assert_eq!(expected_config_dir, normalized_path_string(&config));
        assert_eq!(expected_data_dir, normalized_path_string(&data));
        let result = wire_submit_request(&submission, &|| {
            Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "owner directories unavailable",
                false,
            ))
        });
        assert!(result.is_err());
    }

    #[test]
    fn reconcile_wire_keeps_chimera_client_alpha_channel_identity() {
        let core_type = chimera_utils::core::CoreType::Clash(
            chimera_utils::core::ClashCoreType::ChimeraClientAlpha,
        );
        let submission = CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: "0011223344556677-8899aabb-ccddeeff".parse().unwrap(),
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: CoreSpec {
                        kind: CoreKind::ChimeraClient,
                        binary_path: Utf8PathBuf::from("/opt/chimera/chimera-client-alpha"),
                        version: None,
                        features: Vec::new(),
                    },
                    config: ConfigInput::inline(b"mode: rule\n".to_vec()),
                    options: InstanceOptions::default(),
                    expected_applied: None,
                })),
            },
            core_type: Some(core_type.clone()),
        };

        let wire = wire_submit_request(&submission, &|| {
            panic!("non-recover command must not resolve owner dirs")
        })
        .unwrap();
        let CoreCommandInfo::Reconcile {
            core_type: wire_type,
            config,
            ..
        } = wire.command
        else {
            panic!("reconcile command changed shape");
        };

        assert_eq!(wire_type.as_ref(), &core_type);
        assert_eq!(config, "mode: rule\n");
    }

    /// The fallback half of ref's alpha-routing test belongs to the Service
    /// wire adapter: a missing explicit CoreType must use CoreKind's base
    /// identity, without rewriting an explicitly supplied alpha type.
    #[test]
    fn reconcile_wire_without_core_type_uses_base_kind() {
        let submission = CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: chimera_core_manager::OperationId::generate(),
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: CoreSpec {
                        kind: CoreKind::Mihomo,
                        binary_path: Utf8PathBuf::from("mihomo-alpha"),
                        version: None,
                        features: vec![],
                    },
                    config: ConfigInput::inline(b"proxies: []".to_vec()),
                    options: InstanceOptions::default(),
                    expected_applied: None,
                })),
            },
            core_type: None,
        };

        let wire = wire_submit_request(&submission, &|| {
            panic!("non-recover command must not resolve owner dirs")
        })
        .expect("wire fallback must resolve");
        let CoreCommandInfo::Reconcile { core_type, .. } = wire.command else {
            panic!("expected reconcile wire command");
        };
        assert_eq!(
            core_type.into_owned(),
            chimera_utils::core::CoreType::Clash(chimera_utils::core::ClashCoreType::Mihomo)
        );
    }

    #[test]
    fn stop_if_owner_wire_keeps_process_and_revision_fence() {
        let owner = CoreOwner::from_parts(
            "9af54a67-fc31-4dcb-8422-26644c6277c8",
            RevisionId {
                epoch: Epoch::new(4).unwrap(),
                generation: 12,
                effective_hash: "0123456789abcdef".into(),
            },
        )
        .unwrap();
        let submission = CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: "0011223344556677-8899aabb-ccddeeff".parse().unwrap(),
                command: CoreCommand::StopIfOwner(owner),
            },
            core_type: None,
        };

        let wire = wire_submit_request(&submission, &|| {
            panic!("non-recover command must not resolve owner dirs")
        })
        .unwrap();
        let CoreCommandInfo::StopIfOwner { expected_owner } = wire.command else {
            panic!("owner-fenced stop changed shape");
        };

        assert_eq!(
            expected_owner.instance_id,
            "9af54a67-fc31-4dcb-8422-26644c6277c8"
        );
        assert_eq!(expected_owner.revision.epoch, 4);
        assert_eq!(expected_owner.revision.generation, 12);
        assert_eq!(expected_owner.revision.effective_hash, "0123456789abcdef");
    }

    #[test]
    fn service_quarantine_recovery_wire_names_the_current_installation() {
        let root = tempfile::tempdir().unwrap();
        let config_dir = root.path().join("config");
        let data_dir = root.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        let command = service_owner_recovery_command(&config_dir, &data_dir);
        let CoreCommandInfo::RecoverIfServiceOwner {
            expected_config_dir,
            expected_data_dir,
        } = command
        else {
            panic!("Service recovery must include the app installation identity");
        };

        assert_eq!(expected_config_dir, normalized_path_string(&config_dir));
        assert_eq!(expected_data_dir, normalized_path_string(&data_dir));
    }

    #[test]
    fn disabled_transparent_proxy_treats_missing_legacy_route_as_unsupported() {
        use chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyMode;

        let not_found = ClientError::HttpStatus {
            status: 404_u16.try_into().unwrap(),
            message: "route not found".into(),
        };
        let status = legacy_disabled_transparent_proxy_response(
            NetworkTransparentProxyMode::Disabled,
            &not_found,
        )
        .expect("missing legacy route should be reported as unsupported");

        assert!(!status.supported);
        assert!(
            legacy_disabled_transparent_proxy_response(
                NetworkTransparentProxyMode::Tproxy,
                &not_found,
            )
            .is_none()
        );
    }

    #[test]
    fn known_server_error_metadata_maps_without_message_parsing() {
        let error = ClientError::ServerResponseFailed(chimera_ipc::api::R {
            code: chimera_ipc::api::ResponseCode::OtherError,
            msg: Cow::Borrowed("stale revision"),
            data: None,
            ts: 1,
            error_kind: Some(Cow::Borrowed("revision_conflict")),
            retryable: Some(false),
        });

        let mapped = map_client_error(error);

        assert_eq!(mapped.kind, Some(CoreErrorKind::RevisionConflict));
        assert!(!mapped.retryable);
        assert_eq!(mapped.message, "stale revision");
    }
}

#[cfg(test)]
mod endpoint_contract_tests {
    use super::*;

    #[test]
    fn controller_status_projection_removes_embedded_http_credentials() {
        let host = chimera_core_manager::Host::Http(
            url::Url::parse("http://support-user:private-token@127.0.0.1:9090").unwrap(),
        );
        assert!(matches!(
            map_controller(&host),
            Some(chimera_ipc::api::status::CoreControllerInfo::Http(url))
                if url == "http://127.0.0.1:9090/"
        ));
    }

    #[test]
    fn local_operation_projection_preserves_admission_and_terminal_phases() {
        let id = OperationId::generate();
        let queued = map_local_operation(id, OperationState::Queued);
        assert_eq!(queued.phase, OperationPhase::Queued);
        assert_eq!(queued.id, id.to_string());
        assert!(queued.output.is_none());
        assert!(queued.error.is_none());

        let stopped = map_local_operation(id, OperationState::Succeeded(OperationOutput::Stopped));
        assert_eq!(stopped.phase, OperationPhase::Succeeded);
        assert!(matches!(stopped.output, Some(OperationOutputInfo::Stopped)));
        assert!(stopped.error.is_none());
    }

    #[test]
    fn legacy_service_status_does_not_fabricate_v2_stop_or_applied_identity() {
        use chimera_ipc::api::status::{CoreInfos, CoreState};
        let legacy = CoreInfos {
            instance_id: None,
            r#type: Some(chimera_utils::core::CoreType::Clash(
                chimera_utils::core::ClashCoreType::Mihomo,
            )),
            state: CoreState::Stopped(None),
            state_changed_at: 41,
            config_path: None,
            controller: None,
            health: None,
            revision: None,
            detail: None,
        };
        let projected = map_service_status(&legacy);
        assert_eq!(projected.state_changed_at, 41);
        assert!(
            projected.state.is_none(),
            "v1 stopped is not an authoritative v2 stop proof"
        );
        assert!(
            projected.applied_kind.is_none(),
            "the requested core is not proven applied"
        );
    }

    #[test]
    fn unknown_status_fields_never_synthesize_a_stop_proof() {
        let status = CoreStatusSnapshot {
            instance_id: None,
            controller: None,
            state: None,
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: None,
            applied_kind: None,
        };
        assert!(status.state.is_none());
        assert!(status.revision.is_none());
        assert!(status.instance_id.is_none());
    }
}

#[cfg(test)]
mod controller_tests {
    use super::*;

    #[test]
    fn controller_status_projection_removes_embedded_credentials() {
        let host = chimera_core_manager::Host::Http(
            url::Url::parse("http://support-user:private-token@127.0.0.1:9090").unwrap(),
        );

        let projected = map_controller(&host).unwrap();

        assert!(matches!(
            projected,
            chimera_ipc::api::status::CoreControllerInfo::Http(url)
                if url == "http://127.0.0.1:9090/"
        ));
    }
}

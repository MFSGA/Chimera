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

use std::{borrow::Cow, sync::Arc, time::Duration};

use chimera_core_manager::{
    ApplyOutcome, CoreCommandEnvelope, CoreControl, CoreError, CoreErrorKind, CoreKind,
    OperationId, OperationOutput, OperationState,
};
use chimera_ipc::api::{
    core::v2::{
        OperationInfo, OperationOutputInfo, OperationPhase, ReconcileOutcomeInfo,
        ReconcileOutcomeKind,
    },
    status::CoreStateDetail,
};

/// Which controller owns the runtime. The app perceives the difference in
/// exactly two places: this tag on the endpoint slot, and the handoff
/// protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHost {
    Local,
    Service,
}

/// The host's canonical core status, projected field by field for the app.
/// No field here is ever derived by the router itself.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreStatusSnapshot {
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

// ---------------------------------------------------------------------------
// Local host: the in-process CoreControl.
// ---------------------------------------------------------------------------

pub type ApiChanges = std::pin::Pin<Box<dyn futures::Stream<Item = Result<(), CoreError>> + Send>>;

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
        let changes = futures::stream::unfold(self.control.subscribe(), |mut rx| async move {
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

/// The local sibling of the daemon bridge's wire mapping. Duplicated on
/// purpose: the app must not depend on the daemon host crate, and the wire
/// DTO is the shared vocabulary both sides project into.
fn map_local_operation(id: OperationId, state: OperationState) -> OperationInfo {
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

fn map_local_status(status: &chimera_core_manager::CoreStatus) -> CoreStatusSnapshot {
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

fn map_controller(
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

/// Shared handle shape the actor stores and hands out.
pub type EndpointHandle = Arc<dyn ControlEndpoint>;

#[cfg(test)]
mod tests {
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

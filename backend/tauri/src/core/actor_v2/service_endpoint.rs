//! IPC v2 adapter for a service-owned core.
//!
//! The service process exposes these v2 handlers. The production facade uses
//! this adapter for Service core stop; reconcile and core selection remain on
//! the legacy manager during migration.

use std::{borrow::Cow, time::Duration};

use chimera_core_manager::{
    ConfigInput, CoreCommand, CoreError, CoreErrorKind, CoreKind, OperationId,
};
use chimera_ipc::{
    api::{
        core::v2::{CoreCommandInfo, CoreOperationReq, CoreSubmitReq, OperationInfo},
        status::{CoreInfos, RevisionIdInfo},
    },
    client::{ClientError, shortcuts::Client},
};

use super::control_endpoint::{
    CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
    ExecutionHost,
};

/// Service adapter using the app's shared IPC client.
#[derive(Clone)]
pub struct ServiceEndpoint {
    client: &'static Client<'static>,
}

impl ServiceEndpoint {
    pub fn new(client: &'static Client<'static>) -> Self {
        Self { client }
    }

    pub fn service_default() -> Self {
        Self::new(Client::service_default())
    }
}

impl std::fmt::Debug for ServiceEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceEndpoint")
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for ServiceEndpoint {
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

    async fn api_changes(&self) -> Result<Option<super::control_endpoint::ApiChanges>, CoreError> {
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
        let request = wire_submit_request(&submission)?;
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

fn wire_submit_request(submission: &CoreSubmission) -> Result<CoreSubmitReq<'static>, CoreError> {
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
        CoreCommand::Recover => CoreCommandInfo::Recover,
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

fn map_service_status(infos: &CoreInfos) -> CoreStatusSnapshot {
    CoreStatusSnapshot {
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

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use chimera_core_manager::{CoreCommandEnvelope, CoreSpec, InstanceOptions, ReconcileRequest};
    use chimera_ipc::api::core::v2::CoreCommandInfo;

    #[test]
    fn reconcile_wire_keeps_chimera_client_core_identity() {
        let core_type =
            chimera_utils::core::CoreType::Clash(chimera_utils::core::ClashCoreType::ChimeraClient);
        let submission = CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: "0011223344556677-8899aabb-ccddeeff".parse().unwrap(),
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: CoreSpec {
                        kind: CoreKind::ChimeraClient,
                        binary_path: Utf8PathBuf::from("/opt/chimera/chimera-client"),
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

        let wire = wire_submit_request(&submission).unwrap();
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

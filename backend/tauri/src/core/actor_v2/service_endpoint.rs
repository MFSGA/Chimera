//! IPC v2 adapter for a service-owned core.
//!
//! The service process exposes these v2 handlers. The production facade uses
//! this adapter for Service core stop; reconcile and core selection remain on
//! the legacy manager during migration.

use std::{borrow::Cow, path::Path, time::Duration};

use chimera_core_manager::{
    ConfigInput, CoreCommand, CoreError, CoreErrorKind, CoreKind, OperationId,
};
use chimera_ipc::{
    api::{
        core::v2::{
            CoreCommandInfo, CoreOperationReq, CoreOwnerInfo, CoreSubmitReq, OperationInfo,
        },
        status::RevisionIdInfo,
    },
    client::{ClientError, shortcuts::Client},
};

use super::control_endpoint::{
    CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
    ExecutionHost,
};
use chimera_core::actor_v2::endpoint::map_service_status;

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
            let config_dir = crate::utils::dirs::app_config_dir().map_err(|error| {
                CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    format!("failed to resolve this application's config directory: {error}"),
                    false,
                )
            })?;
            let data_dir = crate::utils::dirs::app_data_dir().map_err(|error| {
                CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    format!("failed to resolve this application's data directory: {error}"),
                    false,
                )
            })?;
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
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use chimera_core_manager::{
        CoreCommandEnvelope, CoreOwner, CoreSpec, Epoch, InstanceOptions, ReconcileRequest,
        RevisionId,
    };
    use chimera_ipc::api::core::v2::CoreCommandInfo;

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

        let wire = wire_submit_request(&submission).unwrap();
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

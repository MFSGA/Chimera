//! Chimera's local CoreControl adapter. The portable status, submission
//! contract, and ControlEndpoint trait are directly upstream-derived and now
//! owned by the independent chimera-core crate. Service-specific transports
//! remain at the Tauri boundary until their IPC protocol is migrated.
use std::time::Duration;

use chimera_core_manager::{CoreControl, CoreError, CoreErrorKind, OperationId};
use chimera_ipc::api::core::v2::OperationInfo;

#[cfg(test)]
use chimera_core::actor_v2::endpoint::map_controller;
pub use chimera_core::actor_v2::endpoint::{
    ApiChanges, CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
    EndpointHandle, ExecutionHost,
};
use chimera_core::actor_v2::endpoint::{map_local_operation, map_local_status};

// ---------------------------------------------------------------------------
// Local host: the in-process CoreControl.
// ---------------------------------------------------------------------------

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

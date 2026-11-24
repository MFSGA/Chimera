use super::endpoint::{
    ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
};
#[cfg(test)]
use axum::Router;
use chimera_core_manager::{CoreError, CoreErrorKind, OperationId};
use chimera_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo, OperationOutputInfo, OperationPhase},
    status::{CoreControllerInfo, CoreStateDetail},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
pub struct Endpoint {
    pub(super) host: ExecutionHost,
    pub binding: watch::Sender<Option<CoreApiConnection>>,
}

#[async_trait::async_trait]
impl ControlEndpoint for Endpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        Ok(self.binding.borrow().clone())
    }

    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        Ok(Some(Box::pin(futures_util::stream::unfold(
            self.binding.subscribe(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((Ok(()), rx))
            },
        ))))
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            controller: None,
            state: Some(if self.binding.borrow().is_some() {
                CoreStateDetail::Running { epoch: 1, pid: 7 }
            } else {
                CoreStateDetail::Stopped { reason: None }
            }),
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: Some(true),
            applied_kind: None,
        })
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        if !matches!(
            submission.envelope.command,
            chimera_core_manager::CoreCommand::Stop
        ) {
            return Err(CoreError::new(
                CoreErrorKind::Internal,
                "test accepts only stop",
                false,
            ));
        }
        self.binding.send_replace(None);
        Ok(OperationInfo {
            id: submission.envelope.operation_id.to_string(),
            phase: OperationPhase::Succeeded,
            output: Some(OperationOutputInfo::Stopped),
            error: None,
        })
    }

    async fn wait_operation(&self, _: OperationId, _: Duration) -> Option<OperationInfo> {
        None
    }
}

pub fn endpoint(url: String) -> Arc<Endpoint> {
    let (binding, _) = watch::channel(Some(CoreApiConnection {
        instance_id: "first-process".into(),
        controller: CoreControllerInfo::Http(url),
        secret: None,
    }));
    Arc::new(Endpoint {
        binding,
        host: ExecutionHost::Local,
    })
}

#[cfg(test)]
pub async fn server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, task)
}

use chimera_core_manager::OperationId;

use crate::{
    actor_v2::{
        api::{ApiClient, ApiError},
        facade::CoreFacade,
    },
    connections::{ConnectionScope, interrupt_connections},
};

#[derive(Default)]
pub struct RuntimeApplyOptions {
    pub interrupt_connections: Option<ConnectionScope>,
}

pub struct RuntimeApplyContext {
    operation_id: OperationId,
    interruption: Option<PreparedInterruption>,
}

struct PreparedInterruption {
    scope: ConnectionScope,
    // None is an authoritative stopped source, not an unavailable API.
    source: Result<Option<ApiClient>, ApiError>,
}

// The new RuntimeReconciler will call these helpers from its one mailbox.
// Chimera's legacy CoreLifecycleWorkflow owns an older port instead of a
// CoreFacade, so pass the same ref facade explicitly until that owner is cut over.
pub async fn prepare_apply(
    core: &CoreFacade,
    operation_id: OperationId,
    options: RuntimeApplyOptions,
) -> RuntimeApplyContext {
    let interruption = match options.interrupt_connections {
        Some(scope) => Some(PreparedInterruption {
            scope,
            source: core.api_client_if_running().await,
        }),
        None => None,
    };
    RuntimeApplyContext {
        operation_id,
        interruption,
    }
}

pub async fn finish_interruption(context: RuntimeApplyContext, replaced: bool) -> Option<ApiError> {
    if !replaced && let Some(interruption) = context.interruption {
        let result = match interruption.source {
            Ok(Some(source)) => interrupt_connections(&source, &interruption.scope).await,
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            tracing::warn!(operation_id = ?context.operation_id, %error,
                    "runtime applied, but source-instance connection interruption failed");
        }
        result.err()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_requested_interruption_does_not_require_an_api_lease() {
        let context = RuntimeApplyContext {
            operation_id: OperationId::generate(),
            interruption: None,
        };
        assert!(finish_interruption(context, false).await.is_none());
    }

    #[tokio::test]
    async fn known_stopped_source_does_not_count_as_an_api_error() {
        let context = RuntimeApplyContext {
            operation_id: OperationId::generate(),
            interruption: Some(PreparedInterruption {
                scope: ConnectionScope::All,
                source: Ok(None),
            }),
        };
        assert!(finish_interruption(context, false).await.is_none());
    }

    #[tokio::test]
    async fn an_unavailable_preapply_source_is_not_silently_ignored() {
        let context = RuntimeApplyContext {
            operation_id: OperationId::generate(),
            interruption: Some(PreparedInterruption {
                scope: ConnectionScope::All,
                source: Err(ApiError::Stale),
            }),
        };
        assert!(matches!(
            finish_interruption(context, false).await,
            Some(ApiError::Stale)
        ));
    }

    #[tokio::test]
    async fn finish_interrupts_only_the_captured_preapply_controller() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let length = socket.read(&mut request).await.unwrap();
            let line = String::from_utf8_lossy(&request[..length])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            socket
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            line
        });
        let source = ApiClient::new_unbound(address.to_string(), None).unwrap();
        let context = RuntimeApplyContext {
            operation_id: OperationId::generate(),
            interruption: Some(PreparedInterruption {
                scope: ConnectionScope::All,
                source: Ok(Some(source)),
            }),
        };
        assert!(finish_interruption(context, false).await.is_none());
        let request = tokio::time::timeout(std::time::Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        assert!(request.starts_with("DELETE /connections "), "{request}");
    }

    #[tokio::test]
    async fn replaced_runtime_does_not_try_to_interrupt_retired_source() {
        let context = RuntimeApplyContext {
            operation_id: OperationId::generate(),
            interruption: Some(PreparedInterruption {
                scope: ConnectionScope::All,
                source: Err(ApiError::Stale),
            }),
        };
        assert!(finish_interruption(context, true).await.is_none());
    }
}

//! Admission and operation-wait adapter for ref's CoreClient router.
//!
//! A submit enters the CoreActor mailbox, which serializes it with handoff.
//! The ticket returned by that mailbox owns the exact endpoint that accepted
//! the operation. A later wait MUST use that endpoint, never reselect the
//! current host; otherwise a handoff can make an admitted operation disappear.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chimera_core_manager::{CoreError, CoreErrorKind, OperationId};
use chimera_ipc::api::core::v2::{OperationInfo, OperationPhase};
use tokio::sync::Mutex;

use super::{
    endpoint::{
        ApiChanges, CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot,
        CoreSubmission, EndpointHandle, ExecutionHost,
    },
    router::{CoreClient, EndpointConnectivity, SubmitFailure},
};

/// One logical host's adapter. Create it per lifecycle operation so admission
/// tickets are held only until the operation settles or the caller drops it.
/// Read-only access uses the router's *currently connected* owner.
pub struct RoutedEndpoint {
    router: CoreClient,
    host: ExecutionHost,
    tickets: Mutex<HashMap<OperationId, EndpointHandle>>,
}

impl RoutedEndpoint {
    pub fn new(router: CoreClient, host: ExecutionHost) -> Self {
        Self {
            router,
            host,
            tickets: Mutex::new(HashMap::new()),
        }
    }

    async fn selected_endpoint(&self) -> Result<EndpointHandle, CoreError> {
        let connected = self.router.connected_endpoint().await?;
        if connected.host() != self.host {
            return Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "the selected Core host changed before the read could begin",
                true,
            ));
        }
        Ok(connected)
    }

    fn map_submit_failure(error: SubmitFailure) -> CoreError {
        match error {
            SubmitFailure::NotSubmitted(error) => error,
            SubmitFailure::Unknown(error) => {
                // The actor may have admitted this exact operation. Callers
                // must retain the original idempotency id; the retry policy
                // remains the one the CoreActor supplies.
                error
            }
        }
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for RoutedEndpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn reconcile_transparent_proxy(
        &self,
        request: &chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyRequest,
    ) -> Result<
        chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus,
        CoreError,
    > {
        self.selected_endpoint()
            .await?
            .reconcile_transparent_proxy(request)
            .await
    }

    async fn transparent_proxy_status(
        &self,
    ) -> Result<
        chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus,
        CoreError,
    > {
        self.selected_endpoint()
            .await?
            .transparent_proxy_status()
            .await
    }

    async fn effective_config(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.selected_endpoint().await?.effective_config().await
    }

    async fn api_connection(
        &self,
    ) -> Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        self.selected_endpoint().await?.api_connection().await
    }

    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        self.selected_endpoint().await?.api_changes().await
    }

    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        match self.selected_endpoint().await {
            Ok(endpoint) => endpoint.check_config(submission).await,
            Err(error) => CheckSupport::Ran(Err(error)),
        }
    }

    async fn submit(&self, mut submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        // Snapshot and mailbox generation both matter. The mailbox checks
        // that neither the host nor the generation changed before admission.
        let projection = self.router.status();
        if projection.host != self.host
            || !matches!(projection.connectivity, EndpointConnectivity::Connected)
        {
            return Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "the requested Core host is not the connected routing owner",
                true,
            ));
        }
        let expected_owner = (self.host, projection.generation);
        if submission
            .expected_owner
            .is_some_and(|provided| provided != expected_owner)
        {
            return Err(CoreError::new(
                CoreErrorKind::RevisionConflict,
                "the caller's Core owner token disagrees with the current router",
                false,
            ));
        }
        submission.expected_owner = Some(expected_owner);

        let ticket = self
            .router
            .submit(submission)
            .await
            .map_err(Self::map_submit_failure)?;
        let result = ticket.admitted;
        if matches!(
            result.phase,
            OperationPhase::Queued | OperationPhase::Running
        ) {
            self.tickets.lock().await.insert(ticket.id, ticket.endpoint);
        }
        Ok(result)
    }

    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        let endpoint = self.tickets.lock().await.get(&id).cloned()?;
        let result = endpoint.wait_operation(id, timeout).await;
        // A missing result is not a proof that the operation did not run.
        // Keep the ticket until the caller drops this scoped adapter.
        if result.as_ref().is_some_and(|info| {
            info.id == id.to_string()
                && matches!(
                    info.phase,
                    OperationPhase::Succeeded | OperationPhase::Failed
                )
        }) {
            self.tickets.lock().await.remove(&id);
        }
        result
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        let projection = self.router.refresh_status().await?;
        if projection.host != self.host
            || !matches!(projection.connectivity, EndpointConnectivity::Connected)
        {
            return Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "the selected Core host is no longer connected",
                true,
            ));
        }
        projection.snapshot.ok_or_else(|| {
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "the Core router has no authoritative status snapshot",
                true,
            )
        })
    }
}

#[cfg(test)]
#[path = "routed_endpoint_tests.rs"]
mod tests;

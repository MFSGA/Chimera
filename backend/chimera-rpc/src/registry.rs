//! Transport-neutral caller values and audited execution kernel.
//! The command registry and application dependencies follow the shared client migration.

use crate::audit::{
    AuditInvocation, AuditPhase, CompletionSummary, RpcAudit, RpcAuditGuard, RpcAuditOutcome,
    RpcTransport,
};
use chimera_rpc_protocol::{AuthContext, Principal, RpcError, RpcOwner};
use serde_json::Value;
use std::{future::Future, pin::Pin};

/// A borrowed dependency in a shared declaration, omitted from its wire schema.
pub struct RpcState<'a, T: ?Sized>(&'a T);

impl<T: ?Sized> std::ops::Deref for RpcState<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.0
    }
}

impl<T: ?Sized> specta::function::FunctionArg for RpcState<'_, T> {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

/// Transport-resolved caller identity, scopes, and owner label omitted from wire schemas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcCaller {
    auth: AuthContext,
    owner: RpcOwner,
}

impl RpcCaller {
    /// Construct a caller combining resolved authentication context and owner label.
    #[must_use]
    pub fn new(auth: AuthContext, owner: RpcOwner) -> Self {
        Self { auth, owner }
    }

    /// Construct a full-scope desktop window caller for in-process GUI dispatches.
    #[must_use]
    pub fn desktop(window: impl Into<String>) -> Self {
        let window = window.into();
        let owner = RpcOwner::desktop(&window);
        Self {
            auth: AuthContext::desktop(window),
            owner,
        }
    }

    /// Resolved principal and granted scopes for policy admission and `rpc_describe`.
    #[must_use]
    pub fn auth(&self) -> &AuthContext {
        &self.auth
    }

    /// Keyed resource owner label for log/session ownership correlation.
    #[must_use]
    pub fn owner(&self) -> &RpcOwner {
        &self.owner
    }

    /// Preserve the caller label supplied by the transport without reconstructing it.
    #[must_use]
    pub fn into_owner(self) -> RpcOwner {
        self.owner
    }
}

impl specta::function::FunctionArg for RpcCaller {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

/// Observe dispatch without changing its result, polling, cancellation or unwind.
/// The name resolver returns only registry or fixed names. Preparation errors are
/// rejected before admission; prepared futures return their own completion metadata.
/// Aborting the process cannot produce a Drop record.
pub async fn dispatch_audited<T, F>(
    audit: &RpcAudit,
    transport: RpcTransport,
    owner: RpcOwner,
    principal: Option<&Principal>,
    command: impl FnOnce() -> &'static str,
    request_id: Option<chimera_rpc_protocol::frame::RequestId>,
    prepare: impl FnOnce() -> Result<F, RpcError>,
) -> Result<T, RpcError>
where
    F: Future<Output = RpcCompletion<T>>,
{
    let invocation = audit.invocation(transport, &owner, principal, command(), request_id);
    dispatch_with_audit(&invocation, prepare).await
}

/// Use adapter-supplied correlation without giving the invocation transport ownership.
pub(crate) async fn dispatch_with_audit<T, F>(
    invocation: &AuditInvocation,
    prepare: impl FnOnce() -> Result<F, RpcError>,
) -> Result<T, RpcError>
where
    F: Future<Output = RpcCompletion<T>>,
{
    let mut guard = RpcAuditGuard::new(invocation);
    let call = match prepare() {
        Ok(call) => call,
        Err(error) => {
            guard.record(AuditPhase::Rejected {
                kind: error.kind,
                code: error.code.clone(),
            });
            return Err(error);
        }
    };
    guard.admit();
    let RpcCompletion { result, completion } = call.await;
    let outcome = match &result {
        Ok(_) => RpcAuditOutcome::Success,
        Err(error) => RpcAuditOutcome::Failure {
            kind: error.kind,
            code: error.code.clone(),
        },
    };
    guard.record(AuditPhase::Finished {
        outcome,
        completion,
    });
    result
}

/// A handler result with metadata extracted before response encoding, even on encode failure.
pub struct RpcCompletion<T> {
    pub result: Result<T, RpcError>,
    pub completion: CompletionSummary,
}

impl<T> RpcCompletion<T> {
    /// Wrap a result whose declaration does not explicitly report mutation receipts.
    #[must_use]
    pub fn unreported(result: Result<T, RpcError>) -> Self {
        Self {
            result,
            completion: CompletionSummary::NotReported,
        }
    }
}

/// Adapt a direct route without reporting completion metadata or changing its polling.
pub async fn unreported<T>(call: impl Future<Output = Result<T, RpcError>>) -> RpcCompletion<T> {
    RpcCompletion::unreported(call.await)
}

/// A prepared handler, polled inline without cancelling owner-started work on drop.
pub type RpcFuture = Pin<Box<dyn Future<Output = RpcCompletion<Value>> + Send + 'static>>;

#[cfg(test)]
mod tests;

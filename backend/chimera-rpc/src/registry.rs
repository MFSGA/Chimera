//! Shared command registry and audited execution kernel.

use crate::audit::{
    AuditInvocation, AuditPhase, CompletionSummary, RpcAudit, RpcAuditGuard, RpcAuditOutcome,
    RpcTransport,
};
use crate::permission::Authority;
use chimera_core::client::ChimeraClient;
use chimera_rpc_protocol::{
    AuthContext, CommandKind, Principal, ResponseVisibility, RpcDescribeResponse, RpcError,
    RpcOwner, RpcTarget, ServerInfo, ShellCapability,
    permission::{RESOURCES, ResourceMetadata},
};
use serde_json::Value;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

/// Only already-composed, real host dependencies are installed.
pub struct RpcDependencies {
    pub client: ChimeraClient,
    pub audit: RpcAudit,
}

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

const RPC_DESCRIBE_PROTOCOL_VERSION: u32 = chimera_rpc_protocol::frame::PROTOCOL_VERSION;

/// Host and caller snapshot injected into [`crate::commands::meta::rpc_describe`].
pub struct RpcDescribeContext {
    response: RpcDescribeResponse,
}

impl RpcDescribeContext {
    /// Consume the injected snapshot to return the canonical `rpc_describe` response.
    #[must_use]
    pub fn into_response(self) -> RpcDescribeResponse {
        self.response
    }
}

impl specta::function::FunctionArg for RpcDescribeContext {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

/// Dispatch metadata injected into neutral command wrappers.
#[derive(Clone)]
pub struct RpcCallMeta {
    server: ServerInfo,
    target: RpcTarget,
    commands: CommandHandlers,
    provided: Vec<ShellCapability>,
    /// The row admission admitted this call to: the seam the layers inside admission read.
    resource: ResourceMetadata,
    /// What the redaction layer decided the caller sees of the result and errors.
    visibility: ResponseVisibility,
}

impl RpcCallMeta {
    /// The resource table row this call was admitted to.
    #[must_use]
    pub fn resource(&self) -> &ResourceMetadata {
        &self.resource
    }

    /// How the result and errors of this call are encoded for the caller.
    #[must_use]
    pub fn visibility(&self) -> ResponseVisibility {
        self.visibility
    }

    /// Build a caller-scoped self-description snapshot for `rpc_describe`.
    #[must_use]
    pub fn describe_context(&self, auth: &AuthContext) -> RpcDescribeContext {
        RpcDescribeContext {
            response: RpcDescribeResponse {
                protocol: RPC_DESCRIBE_PROTOCOL_VERSION,
                server: self.server.clone(),
                transport: self.target,
                principal: auth.principal.clone(),
                role: auth.role,
                permissions: auth.permissions,
                commands: crate::permission::describe_commands(
                    &self.commands,
                    Authority::new(&self.provided),
                    self.target,
                    auth,
                ),
            },
        }
    }
}

/// The refusal of a method the registry does not know. It has no resource whose `write` would lift
/// redaction, so the name the caller sent is never echoed back.
fn unknown_method(method: &str) -> RpcError {
    RpcError::unknown_method_for(ResponseVisibility::Public, method)
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

/// Decode and bind inputs before returning the future that executes the handler.
pub type RpcHandler =
    fn(Arc<RpcDependencies>, RpcCallMeta, RpcCaller, Value) -> Result<RpcFuture, RpcError>;
/// One implementation linked by the declaration macro.
pub struct CommandEntry {
    pub name: &'static str,
    pub kind: CommandKind,
    /// Whether the neutral handler runs the implementation, as HTTP and local exposure require.
    pub transport_neutral: bool,
    pub handler: RpcHandler,
}

inventory::collect!(CommandEntry);

#[derive(Clone)]
pub(crate) struct RegisteredCommand {
    metadata: ResourceMetadata,
    native_linked: bool,
    handler: Option<RpcHandler>,
}

impl RegisteredCommand {
    pub(crate) fn metadata(&self) -> &ResourceMetadata {
        &self.metadata
    }

    /// Whether an implementation that serves `target` is linked: the native wrapper for a native
    /// command on the desktop, the neutral handler otherwise.
    pub(crate) fn linked(&self, target: RpcTarget) -> bool {
        if target == RpcTarget::Desktop && self.metadata.is_native_command() {
            self.native_linked
        } else {
            self.handler.is_some()
        }
    }

    /// The neutral handler serving `target`: none where the command is not exposed, or where
    /// only the native wrapper serves it. Capabilities and the requirement are admission's.
    fn neutral_handler(&self, target: RpcTarget) -> Result<RpcHandler, RpcError> {
        if !self.metadata.targets.contains(&target)
            || (target == RpcTarget::Desktop && self.metadata.is_native_command())
        {
            return Err(crate::permission::unavailable(&self.metadata, target));
        }
        self.handler
            .ok_or_else(|| crate::permission::unavailable(&self.metadata, target))
    }
}

pub(crate) type CommandHandlers = Arc<HashMap<&'static str, RegisteredCommand>>;

pub(crate) fn register_commands<'a>(
    resources: impl IntoIterator<Item = &'a ResourceMetadata>,
    entries: impl IntoIterator<Item = &'static CommandEntry>,
) -> Result<CommandHandlers, SetupError> {
    let mut commands = HashMap::new();
    for metadata in resources
        .into_iter()
        .filter(|resource| resource.is_command())
    {
        if commands
            .insert(
                metadata.name,
                RegisteredCommand {
                    metadata: *metadata,
                    native_linked: false,
                    handler: None,
                },
            )
            .is_some()
        {
            return Err(SetupError::RegisterCommand {
                name: metadata.name,
            });
        }
    }
    for entry in entries {
        // A handler without a table row has no authorization metadata, so it is never served.
        let command = commands
            .get_mut(entry.name)
            .ok_or(SetupError::UndeclaredCommand { name: entry.name })?;
        if command.metadata.kind
            != chimera_rpc_protocol::permission::ResourceKind::Command(entry.kind)
            || (!entry.transport_neutral && !command.metadata.is_native_command())
        {
            return Err(SetupError::DeclarationMismatch { name: entry.name });
        }
        if command.handler.replace(entry.handler).is_some() {
            return Err(SetupError::RegisterCommand { name: entry.name });
        }
    }
    Ok(Arc::new(commands))
}

#[derive(Clone)]
pub struct UnifiedRpc {
    pub(crate) dependencies: Arc<RpcDependencies>,
    pub(crate) server: ServerInfo,
    pub(crate) provided: Vec<ShellCapability>,
    pub(crate) commands: CommandHandlers,
}

impl UnifiedRpc {
    /// Shares the explicitly assembled inputs with special transport adapters.
    pub fn dependencies(&self) -> Arc<RpcDependencies> {
        self.dependencies.clone()
    }

    pub fn new(
        dependencies: RpcDependencies,
        server: ServerInfo,
        provided: &[ShellCapability],
    ) -> Result<Self, SetupError> {
        let commands = register_commands(RESOURCES, inventory::iter::<CommandEntry>)?;
        Ok(Self {
            dependencies: Arc::new(dependencies),
            server,
            provided: provided.to_vec(),
            commands,
        })
    }

    pub fn command_names(&self) -> Vec<&'static str> {
        let mut names = self.commands.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    /// Return the canonical discovery snapshot for `target` and resolved `auth`.
    #[must_use]
    pub fn describe(&self, target: RpcTarget, auth: &AuthContext) -> RpcDescribeResponse {
        RpcDescribeResponse {
            protocol: RPC_DESCRIBE_PROTOCOL_VERSION,
            server: self.server.clone(),
            transport: target,
            principal: auth.principal.clone(),
            role: auth.role,
            permissions: auth.permissions,
            commands: crate::permission::describe_commands(
                &self.commands,
                self.authority(),
                target,
                auth,
            ),
        }
    }

    /// Admission for the capabilities this host provides.
    pub(crate) fn authority(&self) -> Authority<'_> {
        Authority::new(&self.provided)
    }

    /// Dispatch a shared command with an already-resolved caller context.
    pub async fn call(
        &self,
        transport: RpcTransport,
        method: &str,
        caller: RpcCaller,
        params: Value,
    ) -> Result<Value, RpcError> {
        let invocation = self.audit_invocation(
            transport,
            method,
            caller.owner(),
            Some(&caller.auth().principal),
            None,
        );
        self.call_with_admission(&invocation, transport, method, Some(caller), params, || {
            Ok(())
        })
        .await
    }

    pub(crate) fn admit_command(
        &self,
        transport: RpcTransport,
        method: &str,
        auth: Option<&AuthContext>,
    ) -> Result<(), RpcError> {
        let target = match transport {
            RpcTransport::TauriIpc | RpcTransport::TauriProtocol => RpcTarget::Desktop,
            RpcTransport::Http => RpcTarget::Http,
            RpcTransport::Local => RpcTarget::Local,
        };
        if auth.is_none() {
            return Err(RpcError::unauthorized("authentication required"));
        }
        let command = self
            .commands
            .get(method)
            .ok_or_else(|| unknown_method(method))?;
        command.neutral_handler(target)?;
        self.authority().admit(
            &command.metadata,
            target,
            auth.map(|auth| &auth.permissions),
        )
    }

    pub(crate) async fn call_with_admission(
        &self,
        invocation: &AuditInvocation,
        transport: RpcTransport,
        method: &str,
        caller: Option<RpcCaller>,
        params: Value,
        admit_resource: impl FnOnce() -> Result<(), RpcError>,
    ) -> Result<Value, RpcError> {
        let target = match transport {
            RpcTransport::TauriIpc | RpcTransport::TauriProtocol => RpcTarget::Desktop,
            RpcTransport::Http => RpcTarget::Http,
            RpcTransport::Local => RpcTarget::Local,
        };
        dispatch_with_audit(invocation, || {
            let caller = caller.ok_or_else(|| RpcError::unauthorized("authentication required"))?;
            let command = self
                .commands
                .get(method)
                .ok_or_else(|| unknown_method(method))?;
            let handler = command.neutral_handler(target)?;
            self.authority()
                .admit(&command.metadata, target, Some(&caller.auth().permissions))?;
            admit_resource()?;
            // Data sharing (TODO, permission namespace spec section 4.6) sits here, reading the
            // admitted row from the call metadata.
            let meta = RpcCallMeta {
                server: self.server.clone(),
                target,
                commands: self.commands.clone(),
                provided: self.provided.clone(),
                resource: command.metadata,
                visibility: crate::redaction::visibility(
                    &command.metadata,
                    &caller.auth().permissions,
                ),
            };
            handler(self.dependencies.clone(), meta, caller, params)
        })
        .await
    }

    fn audit_command_name(&self, method: &str) -> &'static str {
        self.commands
            .get_key_value(method)
            .map_or("unknown_method", |(name, _)| *name)
    }

    pub(crate) fn audit_invocation(
        &self,
        transport: RpcTransport,
        method: &str,
        owner: &RpcOwner,
        principal: Option<&Principal>,
        request_id: Option<chimera_rpc_protocol::frame::RequestId>,
    ) -> AuditInvocation {
        self.dependencies.audit.invocation(
            transport,
            owner,
            principal,
            self.audit_command_name(method),
            request_id,
        )
    }
}

/// Registration errors: no fallback that could bypass admission.
#[derive(Debug, snafu::Snafu)]
pub enum SetupError {
    #[snafu(display("duplicate RPC command: {name}"))]
    RegisterCommand { name: &'static str },
    #[snafu(display("RPC command {name} is linked but has no resource table row"))]
    UndeclaredCommand { name: &'static str },
    #[snafu(display("RPC command {name} does not match its resource declaration"))]
    DeclarationMismatch { name: &'static str },
}

#[cfg(test)]
mod tests;

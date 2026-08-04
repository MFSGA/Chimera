use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::to_bytes,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use serde_json::Value;
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{Mutex, Semaphore, oneshot},
    task::JoinHandle,
    time::timeout,
};

#[cfg(test)]
use super::super::ports::AgentToolExecutionFuture as ToolExecutionFuture;
use super::super::{
    AgentCommandError, AgentManifest, agent_manifest,
    bridge::{AgentBridgeStartResult, AgentBridgeStatus},
    ports::{AgentBridgeHealthPort, AgentBridgePort, AgentToolExecutorPort as ToolExecutor},
    registry::{AgentToolError, AgentToolErrorCode, tool_timeout, validate_tool_request},
};

const BRIDGE_BIND_ADDRESS: &str = "127.0.0.1:0";
const BRIDGE_SCHEMA_VERSION: u16 = 1;
const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024;
const MAX_CONCURRENT_TOOL_CALLS: usize = 4;
const MAX_TOOL_CALLS_PER_WINDOW: u32 = 30;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const REQUEST_ID_HEADER: &str = "x-request-id";
const BRIDGE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    schema_version: u16,
}

#[derive(Debug, Serialize)]
struct ToolSuccessResponse {
    request_id: String,
    tool: String,
    output: Value,
}

#[derive(Debug, Serialize)]
struct ToolErrorResponse {
    request_id: String,
    error: ToolErrorBody,
}

#[derive(Debug, Serialize)]
struct ToolErrorBody {
    code: &'static str,
    message: &'static str,
}

#[derive(Clone)]
struct HttpState {
    executor: Option<Arc<dyn ToolExecutor>>,
    token: Arc<str>,
    execution: Arc<Semaphore>,
    rate_limit: Arc<Mutex<RateLimitState>>,
}

struct RateLimitState {
    window_started: Instant,
    calls: u32,
}

impl RateLimitState {
    fn new() -> Self {
        Self {
            window_started: Instant::now(),
            calls: 0,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_started) >= RATE_LIMIT_WINDOW {
            self.window_started = now;
            self.calls = 0;
        }
        if self.calls >= MAX_TOOL_CALLS_PER_WINDOW {
            return false;
        }
        self.calls += 1;
        true
    }
}

#[derive(Clone, Copy)]
struct BridgeEndpoint(SocketAddr);

impl BridgeEndpoint {
    fn new(address: SocketAddr) -> Result<Self, AgentCommandError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(AgentCommandError::BridgeStartFailed);
        }
        Ok(Self(address))
    }

    fn base_url(self) -> String {
        format!("http://{}", self.0)
    }

    fn health_url(self) -> String {
        format!("{}/health", self.base_url())
    }
}

struct BridgeRuntime {
    endpoint: BridgeEndpoint,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

pub(crate) struct HttpAgentBridge {
    runtime: Option<BridgeRuntime>,
    executor: Arc<dyn ToolExecutor>,
    health: Arc<dyn AgentBridgeHealthPort>,
}

impl HttpAgentBridge {
    pub(crate) fn new(
        executor: Arc<dyn ToolExecutor>,
        health: Arc<dyn AgentBridgeHealthPort>,
    ) -> Self {
        Self {
            runtime: None,
            executor,
            health,
        }
    }
}

#[async_trait::async_trait]
impl AgentBridgePort for HttpAgentBridge {
    async fn start(&mut self) -> Result<AgentBridgeStartResult, AgentCommandError> {
        reconcile_runtime(&mut self.runtime, self.health.as_ref()).await;
        if let Some(current) = self.runtime.as_ref() {
            return Ok(AgentBridgeStartResult::already_running(
                current.endpoint.base_url(),
            ));
        }

        let listener = TcpListener::bind(BRIDGE_BIND_ADDRESS)
            .await
            .map_err(|_| AgentCommandError::BridgeStartFailed)?;
        let address = listener
            .local_addr()
            .map_err(|_| AgentCommandError::BridgeStartFailed)?;
        let endpoint = BridgeEndpoint::new(address)?;
        let base_url = endpoint.base_url();
        let token = hex::encode(rand::random::<[u8; 32]>());
        let router = build_router(HttpState {
            executor: Some(self.executor.clone()),
            token: Arc::from(token.as_str()),
            execution: Arc::new(Semaphore::new(MAX_CONCURRENT_TOOL_CALLS)),
            rate_limit: Arc::new(Mutex::new(RateLimitState::new())),
        });
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
            if result.is_err() {
                tracing::error!(target: "agent_bridge", "agent bridge server stopped unexpectedly");
            }
        });

        self.runtime = Some(BridgeRuntime {
            endpoint,
            shutdown,
            task,
        });

        tracing::info!(target: "agent_bridge", "agent bridge started");
        Ok(AgentBridgeStartResult::started(base_url, token))
    }

    async fn status(&mut self) -> AgentBridgeStatus {
        reconcile_runtime(&mut self.runtime, self.health.as_ref()).await;
        AgentBridgeStatus {
            running: self.runtime.is_some(),
            base_url: self.runtime.as_ref().map(|value| value.endpoint.base_url()),
        }
    }

    async fn stop(&mut self) -> AgentBridgeStatus {
        remove_finished_runtime(&mut self.runtime);
        let Some(runtime) = self.runtime.take() else {
            return stopped_status();
        };

        shutdown_runtime(runtime).await;
        tracing::info!(target: "agent_bridge", "agent bridge stopped");
        stopped_status()
    }
}

async fn shutdown_runtime(runtime: BridgeRuntime) {
    let BridgeRuntime {
        shutdown, mut task, ..
    } = runtime;
    let _ = shutdown.send(());
    match timeout(BRIDGE_SHUTDOWN_TIMEOUT, &mut task).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            tracing::warn!(target: "agent_bridge", "agent bridge task join failed");
        }
        Err(_) => {
            tracing::warn!(target: "agent_bridge", "agent bridge shutdown timed out; aborting task");
            task.abort();
            let _ = task.await;
        }
    }
}

async fn reconcile_runtime(
    runtime: &mut Option<BridgeRuntime>,
    health: &dyn AgentBridgeHealthPort,
) {
    if runtime
        .as_ref()
        .is_some_and(|current| current.task.is_finished())
    {
        tracing::warn!(target: "agent_bridge", "clearing finished agent bridge runtime");
        runtime.take();
        return;
    }

    let Some(endpoint) = runtime.as_ref().map(|current| current.endpoint) else {
        return;
    };
    if bridge_is_healthy(health, endpoint).await {
        return;
    }

    tracing::warn!(target: "agent_bridge", "clearing unresponsive agent bridge runtime");
    if let Some(stale) = runtime.take() {
        let _ = stale.shutdown.send(());
        stale.task.abort();
        let _ = stale.task.await;
    }
}

fn remove_finished_runtime(runtime: &mut Option<BridgeRuntime>) {
    if runtime
        .as_ref()
        .is_some_and(|current| current.task.is_finished())
    {
        tracing::warn!(target: "agent_bridge", "clearing finished agent bridge runtime");
        runtime.take();
    }
}

async fn bridge_is_healthy(health: &dyn AgentBridgeHealthPort, endpoint: BridgeEndpoint) -> bool {
    health
        .is_healthy(&endpoint.health_url(), BRIDGE_SCHEMA_VERSION)
        .await
}

fn stopped_status() -> AgentBridgeStatus {
    AgentBridgeStatus {
        running: false,
        base_url: None,
    }
}

fn build_router(state: HttpState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/agent/manifest", get(manifest))
        .route("/agent/tools/{tool_name}", post(call_tool))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        schema_version: BRIDGE_SCHEMA_VERSION,
    })
}

async fn manifest() -> Json<AgentManifest> {
    Json(agent_manifest())
}

async fn call_tool(
    Path(tool_name): Path<String>,
    State(state): State<HttpState>,
    request: Request,
) -> Response {
    let request_id = new_request_id();
    if !is_authorized(request.headers(), &state.token) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            request_id,
            "unauthorized",
            "a valid bearer token is required",
        );
    }

    if !state.rate_limit.lock().await.allow(Instant::now()) {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            request_id,
            "rate_limited",
            "agent bridge request rate limit exceeded",
        );
    }

    let Some(tool_timeout) = tool_timeout(&tool_name) else {
        return error_response(
            StatusCode::NOT_FOUND,
            request_id,
            "unknown_tool",
            "unknown agent tool",
        );
    };
    let body = match to_bytes(request.into_body(), MAX_REQUEST_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                request_id,
                "request_too_large",
                "agent tool request body exceeds the allowed size",
            );
        }
    };
    if let Err(error) = validate_tool_request(&tool_name, &body) {
        return registry_error_response(request_id, error);
    }

    let _permit = match state.execution.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                request_id,
                "bridge_busy",
                "too many agent tool calls are running",
            );
        }
    };

    let Some(executor) = state.executor.clone() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            request_id,
            "bridge_unavailable",
            "agent bridge application state is unavailable",
        );
    };
    let result = timeout(tool_timeout, executor.execute(tool_name.clone(), body)).await;
    match result {
        Ok(Ok(output)) => success_response(request_id, tool_name, output),
        Ok(Err(error)) => registry_error_response(request_id, error),
        Err(_) => error_response(
            StatusCode::GATEWAY_TIMEOUT,
            request_id,
            "tool_timeout",
            "agent tool execution timed out",
        ),
    }
}

fn success_response(request_id: String, tool: String, output: Value) -> Response {
    response_with_request_id(
        StatusCode::OK,
        &request_id,
        ToolSuccessResponse {
            request_id: request_id.clone(),
            tool,
            output,
        },
    )
}

fn registry_error_response(request_id: String, error: AgentToolError) -> Response {
    let status = match error.code {
        AgentToolErrorCode::UnknownTool => StatusCode::NOT_FOUND,
        AgentToolErrorCode::InvalidRequest | AgentToolErrorCode::InvalidTarget => {
            StatusCode::BAD_REQUEST
        }
        AgentToolErrorCode::TargetBlocked => StatusCode::FORBIDDEN,
        AgentToolErrorCode::ResolutionFailed | AgentToolErrorCode::ExecutionFailed => {
            StatusCode::BAD_GATEWAY
        }
        AgentToolErrorCode::TimedOut => StatusCode::GATEWAY_TIMEOUT,
    };
    error_response(status, request_id, error.code.as_str(), error.message)
}

fn error_response(
    status: StatusCode,
    request_id: String,
    code: &'static str,
    message: &'static str,
) -> Response {
    response_with_request_id(
        status,
        &request_id,
        ToolErrorResponse {
            request_id: request_id.clone(),
            error: ToolErrorBody { code, message },
        },
    )
}

fn response_with_request_id<T: Serialize>(
    status: StatusCode,
    request_id: &str,
    payload: T,
) -> Response {
    let mut response = (status, Json(payload)).into_response();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

fn new_request_id() -> String {
    hex::encode(rand::random::<[u8; 12]>())
}

fn is_authorized(headers: &HeaderMap, token: &str) -> bool {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    value
        .strip_prefix("Bearer ")
        .is_some_and(|candidate| bool::from(candidate.as_bytes().ct_eq(token.as_bytes())))
}

#[cfg(test)]
#[path = "../bridge_tests.rs"]
mod tests;

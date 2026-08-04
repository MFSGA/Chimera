use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

use axum::{
    Router,
    http::{HeaderMap, HeaderValue, header::AUTHORIZATION},
    routing::get,
};
use serde_json::Value;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{Semaphore, oneshot},
    time::timeout,
};

use super::{
    AgentBridgeStartResult, AgentToolError, BRIDGE_BIND_ADDRESS, BRIDGE_SHUTDOWN_TIMEOUT,
    BridgeEndpoint, BridgeRuntime, HttpState, MAX_CONCURRENT_TOOL_CALLS, MAX_REQUEST_BODY_BYTES,
    MAX_TOOL_CALLS_PER_WINDOW, RATE_LIMIT_WINDOW, RateLimitState, ToolExecutionFuture,
    ToolExecutor, bridge_is_healthy, build_router, health, is_authorized, manifest, new_request_id,
    reconcile_runtime, shutdown_runtime,
};
use crate::features::agent::HttpBridgeHealth;

const MAX_HEALTH_RESPONSE_BYTES: usize = 256;

fn test_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build test HTTP client")
}

struct ImmediateExecutor {
    calls: AtomicUsize,
}

impl ToolExecutor for ImmediateExecutor {
    fn execute(&self, _tool_name: String, _body: axum::body::Bytes) -> ToolExecutionFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(serde_json::json!({ "completed": true })) })
    }
}

struct GatedExecutor {
    calls: AtomicUsize,
    release: Arc<Semaphore>,
}

impl ToolExecutor for GatedExecutor {
    fn execute(&self, _tool_name: String, _body: axum::body::Bytes) -> ToolExecutionFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let release = self.release.clone();
        Box::pin(async move {
            let _permit = release
                .acquire_owned()
                .await
                .expect("execution release semaphore");
            Ok(serde_json::json!({ "completed": true }))
        })
    }
}

struct PendingExecutor {
    calls: AtomicUsize,
    started: Mutex<Option<oneshot::Sender<()>>>,
    cancelled: Mutex<Option<oneshot::Sender<()>>>,
}

impl ToolExecutor for PendingExecutor {
    fn execute(&self, _tool_name: String, _body: axum::body::Bytes) -> ToolExecutionFuture {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            return Box::pin(async { Ok(serde_json::json!({ "recovered": true })) });
        }
        if let Some(started) = self.started.lock().expect("lock start signal").take() {
            let _ = started.send(());
        }
        let cancelled = self
            .cancelled
            .lock()
            .expect("lock cancellation signal")
            .take();
        Box::pin(PendingExecution { cancelled })
    }
}

struct PendingExecution {
    cancelled: Option<oneshot::Sender<()>>,
}

impl Future for PendingExecution {
    type Output = Result<Value, AgentToolError>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingExecution {
    fn drop(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            let _ = cancelled.send(());
        }
    }
}

async fn start_test_bridge_with_limits(
    executor: Option<Arc<dyn ToolExecutor>>,
    permits: usize,
    rate_limit: RateLimitState,
) -> (
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
    Arc<Semaphore>,
) {
    let listener = TcpListener::bind(BRIDGE_BIND_ADDRESS)
        .await
        .expect("bind test bridge");
    let address = listener.local_addr().expect("read test bridge address");
    let execution = Arc::new(Semaphore::new(permits));
    let router = build_router(HttpState {
        executor,
        token: Arc::from("expected"),
        execution: execution.clone(),
        rate_limit: Arc::new(tokio::sync::Mutex::new(rate_limit)),
    });
    let (shutdown, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve test bridge");
    });
    (format!("http://{address}"), shutdown, task, execution)
}

async fn start_test_bridge_with_executor(
    executor: Option<Arc<dyn ToolExecutor>>,
    permits: usize,
) -> (
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
    Arc<Semaphore>,
) {
    start_test_bridge_with_limits(executor, permits, RateLimitState::new()).await
}

async fn start_test_bridge() -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let (base_url, shutdown, task, _) = start_test_bridge_with_executor(None, 4).await;
    (base_url, shutdown, task)
}

async fn stop_test_bridge(shutdown: oneshot::Sender<()>, task: tokio::task::JoinHandle<()>) {
    shutdown.send(()).expect("signal test bridge shutdown");
    task.await.expect("join test bridge");
}

#[test]
fn bridge_endpoint_accepts_only_bound_loopback_addresses() {
    let ipv4 = BridgeEndpoint::new("127.0.0.1:3210".parse().unwrap()).unwrap();
    let ipv6 = BridgeEndpoint::new("[::1]:3210".parse().unwrap()).unwrap();

    assert_eq!(ipv4.base_url(), "http://127.0.0.1:3210");
    assert_eq!(ipv4.health_url(), "http://127.0.0.1:3210/health");
    assert_eq!(ipv6.base_url(), "http://[::1]:3210");
    assert!(BridgeEndpoint::new("127.0.0.1:0".parse().unwrap()).is_err());
    assert!(BridgeEndpoint::new("192.0.2.1:3210".parse().unwrap()).is_err());
}

#[test]
fn start_result_discloses_token_only_for_a_new_runtime() {
    let started = AgentBridgeStartResult::started("http://127.0.0.1:1".into(), "secret".into());
    let running = AgentBridgeStartResult::already_running("http://127.0.0.1:1".into());

    assert_eq!(started.token.as_deref(), Some("secret"));
    assert!(running.token.is_none());
}

#[test]
fn bearer_authorization_requires_exact_token() {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer expected"));

    assert!(is_authorized(&headers, "expected"));

    for candidate in [
        "Bearer expectee",
        "Bearer xpected",
        "Bearer expected ",
        "Bearer different",
    ] {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(candidate).expect("valid authorization header"),
        );
        assert!(!is_authorized(&headers, "expected"), "{candidate}");
    }
}

#[test]
fn bridge_error_contract_accepts_only_static_codes_and_messages() {
    let bridge_source = include_str!("adapters/http_bridge.rs");
    let registry_source = include_str!("registry.rs");

    assert!(bridge_source.contains("code: &'static str,"));
    assert!(bridge_source.contains("message: &'static str,"));
    assert!(registry_source.contains("pub message: &'static str,"));
    let dynamic_message = ["message:", " String,"].concat();
    let dynamic_public_message = ["pub message:", " String,"].concat();
    assert!(!bridge_source.contains(&dynamic_message));
    assert!(!registry_source.contains(&dynamic_public_message));
}

#[test]
fn bridge_tracing_contains_no_dynamic_secrets_addresses_or_raw_errors() {
    let source = include_str!("adapters/http_bridge.rs");
    let marker = ["tracing", "::"].concat();
    let mut remaining = source;
    let mut invocation_count = 0;

    while let Some(start) = remaining.find(&marker) {
        remaining = &remaining[start..];
        let end = remaining
            .find(");")
            .expect("tracing invocation must terminate");
        let invocation = &remaining[..end + 2];
        invocation_count += 1;

        for forbidden in ["token", "base_url", "address", "?error", "%error"] {
            assert!(
                !invocation.contains(forbidden),
                "bridge tracing must not include {forbidden}: {invocation}"
            );
        }
        remaining = &remaining[end + 2..];
    }

    assert!(invocation_count > 0, "expected bridge tracing invocations");
}

#[test]
fn bearer_authorization_rejects_missing_or_invalid_scheme() {
    assert!(!is_authorized(&HeaderMap::new(), "expected"));

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic expected"));
    assert!(!is_authorized(&headers, "expected"));
}

#[test]
fn request_ids_are_random_fixed_length_hex() {
    let first = new_request_id();
    let second = new_request_id();
    assert_eq!(first.len(), 24);
    assert_eq!(second.len(), 24);
    assert_ne!(first, second);
    assert!(first.bytes().all(|value| value.is_ascii_hexdigit()));
}

#[test]
fn rate_limit_rejects_excess_calls_and_resets() {
    let started = std::time::Instant::now();
    let mut state = RateLimitState {
        window_started: started,
        calls: 0,
    };
    for _ in 0..MAX_TOOL_CALLS_PER_WINDOW {
        assert!(state.allow(started));
    }
    assert!(!state.allow(started));
    assert!(state.allow(started + RATE_LIMIT_WINDOW));
}

#[tokio::test]
async fn shutdown_aborts_a_task_that_ignores_the_signal() {
    let (shutdown, _) = oneshot::channel();
    let task = tokio::spawn(std::future::pending::<()>());
    let runtime = BridgeRuntime {
        endpoint: BridgeEndpoint::new("127.0.0.1:1".parse().unwrap()).unwrap(),
        shutdown,
        task,
    };

    timeout(
        BRIDGE_SHUTDOWN_TIMEOUT + std::time::Duration::from_secs(1),
        shutdown_runtime(runtime),
    )
    .await
    .expect("shutdown must remain bounded");
}

#[tokio::test]
async fn tool_endpoint_rejects_unauthorized_requests_with_correlated_request_id() {
    let (base_url, shutdown, task) = start_test_bridge().await;
    let response = test_http_client()
        .post(format!("{base_url}/agent/tools/core.status"))
        .body("{}")
        .send()
        .await
        .expect("send unauthorized request");

    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let header_id = response
        .headers()
        .get(super::REQUEST_ID_HEADER)
        .expect("request id header")
        .to_str()
        .expect("request id header text")
        .to_owned();
    let body: Value = response.json().await.expect("parse error response");
    assert_eq!(body["request_id"], header_id);
    assert_eq!(body["error"]["code"], "unauthorized");

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn unauthorized_oversized_requests_are_rejected_before_body_extraction() {
    let (base_url, shutdown, task) = start_test_bridge().await;
    let response = test_http_client()
        .post(format!("{base_url}/agent/tools/core.status"))
        .body(vec![b'x'; MAX_REQUEST_BODY_BYTES + 1])
        .send()
        .await
        .expect("send unauthorized oversized request");

    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let body: Value = response.json().await.expect("parse unauthorized response");
    assert_eq!(body["error"]["code"], "unauthorized");

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn bridge_error_responses_never_reflect_credentials_targets_or_request_bodies() {
    const TOKEN_CANARY: &str = "token-canary-never-reflect";
    const TOOL_CANARY: &str = "tool-canary-never-reflect";
    const TARGET_CANARY: &str = "https://target-canary.example/private?secret=value";

    let (base_url, shutdown, task) = start_test_bridge().await;
    let client = test_http_client();

    let unauthorized = client
        .post(format!("{base_url}/agent/tools/{TOOL_CANARY}"))
        .bearer_auth(TOKEN_CANARY)
        .body(TARGET_CANARY)
        .send()
        .await
        .expect("send unauthorized canary request");
    let unauthorized_body = unauthorized.text().await.expect("read unauthorized body");
    for canary in [TOKEN_CANARY, TOOL_CANARY, TARGET_CANARY, "secret=value"] {
        assert!(
            !unauthorized_body.contains(canary),
            "unauthorized response reflected sensitive canary: {canary}"
        );
    }

    let blocked = client
        .post(format!("{base_url}/agent/tools/network.probe"))
        .bearer_auth("expected")
        .json(&serde_json::json!({
            "arguments": {
                "url": TARGET_CANARY,
                "expected_status": 200,
                "timeout_ms": 1000
            }
        }))
        .send()
        .await
        .expect("send blocked target canary request");
    let blocked_body = blocked.text().await.expect("read blocked target body");
    for canary in [TARGET_CANARY, "target-canary.example", "secret=value"] {
        assert!(
            !blocked_body.contains(canary),
            "validation response reflected target canary: {canary}"
        );
    }

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn tool_endpoint_rejects_unknown_tools_and_invalid_json() {
    let (base_url, shutdown, task) = start_test_bridge().await;
    let client = test_http_client();
    let unknown = client
        .post(format!("{base_url}/agent/tools/not.registered"))
        .bearer_auth("expected")
        .body("{}")
        .send()
        .await
        .expect("send unknown tool request");
    assert_eq!(unknown.status(), reqwest::StatusCode::NOT_FOUND);
    let unknown_body: Value = unknown.json().await.expect("parse unknown tool response");
    assert_eq!(unknown_body["error"]["code"], "unknown_tool");

    let invalid = client
        .post(format!("{base_url}/agent/tools/core.status"))
        .bearer_auth("expected")
        .body("not-json")
        .send()
        .await
        .expect("send invalid json request");
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
    let invalid_body: Value = invalid.json().await.expect("parse invalid json response");
    assert_eq!(invalid_body["error"]["code"], "invalid_request");

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn tool_endpoint_rejects_oversized_requests_before_execution() {
    let (base_url, shutdown, task) = start_test_bridge().await;
    let response = test_http_client()
        .post(format!("{base_url}/agent/tools/core.status"))
        .bearer_auth("expected")
        .body(vec![b'x'; MAX_REQUEST_BODY_BYTES + 1])
        .send()
        .await
        .expect("send oversized request");

    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let body: Value = response.json().await.expect("parse oversized response");
    assert_eq!(body["error"]["code"], "request_too_large");

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn concurrency_limit_rejects_excess_requests_before_execution() {
    let release = Arc::new(Semaphore::new(0));
    let executor = Arc::new(GatedExecutor {
        calls: AtomicUsize::new(0),
        release: release.clone(),
    });
    let (base_url, shutdown, task, execution) =
        start_test_bridge_with_executor(Some(executor.clone()), MAX_CONCURRENT_TOOL_CALLS).await;
    let client = test_http_client();
    let mut active = Vec::new();
    for _ in 0..MAX_CONCURRENT_TOOL_CALLS {
        let client = client.clone();
        let url = format!("{base_url}/agent/tools/core.status");
        active.push(tokio::spawn(async move {
            client
                .post(url)
                .bearer_auth("expected")
                .body("{}")
                .send()
                .await
                .expect("send active tool request")
        }));
    }

    timeout(Duration::from_secs(1), async {
        while executor.calls.load(Ordering::SeqCst) != MAX_CONCURRENT_TOOL_CALLS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("all execution slots must become active");
    assert_eq!(execution.available_permits(), 0);

    let rejected = client
        .post(format!("{base_url}/agent/tools/core.status"))
        .bearer_auth("expected")
        .body("{}")
        .send()
        .await
        .expect("send excess tool request");
    assert_eq!(rejected.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let rejected_header = rejected
        .headers()
        .get(super::REQUEST_ID_HEADER)
        .expect("busy request id header")
        .to_str()
        .expect("busy request id text")
        .to_owned();
    let rejected_body: Value = rejected.json().await.expect("parse busy response");
    assert_eq!(rejected_body["request_id"], rejected_header);
    assert_eq!(rejected_body["error"]["code"], "bridge_busy");
    assert_eq!(
        executor.calls.load(Ordering::SeqCst),
        MAX_CONCURRENT_TOOL_CALLS
    );

    release.add_permits(MAX_CONCURRENT_TOOL_CALLS);
    for request in active {
        let response = request.await.expect("join active request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }
    assert_eq!(execution.available_permits(), MAX_CONCURRENT_TOOL_CALLS);

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn rate_limit_ignores_unauthorized_traffic_and_blocks_before_execution() {
    let executor = Arc::new(ImmediateExecutor {
        calls: AtomicUsize::new(0),
    });
    let rate_limit = RateLimitState {
        window_started: Instant::now(),
        calls: MAX_TOOL_CALLS_PER_WINDOW - 1,
    };
    let (base_url, shutdown, task, execution) =
        start_test_bridge_with_limits(Some(executor.clone()), 1, rate_limit).await;
    let client = test_http_client();

    let unauthorized = client
        .post(format!("{base_url}/agent/tools/core.status"))
        .body("{}")
        .send()
        .await
        .expect("send unauthorized request near rate limit");
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let allowed = client
        .post(format!("{base_url}/agent/tools/core.status"))
        .bearer_auth("expected")
        .body("{}")
        .send()
        .await
        .expect("send final allowed request");
    assert_eq!(allowed.status(), reqwest::StatusCode::OK);

    let limited = client
        .post(format!("{base_url}/agent/tools/core.status"))
        .bearer_auth("expected")
        .body("{}")
        .send()
        .await
        .expect("send rate-limited request");
    assert_eq!(limited.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let limited_header = limited
        .headers()
        .get(super::REQUEST_ID_HEADER)
        .expect("rate limit request id header")
        .to_str()
        .expect("rate limit request id text")
        .to_owned();
    let limited_body: Value = limited.json().await.expect("parse rate limit response");
    assert_eq!(limited_body["request_id"], limited_header);
    assert_eq!(limited_body["error"]["code"], "rate_limited");
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(execution.available_permits(), 1);

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn client_disconnect_cancels_execution_and_releases_the_permit() {
    let (started_tx, started_rx) = oneshot::channel();
    let (cancelled_tx, cancelled_rx) = oneshot::channel();
    let executor: Arc<dyn ToolExecutor> = Arc::new(PendingExecutor {
        calls: AtomicUsize::new(0),
        started: Mutex::new(Some(started_tx)),
        cancelled: Mutex::new(Some(cancelled_tx)),
    });
    let (base_url, shutdown, task, execution) =
        start_test_bridge_with_executor(Some(executor), 1).await;
    let address = base_url
        .strip_prefix("http://")
        .expect("test bridge URL prefix");
    let mut client = TcpStream::connect(address)
        .await
        .expect("connect raw bridge client");
    client
        .write_all(
            b"POST /agent/tools/core.status HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer expected\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
        )
        .await
        .expect("write tool request");

    timeout(Duration::from_secs(1), started_rx)
        .await
        .expect("tool execution must start")
        .expect("tool start signal");
    assert_eq!(execution.available_permits(), 0);

    drop(client);

    timeout(Duration::from_secs(1), cancelled_rx)
        .await
        .expect("disconnect must cancel execution")
        .expect("tool cancellation signal");
    timeout(Duration::from_secs(1), async {
        while execution.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("execution permit must be released");

    let recovered = test_http_client()
        .post(format!("{base_url}/agent/tools/core.status"))
        .bearer_auth("expected")
        .body("{}")
        .send()
        .await
        .expect("send request after disconnect");
    assert_eq!(recovered.status(), reqwest::StatusCode::OK);
    let recovered_body: Value = recovered.json().await.expect("parse recovered response");
    assert_eq!(recovered_body["output"]["recovered"], true);

    stop_test_bridge(shutdown, task).await;
}

#[tokio::test]
async fn stale_runtime_is_cleared_when_health_endpoint_is_unreachable() {
    let listener = TcpListener::bind(BRIDGE_BIND_ADDRESS).await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let (shutdown, _) = oneshot::channel();
    let task = tokio::spawn(std::future::pending::<()>());
    let mut runtime = Some(BridgeRuntime {
        endpoint: BridgeEndpoint::new(address).unwrap(),
        shutdown,
        task,
    });

    let health = HttpBridgeHealth::new();
    reconcile_runtime(&mut runtime, &health).await;

    assert!(runtime.is_none());
}

#[tokio::test]
async fn health_probe_rejects_wrong_schema_and_oversized_responses() {
    let listener = TcpListener::bind(BRIDGE_BIND_ADDRESS).await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/health",
                get(|| async {
                    axum::Json(serde_json::json!({
                        "status": "ok",
                        "schema_version": 999
                    }))
                }),
            ),
        )
        .await
    });
    let health = HttpBridgeHealth::new();
    assert!(!bridge_is_healthy(&health, BridgeEndpoint::new(address).unwrap()).await);
    task.abort();
    let _ = task.await;

    let listener = TcpListener::bind(BRIDGE_BIND_ADDRESS).await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/health",
                get(|| async { "x".repeat(MAX_HEALTH_RESPONSE_BYTES + 1) }),
            ),
        )
        .await
    });
    assert!(!bridge_is_healthy(&health, BridgeEndpoint::new(address).unwrap()).await);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn public_endpoints_serve_and_release_their_port() {
    let listener = TcpListener::bind(BRIDGE_BIND_ADDRESS).await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/health", get(health))
        .route("/agent/manifest", get(manifest));
    let (shutdown, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let client = test_http_client();
    let health = HttpBridgeHealth::new();
    assert!(bridge_is_healthy(&health, BridgeEndpoint::new(address).unwrap()).await);
    let health_response = client
        .get(format!("http://{address}/health"))
        .send()
        .await
        .unwrap();
    assert!(health_response.status().is_success());
    let health_body: Value = health_response.json().await.unwrap();
    assert_eq!(health_body["status"], "ok");

    let manifest_response = client
        .get(format!("http://{address}/agent/manifest"))
        .send()
        .await
        .unwrap();
    assert!(manifest_response.status().is_success());
    let manifest_body: Value = manifest_response.json().await.unwrap();
    assert_eq!(manifest_body["schema_version"], 1);
    let tool_names = manifest_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names,
        [
            "system.snapshot",
            "network.diagnose",
            "network.probe",
            "core.status",
            "proxy.status",
            "tun.status",
            "profile.summary",
            "service.status",
        ]
    );

    shutdown.send(()).unwrap();
    task.await.unwrap().unwrap();
    let rebound = TcpListener::bind(address).await.unwrap();
    drop(rebound);
}

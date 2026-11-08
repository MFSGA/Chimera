//! Instance-bound Clash HTTP/WebSocket client and cancellation semantics.
//! This is the real core-side protocol capability, not a DTO-only facade.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result};
use chimera_ipc::api::core::v2::CoreApiConnection;
use clash_api::{ProviderName, Proxy, ProxyName, ProxyProvider};
use futures_util::{Stream, StreamExt};
use indexmap::IndexMap;
use serde::Serialize;
use serde_yaml::Mapping;
use tokio_util::sync::CancellationToken;

use crate::clash::api::{ClashRuntimeConfig, ConnectionItem, ConnectionsRes, DelayRes};

/// Lifecycle notifications in the narrow core API boundary. The application
/// adapter projects its host's error type without exposing that host crate.
pub type ApiChanges = Pin<Box<dyn Stream<Item = Result<()>> + Send>>;

/// Narrow authority and lifecycle port injected by Local/Service adapters.
/// The core's API capability never depends on Tauri or the full host facade.
#[async_trait::async_trait]
pub trait CoreApiEndpoint: Send + Sync {
    async fn api_connection(&self) -> Result<Option<CoreApiConnection>>;
    async fn api_changes(&self) -> Result<Option<ApiChanges>>;
    fn host_label(&self) -> String;
}

/// Core-owned controller error semantics, matching ref's actor_v2::api.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("the Clash API capability belongs to a retired instance")]
    Stale,
    #[error("the running core's API is unavailable: {0}")]
    Unavailable(String),
    #[error("Clash API operation timed out; a submitted mutation may have taken effect")]
    Timeout,
    #[error(transparent)]
    Protocol(#[from] clash_api::Error),
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// A single instance-bound read of the proxy view, matching ref's actor API.
/// When `/group` is unavailable, its groups are inferred from `/proxies`.
pub struct ProxySnapshot {
    pub proxies: IndexMap<ProxyName, Proxy>,
    pub providers: IndexMap<ProviderName, ProxyProvider>,
    pub groups: Option<IndexMap<ProxyName, Proxy>>,
}

const DEFAULT_API_TIMEOUT: Duration = Duration::from_secs(30);
const DELAY_DEADLINE_MARGIN: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
pub enum ApiStreamKind {
    Connections,
    Logs,
    Traffic,
    Memory,
}

/// Compatibility adapter from the application's existing API/IPC DTOs to the
/// shared Clash API client. Production instances are bound to the host's
/// authoritative controller receipt; `new` is retained only for core startup
/// readiness probes and tests that do not have a CoreControl endpoint.
#[derive(Clone)]
pub struct ApiClient {
    client: clash_api::Client,
    binding: Option<chimera_ipc::api::core::v2::CoreApiConnection>,
    endpoint: Option<Arc<dyn CoreApiEndpoint>>,
    /// Clones share identity; separately acquired leases with the same
    /// authoritative CoreApiConnection compare equal by their binding.
    identity: Arc<()>,
    revoked: CancellationToken,
    /// Cache a conclusive `/group` availability answer per running core.
    group_list: Arc<OnceLock<bool>>,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiClient")
            .field("instance_id", &self.instance_id())
            .field("revoked", &self.revoked.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl ApiClient {
    pub fn instance_id(&self) -> Option<&str> {
        self.binding
            .as_ref()
            .map(|binding| binding.instance_id.as_str())
    }

    /// Whether two leases refer to the same running controller instance.
    /// Bound leases compare their full authority receipt; unbound startup/test
    /// clients compare only when they are clones of the same client.
    pub fn same_instance(&self, other: &Self) -> bool {
        if self.is_revoked() || other.is_revoked() {
            return false;
        }
        match (&self.binding, &other.binding) {
            (Some(left), Some(right)) => left == right,
            (None, None) => Arc::ptr_eq(&self.identity, &other.identity),
            _ => false,
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked.is_cancelled()
    }

    /// Explicitly unbound client for startup readiness probes and tests.
    /// Production controller calls must use `from_connection`.
    pub fn new_unbound(server: String, secret: Option<String>) -> Result<Self> {
        let host = clash_api::Host::http(format!("http://{server}"))?;
        Self::from_host(host, secret, false).context("failed to create Clash API client")
    }

    pub fn from_connection(
        binding: chimera_ipc::api::core::v2::CoreApiConnection,
        endpoint: Arc<dyn CoreApiEndpoint>,
    ) -> Result<Self> {
        anyhow::ensure!(
            !binding.instance_id.is_empty(),
            "the running core API has no stable instance identity"
        );
        use chimera_ipc::api::status::CoreControllerInfo;
        let host = match &binding.controller {
            CoreControllerInfo::Http(url) => clash_api::Host::url(url)?,
            CoreControllerInfo::UnixSocket(path) => clash_api::Host::unix_socket(path),
            CoreControllerInfo::NamedPipe(path) => clash_api::Host::named_pipe(path),
        };
        let mut client = Self::from_host(host, binding.secret.clone(), true)?;
        client.binding = Some(binding);
        client.endpoint = Some(endpoint);
        Ok(client)
    }

    fn from_host(
        host: clash_api::Host,
        secret: Option<String>,
        websocket_token_query: bool,
    ) -> Result<Self> {
        let mut builder = clash_api::Client::builder(host);
        if let Some(secret) = secret {
            builder = builder.secret(secret);
        }
        if websocket_token_query {
            builder = builder.websocket_token_query_compatibility();
        }
        Ok(Self {
            client: builder.build()?,
            binding: None,
            endpoint: None,
            identity: Arc::new(()),
            revoked: CancellationToken::new(),
            group_list: Arc::default(),
        })
    }

    pub fn matches_binding(&self, binding: &chimera_ipc::api::core::v2::CoreApiConnection) -> bool {
        !self.revoked.is_cancelled() && self.binding.as_ref() == Some(binding)
    }

    pub fn revoke(&self) {
        self.revoked.cancel();
    }

    /// Wait for permanent revocation without exposing the token to adapters.
    pub async fn cancelled(&self) {
        self.revoked.cancelled().await;
    }

    pub async fn check_binding(&self) -> ApiResult<()> {
        if self.revoked.is_cancelled() {
            return Err(ApiError::Stale);
        }
        let (Some(expected), Some(endpoint)) = (&self.binding, &self.endpoint) else {
            return Ok(());
        };
        match endpoint.api_connection().await {
            Ok(Some(current)) if current == *expected => Ok(()),
            Ok(_) => {
                self.revoke();
                Err(ApiError::Stale)
            }
            Err(error) => {
                self.revoke();
                Err(ApiError::Unavailable(error.to_string()))
            }
        }
    }

    /// Preflight, full protocol read/decode, and postflight share one deadline.
    /// No retry: a timed-out write may have been applied by the controller.
    async fn execute<T>(
        &self,
        timeout: Duration,
        operation: impl Future<Output = clash_api::Result<T>>,
    ) -> ApiResult<T> {
        tokio::select! {
            biased;
            _ = self.revoked.cancelled() => Err(ApiError::Stale),
            result = tokio::time::timeout(timeout, async {
                self.check_binding().await?;
                let result = operation.await;
                self.check_binding().await?;
                result.map_err(ApiError::Protocol)
            }) => result.map_err(|_| ApiError::Timeout)?,
        }
    }

    /// Wait until the applied core's HTTP controller is actually accepting
    /// authenticated requests. Process liveness alone is not API readiness.
    pub async fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            match tokio::time::timeout(Duration::from_secs(1), self.get_configs()).await {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(error)) if tokio::time::Instant::now() >= deadline => {
                    return Err(error).context(format!(
                        "Clash controller did not become ready within {timeout:?}"
                    ));
                }
                Err(_) if tokio::time::Instant::now() >= deadline => {
                    anyhow::bail!("Clash controller did not become ready within {timeout:?}");
                }
                Ok(Err(_)) | Err(_) => {}
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Typed protocol response for application-facing config queries, as in ref.
    /// Legacy runtime patch/agent paths continue to use the expanded snapshot
    /// below, which includes TUN and geodata extensions not in RuntimeConfig.
    pub async fn configs(&self) -> ApiResult<clash_api::RuntimeConfig> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.configs())
            .await
    }

    pub async fn get_configs(&self) -> Result<ClashRuntimeConfig> {
        let value = self
            .execute(DEFAULT_API_TIMEOUT, self.client.configs_value())
            .await?;
        serde_json::from_value(value).context("failed to decode running core config")
    }

    pub async fn version(&self) -> ApiResult<clash_api::Version> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.version())
            .await
    }

    pub async fn rules(&self) -> ApiResult<Vec<clash_api::Rule>> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.rules()).await
    }

    pub async fn rule_providers(
        &self,
    ) -> ApiResult<IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.rule_providers())
            .await
    }

    pub async fn update_rule_provider(&self, name: &clash_api::RuleProviderName) -> ApiResult<()> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.update_rule_provider(name))
            .await
    }

    pub async fn proxy_providers(
        &self,
    ) -> ApiResult<IndexMap<clash_api::ProviderName, clash_api::ProxyProvider>> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.proxy_providers())
            .await
    }

    pub async fn update_proxy_provider(&self, name: &clash_api::ProviderName) -> ApiResult<()> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.update_proxy_provider(name))
            .await
    }

    /// Provider-wide healthchecks can alter delay history even if their reply
    /// is lost. Keep the same lease checks and no-retry boundary as writes.
    pub async fn healthcheck_proxy_provider(
        &self,
        name: &clash_api::ProviderName,
    ) -> ApiResult<()> {
        self.execute(
            DEFAULT_API_TIMEOUT,
            self.client.healthcheck_proxy_provider(name),
        )
        .await
    }

    /// The complete data required to assemble a proxy view. Preflight,
    /// all network reads, decoding and postflight share one lifecycle lease.
    pub async fn proxy_snapshot(&self) -> ApiResult<ProxySnapshot> {
        self.execute(DEFAULT_API_TIMEOUT, async {
            let (proxies, providers, groups) = tokio::try_join!(
                self.client.proxies(),
                self.client.proxy_providers(),
                async { Ok::<_, clash_api::Error>(self.group_list().await) },
            )?;
            Ok::<_, clash_api::Error>(ProxySnapshot {
                proxies,
                providers,
                groups,
            })
        })
        .await
    }

    /// A missing route settles the probe for this instance; other failures
    /// are transient and the next snapshot may try `/group` again.
    async fn group_list(&self) -> Option<IndexMap<clash_api::ProxyName, clash_api::Proxy>> {
        if self.group_list.get() == Some(&false) {
            return None;
        }
        match self.client.groups().await {
            Ok(groups) => {
                let _ = self.group_list.set(true);
                Some(groups)
            }
            Err(error) => {
                if error
                    .status()
                    .is_some_and(|status| matches!(status.as_u16(), 404 | 405))
                {
                    let _ = self.group_list.set(false);
                }
                tracing::debug!(%error, "proxy group list unavailable; inferring groups");
                None
            }
        }
    }

    async fn execute_stream_frame<T>(&self, frame: clash_api::Result<T>) -> ApiResult<T> {
        self.check_binding().await?;
        let result = frame.map_err(ApiError::Protocol);
        self.check_binding().await?;
        result
    }

    pub async fn get_connections(&self) -> Result<ConnectionsRes> {
        let snapshot = self
            .execute(DEFAULT_API_TIMEOUT, self.client.connections())
            .await?;
        Ok(ConnectionsRes {
            connections: snapshot
                .connections
                .unwrap_or_default()
                .into_iter()
                .map(|connection| ConnectionItem {
                    id: connection.id.to_string(),
                    chains: connection.chains,
                })
                .collect(),
        })
    }

    pub async fn delete_connections(&self, id: Option<&str>) -> ApiResult<()> {
        match id {
            Some(id) => {
                self.execute(DEFAULT_API_TIMEOUT, self.client.close_connection_id(id))
                    .await
            }
            None => {
                self.execute(DEFAULT_API_TIMEOUT, self.client.close_all_connections())
                    .await
            }
        }
    }

    pub async fn update_proxy(&self, group: &str, name: &str) -> ApiResult<()> {
        let group = clash_api::ProxyName::new(group);
        let target = clash_api::ProxyName::new(name);
        self.execute(
            DEFAULT_API_TIMEOUT,
            self.client.select_proxy(clash_api::ProxySelection {
                group: &group,
                target: &target,
            }),
        )
        .await
    }

    /// Remove a pin from a capable automatic group. A lost DELETE reply may
    /// still mean the pin was removed; never retry the mutation automatically.
    pub async fn clear_proxy_selection(&self, group: &ProxyName) -> ApiResult<()> {
        self.execute(
            DEFAULT_API_TIMEOUT,
            self.client.clear_proxy_selection(group),
        )
        .await
    }

    pub async fn patch_configs(&self, config: &Mapping) -> Result<()> {
        let patch = serde_json::to_value(config).context("failed to encode config patch")?;
        self.execute(DEFAULT_API_TIMEOUT, self.client.patch_config_value(&patch))
            .await
            .map_err(Into::into)
    }

    /// Ref-compatible native delay capability. Provider healthcheck routes and
    /// expected HTTP statuses come from the typed query; only the controller's
    /// protocol client performs the request, within its bounded deadline.
    pub async fn proxy_delay(
        &self,
        name: &clash_api::ProxyName,
        provider: Option<&clash_api::ProviderName>,
        query: &clash_api::DelayQuery,
    ) -> ApiResult<clash_api::Delay> {
        let deadline = query.timeout + DELAY_DEADLINE_MARGIN;
        match provider {
            Some(provider) => {
                self.execute(
                    deadline,
                    self.client.provider_proxy_delay(provider, name, query),
                )
                .await
            }
            None => {
                self.execute(deadline, self.client.proxy_delay(name, query))
                    .await
            }
        }
    }

    /// Native group-delay API preserves proxy-name ordering and typed errors.
    pub async fn group_delay(
        &self,
        group: &clash_api::ProxyName,
        query: &clash_api::DelayQuery,
    ) -> ApiResult<IndexMap<clash_api::ProxyName, u16>> {
        self.execute(
            query.timeout + DELAY_DEADLINE_MARGIN,
            self.client.group_delay(group, query),
        )
        .await
    }

    /// Existing Tauri/legacy compatibility entry point; the native request
    /// itself is now implemented by the provider-aware core capability.
    pub async fn get_proxy_delay(
        &self,
        name: String,
        test_url: Option<String>,
    ) -> Result<DelayRes> {
        let query = crate::client::clash_api::delay_query(
            test_url.as_deref(),
            None,
            crate::client::clash_api::FALLBACK_LATENCY_TEST_URL,
            Duration::from_secs(10),
        )?;
        let delay = self
            .proxy_delay(&clash_api::ProxyName::new(name), None, &query)
            .await?;
        Ok(DelayRes {
            delay: u64::from(delay.delay),
        })
    }

    pub async fn get_group_delay(
        &self,
        group: String,
        url: Option<String>,
    ) -> Result<HashMap<String, u32>> {
        let query = crate::client::clash_api::delay_query(
            url.as_deref(),
            None,
            crate::client::clash_api::FALLBACK_LATENCY_TEST_URL,
            Duration::from_secs(10),
        )?;
        let delays = self
            .group_delay(&clash_api::ProxyName::new(group), &query)
            .await?;
        Ok(delays
            .into_iter()
            .map(|(name, delay)| (name.to_string(), u32::from(delay)))
            .collect())
    }

    pub async fn connections_ws(&self) -> ApiResult<ApiStream<clash_api::ConnectionsSnapshot>> {
        let stream = self
            .execute(
                DEFAULT_API_TIMEOUT,
                self.client
                    .connections_ws(clash_api::ConnectionStreamQuery::default()),
            )
            .await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn logs_ws(
        &self,
        level: clash_api::LogLevel,
    ) -> ApiResult<ApiStream<clash_api::LogEntry>> {
        let stream = self
            .execute(
                DEFAULT_API_TIMEOUT,
                self.client.logs_ws(clash_api::LogQuery::new(level)),
            )
            .await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn traffic_ws(&self) -> ApiResult<ApiStream<clash_api::Traffic>> {
        let stream = self
            .execute(DEFAULT_API_TIMEOUT, self.client.traffic_ws())
            .await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn memory_ws(&self) -> ApiResult<ApiStream<clash_api::Memory>> {
        let stream = self
            .execute(DEFAULT_API_TIMEOUT, self.client.memory_ws())
            .await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn connect_ws(
        &self,
        kind: ApiStreamKind,
    ) -> Result<tokio::sync::mpsc::Receiver<serde_json::Value>> {
        match kind {
            ApiStreamKind::Connections => {
                let stream = self
                    .execute(
                        DEFAULT_API_TIMEOUT,
                        self.client
                            .connections_ws(clash_api::ConnectionStreamQuery::default()),
                    )
                    .await?;
                Ok(forward_api_stream(
                    self.clone(),
                    stream,
                    connection_snapshot_value,
                ))
            }
            ApiStreamKind::Logs => {
                let stream = self
                    .execute(
                        DEFAULT_API_TIMEOUT,
                        self.client.logs_ws(clash_api::LogQuery::default()),
                    )
                    .await?;
                Ok(forward_api_stream(
                    self.clone(),
                    stream,
                    serialize_api_sample,
                ))
            }
            ApiStreamKind::Traffic => {
                let stream = self
                    .execute(DEFAULT_API_TIMEOUT, self.client.traffic_ws())
                    .await?;
                Ok(forward_api_stream(
                    self.clone(),
                    stream,
                    serialize_api_sample,
                ))
            }
            ApiStreamKind::Memory => {
                let stream = self
                    .execute(DEFAULT_API_TIMEOUT, self.client.memory_ws())
                    .await?;
                Ok(forward_api_stream(
                    self.clone(),
                    stream,
                    serialize_api_sample,
                ))
            }
        }
    }
}

/// An owned subscription bound to the same capability for its entire lifetime.
/// Revocation interrupts both idle reads and in-flight frame delivery.
pub struct ApiStream<T> {
    api: ApiClient,
    stream: Option<clash_api::WebSocketStream<T>>,
}

impl<T> ApiStream<T> {
    fn new(api: ApiClient, stream: clash_api::WebSocketStream<T>) -> Self {
        Self {
            api,
            stream: Some(stream),
        }
    }

    pub async fn next(&mut self) -> Option<ApiResult<T>> {
        let stream = self.stream.as_mut()?;
        let result = tokio::select! {
            biased;
            _ = self.api.cancelled() => Some(Err(ApiError::Stale)),
            frame = stream.next() => match frame {
                Some(frame) => Some(self.api.execute_stream_frame(frame).await),
                None => None,
            },
        };
        if result.is_none()
            || matches!(&result, Some(Err(error)) if !matches!(error, ApiError::Protocol(clash_api::Error::Decode { .. })))
        {
            self.stream.take();
        }
        result
    }
}

pub fn forward_api_stream<T, S, F>(
    api: ApiClient,
    mut stream: S,
    convert: F,
) -> tokio::sync::mpsc::Receiver<serde_json::Value>
where
    T: Send + 'static,
    S: Stream<Item = clash_api::Result<T>> + Unpin + Send + 'static,
    F: Fn(T) -> Result<serde_json::Value> + Send + 'static,
{
    let (sender, receiver) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = api.revoked.cancelled() => break,
                _ = sender.closed() => break,
                item = stream.next() => match item {
                    Some(Ok(sample)) => {
                        if let Err(error) = api.check_binding().await {
                            tracing::debug!(%error, "retired Clash API stream stopped");
                            break;
                        }
                        match convert(sample) {
                            Ok(value) => {
                                let result = tokio::select! {
                                    biased;
                                    _ = api.revoked.cancelled() => break,
                                    _ = sender.closed() => break,
                                    result = sender.send(value) => result,
                                };
                                if result.is_err() {
                                    break;
                                }
                            }
                            Err(error) => tracing::error!(%error, "failed to serialize Clash API stream sample"),
                        }
                    }
                    Some(Err(error)) => {
                        tracing::error!(%error, "failed to read Clash API websocket sample");
                    }
                    None => break,
                }
            }
        }
    });
    receiver
}

pub fn serialize_api_sample<T: Serialize>(sample: T) -> Result<serde_json::Value> {
    serde_json::to_value(sample).context("failed to serialize API sample")
}

pub(crate) fn connection_snapshot_value(
    snapshot: clash_api::ConnectionsSnapshot,
) -> Result<serde_json::Value> {
    let mut value =
        serde_json::to_value(snapshot).context("failed to serialize connection sample")?;
    if let Some(connections) = value
        .get_mut("connections")
        .and_then(serde_json::Value::as_array_mut)
    {
        for connection in connections {
            let Some(object) = connection.as_object_mut() else {
                continue;
            };
            let Some(extra) = object
                .remove("_extra")
                .and_then(|value| value.as_object().cloned())
            else {
                continue;
            };
            object.extend(extra);
        }
    }
    Ok(value)
}

/// Actor-owned lifecycle lease for the cached controller capability. The
/// endpoint's ordered notifications wake revocation; periodic authority checks
/// cover coalesced or lost events, as in ref's `actor_v2::ApiLease`.
pub struct ApiLease {
    pub client: ApiClient,
    monitor: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for ApiLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiLease")
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

impl ApiLease {
    pub fn new(client: ApiClient) -> Self {
        use futures_util::StreamExt;

        let monitored = client.clone();
        let endpoint = monitored
            .endpoint
            .as_ref()
            .expect("an API lease requires an instance-bound endpoint")
            .clone();
        let monitor = tokio::spawn(async move {
            const MONITOR_TIMEOUT: Duration = Duration::from_secs(10);
            let subscription = tokio::time::timeout(MONITOR_TIMEOUT, endpoint.api_changes()).await;
            let mut changes: ApiChanges = match subscription {
                Ok(Ok(Some(changes))) => changes,
                Ok(Ok(None)) => Box::pin(futures_util::stream::pending()),
                Ok(Err(error)) => {
                    tracing::warn!(host = %endpoint.host_label(), "API lifecycle subscription failed: {error}");
                    monitored.revoke();
                    return;
                }
                Err(error) => {
                    tracing::warn!(host = %endpoint.host_label(), "API lifecycle subscription timed out: {error}");
                    monitored.revoke();
                    return;
                }
            };
            let mut authority_check = tokio::time::interval(Duration::from_secs(2));
            loop {
                tokio::select! {
                    biased;
                    _ = monitored.cancelled() => return,
                    event = changes.next() => match event {
                        Some(Ok(())) => {}
                        Some(Err(error)) => {
                            tracing::warn!(host = %endpoint.host_label(), "API lifecycle stream failed: {error}");
                            monitored.revoke();
                            return;
                        }
                        None => {
                            tracing::warn!(host = %endpoint.host_label(), "API lifecycle stream ended; revoking API access");
                            monitored.revoke();
                            return;
                        }
                    },
                    _ = authority_check.tick() => {}
                }
                match tokio::time::timeout(MONITOR_TIMEOUT, monitored.check_binding()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) | Err(_) => {
                        monitored.revoke();
                        return;
                    }
                }
            }
        });
        Self { client, monitor }
    }
}

impl Drop for ApiLease {
    fn drop(&mut self) {
        self.client.revoke();
        self.monitor.abort();
    }
}

#[cfg(test)]
mod typed_error_tests {
    use super::{ApiChanges, ApiClient, ApiError, CoreApiEndpoint};
    use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
    use std::{
        future::pending,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    struct Endpoint {
        binding: Mutex<Option<CoreApiConnection>>,
        unavailable: AtomicBool,
        checks: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl CoreApiEndpoint for Endpoint {
        async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
            self.checks.fetch_add(1, Ordering::SeqCst);
            if self.unavailable.load(Ordering::SeqCst) {
                anyhow::bail!("controller authority is offline");
            }
            Ok(self.binding.lock().expect("binding lock").clone())
        }

        async fn api_changes(&self) -> anyhow::Result<Option<ApiChanges>> {
            Ok(None)
        }

        fn host_label(&self) -> String {
            "test-endpoint".into()
        }
    }

    fn bound() -> (ApiClient, Arc<Endpoint>) {
        let binding = CoreApiConnection {
            instance_id: "typed-api-errors".into(),
            controller: CoreControllerInfo::Http("http://127.0.0.1:1/".into()),
            secret: None,
        };
        let endpoint = Arc::new(Endpoint {
            binding: Mutex::new(Some(binding.clone())),
            unavailable: AtomicBool::new(false),
            checks: AtomicUsize::new(0),
        });
        (
            ApiClient::from_connection(binding, endpoint.clone()).unwrap(),
            endpoint,
        )
    }

    #[test]
    fn same_instance_requires_the_same_live_capability() {
        let unbound = ApiClient::new_unbound("127.0.0.1:1".into(), None).unwrap();
        let unbound_clone = unbound.clone();
        let unrelated = ApiClient::new_unbound("127.0.0.1:1".into(), None).unwrap();
        assert!(unbound.same_instance(&unbound_clone));
        assert!(!unbound.same_instance(&unrelated));

        unbound.revoke();
        assert!(!unbound.same_instance(&unbound_clone));

        let (bound, endpoint) = bound();
        let receipt = endpoint.binding.lock().unwrap().clone().unwrap();
        let same_receipt = ApiClient::from_connection(receipt, endpoint).unwrap();
        assert!(bound.same_instance(&same_receipt));
    }

    #[tokio::test]
    async fn native_delay_routes_provider_and_expected_status_without_replay() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            for index in 0..3 {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(2), listener.accept())
                        .await
                        .expect("client request timed out")
                        .expect("mock controller accept failed");
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..read]);
                paths.push(request.lines().next().unwrap().to_string());
                let body = if index == 2 {
                    r#"{"DIRECT":25}"#
                } else {
                    r#"{"delay":42}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: {}
Connection: close

{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            paths
        });
        let api = ApiClient::new_unbound(address.to_string(), None).unwrap();
        let query = crate::client::clash_api::delay_query(
            Some("https://example.test/generate_204"),
            Some("200/204"),
            "",
            Duration::from_millis(3200),
        )
        .unwrap();
        let name = clash_api::ProxyName::new("Shared Node");
        let provider = clash_api::ProviderName::new("Subscription A");

        let direct = api.proxy_delay(&name, None, &query).await.unwrap();
        let via_provider = api
            .proxy_delay(&name, Some(&provider), &query)
            .await
            .unwrap();
        let group = api
            .group_delay(&clash_api::ProxyName::new("Auto"), &query)
            .await
            .unwrap();

        assert_eq!(direct.delay, 42);
        assert_eq!(via_provider.delay, 42);
        assert_eq!(group.get(&clash_api::ProxyName::new("DIRECT")), Some(&25));

        let paths = server.await.unwrap();
        assert_eq!(paths.len(), 3, "exactly one request for each test");
        for (index, request_line) in paths.iter().enumerate() {
            let path_and_query = request_line.split_whitespace().nth(1).unwrap();
            let url = url::Url::parse(&format!("http://localhost{path_and_query}")).unwrap();
            let expected_path = match index {
                0 => "/proxies/Shared%20Node/delay",
                1 => "/providers/proxies/Subscription%20A/Shared%20Node/healthcheck",
                _ => "/group/Auto/delay",
            };
            assert_eq!(url.path(), expected_path);
            let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(params.get("expected").map(|v| v.as_ref()), Some("200/204"));
            assert_eq!(params.get("timeout").map(|v| v.as_ref()), Some("3200"));
            assert_eq!(
                params.get("url").map(|v| v.as_ref()),
                Some("https://example.test/generate_204")
            );
        }
    }

    #[tokio::test]
    async fn retired_instance_rejects_provider_healthcheck_before_dispatch() {
        let (api, endpoint) = bound();
        *endpoint.binding.lock().unwrap() = None;
        let result = api
            .healthcheck_proxy_provider(&clash_api::ProviderName::new("Subscription"))
            .await;
        assert!(matches!(result, Err(ApiError::Stale)));
        assert_eq!(endpoint.checks.load(Ordering::SeqCst), 1);
        assert!(api.revoked.is_cancelled());
    }

    #[tokio::test]
    async fn retired_instance_is_stale_before_any_mutation_and_stays_revoked() {
        let (client, endpoint) = bound();
        *endpoint.binding.lock().unwrap() = None;
        let executed = Arc::new(AtomicBool::new(false));
        let called = executed.clone();
        let result = client
            .execute(Duration::from_secs(1), async move {
                called.store(true, Ordering::SeqCst);
                Ok::<_, clash_api::Error>(())
            })
            .await;

        assert!(matches!(result, Err(ApiError::Stale)));
        assert!(!executed.load(Ordering::SeqCst));
        assert!(client.check_binding().await.is_err());
        assert!(matches!(client.check_binding().await, Err(ApiError::Stale)));
    }

    #[tokio::test]
    async fn authority_failure_is_unavailable_and_revokes_the_capability() {
        let (client, endpoint) = bound();
        endpoint.unavailable.store(true, Ordering::SeqCst);
        let result = client
            .execute(Duration::from_secs(1), async {
                Ok::<_, clash_api::Error>(())
            })
            .await;

        match result {
            Err(ApiError::Unavailable(reason)) => {
                assert!(reason.contains("controller authority is offline"));
            }
            other => panic!("expected unavailable authority: {other:?}"),
        }
        assert!(matches!(client.check_binding().await, Err(ApiError::Stale)));
    }

    #[tokio::test]
    async fn timeout_is_distinct_from_protocol_failure() {
        let client = ApiClient::new_unbound("127.0.0.1:1".into(), None).unwrap();
        let timeout = client
            .execute(
                Duration::from_millis(15),
                pending::<clash_api::Result<()>>(),
            )
            .await;
        assert!(matches!(timeout, Err(ApiError::Timeout)));

        let protocol = client
            .execute(Duration::from_secs(1), async {
                Err::<(), clash_api::Error>(clash_api::Error::InvalidArgument {
                    argument: "fixture",
                    message: "bad value".into(),
                })
            })
            .await;
        assert!(matches!(protocol, Err(ApiError::Protocol(_))));
    }

    #[tokio::test]
    async fn postflight_rejects_completion_on_a_replaced_instance() {
        let (client, endpoint) = bound();
        let swap = endpoint.clone();
        let result = client
            .execute(Duration::from_secs(1), async move {
                *swap.binding.lock().unwrap() = None;
                Ok::<_, clash_api::Error>(())
            })
            .await;

        assert!(matches!(result, Err(ApiError::Stale)));
        assert_eq!(endpoint.checks.load(Ordering::SeqCst), 2);
        assert!(client.revoked.is_cancelled());
    }
}

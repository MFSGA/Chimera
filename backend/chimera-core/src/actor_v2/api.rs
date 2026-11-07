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

/// A single instance-bound read of the proxy view, matching ref's actor API.
/// When `/group` is unavailable, its groups are inferred from `/proxies`.
pub struct ProxySnapshot {
    pub proxies: IndexMap<ProxyName, Proxy>,
    pub providers: IndexMap<ProviderName, ProxyProvider>,
    pub groups: Option<IndexMap<ProxyName, Proxy>>,
}

const DEFAULT_API_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_DELAY_URL: &str = "http://www.gstatic.com/generate_204";

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

    pub async fn check_binding(&self) -> Result<()> {
        anyhow::ensure!(
            !self.revoked.is_cancelled(),
            "the Clash API capability belongs to a retired core instance"
        );
        let (Some(expected), Some(endpoint)) = (&self.binding, &self.endpoint) else {
            return Ok(());
        };
        match endpoint.api_connection().await {
            Ok(Some(current)) if current == *expected => Ok(()),
            Ok(_) => {
                self.revoke();
                anyhow::bail!("the Clash API capability belongs to a retired core instance")
            }
            Err(error) => {
                self.revoke();
                Err(anyhow::anyhow!(
                    "the running core API binding is unavailable: {error}"
                ))
            }
        }
    }

    async fn execute<T, E>(
        &self,
        timeout: Duration,
        operation: impl Future<Output = std::result::Result<T, E>>,
    ) -> Result<T>
    where
        E: Into<anyhow::Error>,
    {
        let execution = async {
            self.check_binding().await?;
            let result = operation.await.map_err(Into::into);
            self.check_binding().await?;
            result
        };
        tokio::select! {
            biased;
            _ = self.revoked.cancelled() => {
                anyhow::bail!("the Clash API capability belongs to a retired core instance")
            }
            result = tokio::time::timeout(timeout, execution) => {
                result.map_err(|_| anyhow::anyhow!("Clash API operation timed out after {timeout:?}"))?
            }
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
    pub async fn configs(&self) -> Result<clash_api::RuntimeConfig> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.configs())
            .await
    }

    pub async fn get_configs(&self) -> Result<ClashRuntimeConfig> {
        let value = self
            .execute(DEFAULT_API_TIMEOUT, self.client.configs_value())
            .await?;
        serde_json::from_value(value).context("failed to decode running core config")
    }

    pub async fn version(&self) -> Result<clash_api::Version> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.version())
            .await
    }

    pub async fn rules(&self) -> Result<Vec<clash_api::Rule>> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.rules()).await
    }

    pub async fn rule_providers(
        &self,
    ) -> Result<IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.rule_providers())
            .await
    }

    pub async fn update_rule_provider(&self, name: &clash_api::RuleProviderName) -> Result<()> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.update_rule_provider(name))
            .await
    }

    pub async fn proxy_providers(
        &self,
    ) -> Result<IndexMap<clash_api::ProviderName, clash_api::ProxyProvider>> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.proxy_providers())
            .await
    }

    pub async fn update_proxy_provider(&self, name: &clash_api::ProviderName) -> Result<()> {
        self.execute(DEFAULT_API_TIMEOUT, self.client.update_proxy_provider(name))
            .await
    }

    /// The complete data required to assemble a proxy view. Preflight,
    /// all network reads, decoding and postflight share one lifecycle lease.
    pub async fn proxy_snapshot(&self) -> Result<ProxySnapshot> {
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

    pub async fn delete_connections(&self, id: Option<&str>) -> Result<()> {
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

    pub async fn update_proxy(&self, group: &str, name: &str) -> Result<()> {
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

    pub async fn patch_configs(&self, config: &Mapping) -> Result<()> {
        let patch = serde_json::to_value(config).context("failed to encode config patch")?;
        self.execute(DEFAULT_API_TIMEOUT, self.client.patch_config_value(&patch))
            .await
    }

    pub async fn get_proxy_delay(
        &self,
        name: String,
        test_url: Option<String>,
    ) -> Result<DelayRes> {
        let query = delay_query(test_url)?;
        let delay = self
            .execute(
                query.timeout + Duration::from_secs(5),
                self.client
                    .proxy_delay(&clash_api::ProxyName::new(name), &query),
            )
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
        let query = delay_query(url)?;
        let delays = self
            .execute(
                query.timeout + Duration::from_secs(5),
                self.client
                    .group_delay(&clash_api::ProxyName::new(group), &query),
            )
            .await?;
        Ok(delays
            .into_iter()
            .map(|(name, delay)| (name.to_string(), u32::from(delay)))
            .collect())
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

fn connection_snapshot_value(
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

fn delay_query(url: Option<String>) -> Result<clash_api::DelayQuery> {
    let url = url
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_DELAY_URL.to_owned());
    clash_api::DelayQuery::new(
        url::Url::parse(&url).context("invalid delay test URL")?,
        Duration::from_secs(10),
    )
    .context("invalid delay query")
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

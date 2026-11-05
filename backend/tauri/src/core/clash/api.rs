use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use futures_util::{Stream, StreamExt};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::core::actor_v2::control_endpoint::ControlEndpoint;

/// TUN runtime projection returned by the running core's `GET /configs` endpoint.
/// Keep this IPC-facing projection intentionally limited to fields Chimera
/// currently consumes; serde ignores additional runtime fields.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuntimeTun {
    pub enable: bool,
    pub device: String,
    pub auto_route: bool,
    pub auto_detect_interface: bool,
    pub strict_route: bool,
    pub route_address: Vec<String>,
    pub route_exclude_address: Vec<String>,
    pub inet4_route_address: Vec<String>,
    pub inet6_route_address: Vec<String>,
}

/// Runtime state returned by the running core's `GET /configs` endpoint.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
pub struct ClashRuntimeConfig {
    pub port: Option<u16>,
    pub mode: Option<String>,
    #[serde(rename = "geodata-mode")]
    pub geodata_mode: Option<bool>,
    pub ipv6: Option<bool>,
    #[serde(rename = "socket-port")]
    pub socket_port: Option<u16>,
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    #[serde(rename = "socks-port")]
    pub socks_port: Option<u16>,
    #[serde(rename = "tproxy-port")]
    pub tproxy_port: Option<u16>,
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>,
    pub secret: Option<String>,
    #[specta(skip)]
    pub tun: Option<RuntimeTun>,
}

const DEFAULT_API_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_DELAY_URL: &str = "http://www.gstatic.com/generate_204";

#[derive(Debug, Clone, Copy)]
pub(crate) enum ApiStreamKind {
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
pub(crate) struct ApiClient {
    client: chimera_clash_api::Client,
    binding: Option<chimera_ipc::api::core::v2::CoreApiConnection>,
    endpoint: Option<Arc<dyn ControlEndpoint>>,
    revoked: CancellationToken,
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
    pub(crate) fn instance_id(&self) -> Option<&str> {
        self.binding
            .as_ref()
            .map(|binding| binding.instance_id.as_str())
    }

    pub(crate) fn new(info: crate::config::clash::ClashInfo) -> Result<Self> {
        let host = chimera_clash_api::Host::http(format!("http://{}", info.server))?;
        Self::from_host(host, info.secret, false).context("failed to create Clash API client")
    }

    pub(crate) fn from_connection(
        binding: chimera_ipc::api::core::v2::CoreApiConnection,
        endpoint: Arc<dyn ControlEndpoint>,
    ) -> Result<Self> {
        anyhow::ensure!(
            !binding.instance_id.is_empty(),
            "the running core API has no stable instance identity"
        );
        use chimera_ipc::api::status::CoreControllerInfo;
        let host = match &binding.controller {
            CoreControllerInfo::Http(url) => chimera_clash_api::Host::url(url)?,
            CoreControllerInfo::UnixSocket(path) => chimera_clash_api::Host::unix_socket(path),
            CoreControllerInfo::NamedPipe(path) => chimera_clash_api::Host::named_pipe(path),
        };
        let mut client = Self::from_host(host, binding.secret.clone(), true)?;
        client.binding = Some(binding);
        client.endpoint = Some(endpoint);
        Ok(client)
    }

    fn from_host(
        host: chimera_clash_api::Host,
        secret: Option<String>,
        websocket_token_query: bool,
    ) -> Result<Self> {
        let mut builder = chimera_clash_api::Client::builder(host);
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
        })
    }

    pub(crate) fn matches_binding(
        &self,
        binding: &chimera_ipc::api::core::v2::CoreApiConnection,
    ) -> bool {
        !self.revoked.is_cancelled() && self.binding.as_ref() == Some(binding)
    }

    pub(crate) fn revoke(&self) {
        self.revoked.cancel();
    }

    async fn check_binding(&self) -> Result<()> {
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
    pub(crate) async fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
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

    pub(crate) async fn get_configs(&self) -> Result<ClashRuntimeConfig> {
        let value = self
            .execute(DEFAULT_API_TIMEOUT, self.client.configs_value())
            .await?;
        serde_json::from_value(value).context("failed to decode running core config")
    }

    pub(crate) async fn get_proxies(&self) -> Result<ProxiesRes> {
        let proxies = self
            .execute(DEFAULT_API_TIMEOUT, self.client.proxies())
            .await?;
        let proxies = proxies
            .into_iter()
            .map(|(name, proxy)| {
                let item = ProxyItem {
                    name: proxy.name.to_string(),
                    history: proxy
                        .history
                        .into_iter()
                        .map(|history| ProxyItemHistory {
                            time: history.time.to_rfc3339(),
                            delay: history.delay,
                        })
                        .collect(),
                    all: proxy
                        .all
                        .map(|names| names.into_iter().map(|name| name.to_string()).collect()),
                    hidden: proxy.hidden.unwrap_or_default(),
                    now: proxy.now.map(|name| name.to_string()),
                    r#type: proxy.proxy_type,
                    udp: proxy.udp,
                    icon: proxy.icon,
                };
                (name.to_string(), item)
            })
            .collect();
        Ok(ProxiesRes { proxies })
    }

    pub(crate) async fn get_connections(&self) -> Result<ConnectionsRes> {
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

    pub(crate) async fn delete_connections(&self, id: Option<&str>) -> Result<()> {
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

    pub(crate) async fn update_proxy(&self, group: &str, name: &str) -> Result<()> {
        let group = chimera_clash_api::ProxyName::new(group);
        let target = chimera_clash_api::ProxyName::new(name);
        self.execute(
            DEFAULT_API_TIMEOUT,
            self.client.select_proxy(chimera_clash_api::ProxySelection {
                group: &group,
                target: &target,
            }),
        )
        .await
    }

    pub(crate) async fn patch_configs(&self, config: &Mapping) -> Result<()> {
        let patch = serde_json::to_value(config).context("failed to encode config patch")?;
        self.execute(DEFAULT_API_TIMEOUT, self.client.patch_config_value(&patch))
            .await
    }

    pub(crate) async fn get_proxy_delay(
        &self,
        name: String,
        test_url: Option<String>,
    ) -> Result<DelayRes> {
        let query = delay_query(test_url)?;
        let delay = self
            .execute(
                query.timeout + Duration::from_secs(5),
                self.client
                    .proxy_delay(&chimera_clash_api::ProxyName::new(name), &query),
            )
            .await?;
        Ok(DelayRes {
            delay: u64::from(delay.delay),
        })
    }

    pub(crate) async fn get_group_delay(
        &self,
        group: String,
        url: Option<String>,
    ) -> Result<HashMap<String, u32>> {
        let query = delay_query(url)?;
        let delays = self
            .execute(
                query.timeout + Duration::from_secs(5),
                self.client
                    .group_delay(&chimera_clash_api::ProxyName::new(group), &query),
            )
            .await?;
        Ok(delays
            .into_iter()
            .map(|(name, delay)| (name.to_string(), u32::from(delay)))
            .collect())
    }

    pub(crate) async fn connect_ws(
        &self,
        kind: ApiStreamKind,
    ) -> Result<tokio::sync::mpsc::Receiver<serde_json::Value>> {
        match kind {
            ApiStreamKind::Connections => {
                let stream = self
                    .execute(
                        DEFAULT_API_TIMEOUT,
                        self.client
                            .connections_ws(chimera_clash_api::ConnectionStreamQuery::default()),
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
                        self.client.logs_ws(chimera_clash_api::LogQuery::default()),
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

fn forward_api_stream<T, S, F>(
    api: ApiClient,
    mut stream: S,
    convert: F,
) -> tokio::sync::mpsc::Receiver<serde_json::Value>
where
    T: Send + 'static,
    S: Stream<Item = chimera_clash_api::Result<T>> + Unpin + Send + 'static,
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

fn serialize_api_sample<T: Serialize>(sample: T) -> Result<serde_json::Value> {
    serde_json::to_value(sample).context("failed to serialize API sample")
}

fn connection_snapshot_value(
    snapshot: chimera_clash_api::ConnectionsSnapshot,
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

fn delay_query(url: Option<String>) -> Result<chimera_clash_api::DelayQuery> {
    let url = url
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_DELAY_URL.to_owned());
    chimera_clash_api::DelayQuery::new(
        url::Url::parse(&url).context("invalid delay test URL")?,
        Duration::from_secs(10),
    )
    .context("invalid delay query")
}

/// Actor-owned lifecycle lease for the cached controller capability. The
/// endpoint's ordered notifications wake revocation; periodic authority checks
/// cover coalesced or lost events, as in ref's `actor_v2::ApiLease`.
pub(crate) struct ApiLease {
    pub(crate) client: ApiClient,
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
    pub(crate) fn new(client: ApiClient) -> Self {
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
            let mut changes: crate::core::actor_v2::control_endpoint::ApiChanges =
                match subscription {
                    Ok(Ok(Some(changes))) => changes,
                    Ok(Ok(None)) => Box::pin(futures_util::stream::pending()),
                    Ok(Err(error)) => {
                        tracing::warn!(host = ?endpoint.host(), "API lifecycle subscription failed: {error}");
                        monitored.revoke();
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(host = ?endpoint.host(), "API lifecycle subscription timed out: {error}");
                        monitored.revoke();
                        return;
                    }
                };
            let mut authority_check = tokio::time::interval(Duration::from_secs(2));
            loop {
                tokio::select! {
                    biased;
                    _ = monitored.revoked.cancelled() => return,
                    event = changes.next() => match event {
                        Some(Ok(())) => {}
                        Some(Err(error)) => {
                            tracing::warn!(host = ?endpoint.host(), "API lifecycle stream failed: {error}");
                            monitored.revoke();
                            return;
                        }
                        None => {
                            tracing::warn!(host = ?endpoint.host(), "API lifecycle stream ended; revoking API access");
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

/// 缩短clash的日志
#[instrument]
pub fn parse_log(log: String) -> String {
    if log.starts_with("time=") && log.len() > 33 {
        return log[33..].to_owned();
    }
    if log.len() > 9 {
        return log[9..].to_owned();
    }
    log
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItemHistory {
    pub time: String,
    pub delay: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItem {
    pub name: String,
    pub history: Vec<ProxyItemHistory>,
    pub all: Option<Vec<String>>,
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
    pub now: Option<String>, // 当前选中的代理
    pub r#type: String,      // TODO: 考虑改成枚举
    pub udp: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>, // Mihomo Only
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxiesRes {
    #[serde(default)]
    pub proxies: IndexMap<String, ProxyItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionItem {
    pub id: String,
    #[serde(default)]
    pub chains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsRes {
    #[serde(default)]
    pub connections: Vec<ConnectionItem>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    delay: u64,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use std::time::Duration;

    use super::{ApiClient, ClashRuntimeConfig, forward_api_stream, serialize_api_sample};
    use crate::core::actor_v2::control_endpoint::{
        ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    };

    struct MutableApiEndpoint {
        binding: parking_lot::RwLock<Option<chimera_ipc::api::core::v2::CoreApiConnection>>,
        changes: tokio::sync::watch::Sender<u64>,
        subscribed: tokio::sync::Notify,
    }

    impl MutableApiEndpoint {
        fn new(binding: chimera_ipc::api::core::v2::CoreApiConnection) -> Self {
            let (changes, _) = tokio::sync::watch::channel(0);
            Self {
                binding: parking_lot::RwLock::new(Some(binding)),
                changes,
                subscribed: tokio::sync::Notify::new(),
            }
        }

        fn replace_binding(&self, binding: chimera_ipc::api::core::v2::CoreApiConnection) {
            *self.binding.write() = Some(binding);
            self.changes.send_modify(|revision| *revision += 1);
        }
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for MutableApiEndpoint {
        async fn api_connection(
            &self,
        ) -> std::result::Result<
            Option<chimera_ipc::api::core::v2::CoreApiConnection>,
            chimera_core_manager::CoreError,
        > {
            Ok(self.binding.read().clone())
        }

        async fn api_changes(
            &self,
        ) -> std::result::Result<Option<ApiChanges>, chimera_core_manager::CoreError> {
            self.subscribed.notify_one();
            let receiver = self.changes.subscribe();
            let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
                receiver.changed().await.ok()?;
                Some((Ok(()), receiver))
            });
            Ok(Some(Box::pin(stream)))
        }

        fn host(&self) -> ExecutionHost {
            ExecutionHost::Local
        }

        async fn submit(
            &self,
            _submission: CoreSubmission,
        ) -> std::result::Result<
            chimera_ipc::api::core::v2::OperationInfo,
            chimera_core_manager::CoreError,
        > {
            unreachable!("the API binding test does not submit lifecycle commands")
        }

        async fn wait_operation(
            &self,
            _id: chimera_core_manager::OperationId,
            _timeout: Duration,
        ) -> Option<chimera_ipc::api::core::v2::OperationInfo> {
            None
        }

        async fn status(
            &self,
        ) -> std::result::Result<CoreStatusSnapshot, chimera_core_manager::CoreError> {
            unreachable!("the API binding test does not query host status")
        }
    }

    #[tokio::test]
    async fn bound_client_rejects_a_replaced_core_before_sending_a_request() {
        use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};

        let first = CoreApiConnection {
            instance_id: "first-instance".into(),
            controller: CoreControllerInfo::Http("http://127.0.0.1:1/".into()),
            secret: Some("first-secret".into()),
        };
        let endpoint = Arc::new(MutableApiEndpoint::new(first.clone()));
        let api = ApiClient::from_connection(first, endpoint.clone())
            .expect("bound API client should build");
        let replacement = CoreApiConnection {
            instance_id: "replacement-instance".into(),
            controller: CoreControllerInfo::Http("http://127.0.0.1:1/".into()),
            secret: Some("replacement-secret".into()),
        };
        endpoint.replace_binding(replacement);

        let error = api
            .get_configs()
            .await
            .expect_err("a retired capability must not send a request");
        assert!(error.to_string().contains("retired core instance"));
    }

    #[tokio::test]
    async fn lifecycle_change_cancels_an_inflight_controller_response() {
        use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("controller listener should bind");
        let address = listener.local_addr().expect("controller address");
        let response_started = Arc::new(tokio::sync::Notify::new());
        let server_started = response_started.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("client should connect");
            let mut request = [0_u8; 2048];
            let read = stream
                .read(&mut request)
                .await
                .expect("request should read");
            assert!(read > 0);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: keep-alive\r\n\r\n{\"mode\":",
                )
                .await
                .expect("partial response should write");
            server_started.notify_one();
            let mut closed = [0_u8; 1];
            let _ = stream.read(&mut closed).await;
        });

        let binding = CoreApiConnection {
            instance_id: "first-instance".into(),
            controller: CoreControllerInfo::Http(format!("http://{address}/")),
            secret: None,
        };
        let endpoint = Arc::new(MutableApiEndpoint::new(binding.clone()));
        let endpoint_trait: Arc<dyn ControlEndpoint> = endpoint.clone();
        let api = ApiClient::from_connection(binding, endpoint_trait)
            .expect("bound API client should build");
        let _lease = super::ApiLease::new(api.clone());
        tokio::time::timeout(Duration::from_secs(1), endpoint.subscribed.notified())
            .await
            .expect("lease should subscribe to API lifecycle changes");

        let request_api = api.clone();
        let request = tokio::spawn(async move { request_api.get_configs().await });
        tokio::time::timeout(Duration::from_secs(1), response_started.notified())
            .await
            .expect("controller should start a partial response");
        endpoint.replace_binding(CoreApiConnection {
            instance_id: "replacement-instance".into(),
            controller: CoreControllerInfo::Http(format!("http://{address}/")),
            secret: None,
        });

        let error = tokio::time::timeout(Duration::from_secs(1), request)
            .await
            .expect("revocation should cancel the in-flight request")
            .expect("request task should finish")
            .expect_err("retired API capability should fail");
        assert!(error.to_string().contains("retired core instance"));
        server.abort();
    }

    #[tokio::test]
    async fn closing_api_stream_receiver_drops_the_upstream_stream() {
        struct DropProbe(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }

        let (dropped, upstream_dropped) = tokio::sync::oneshot::channel();
        let probe = DropProbe(Some(dropped));
        let stream = futures_util::stream::poll_fn(move |_| {
            let _ = &probe;
            std::task::Poll::<Option<chimera_clash_api::Result<serde_json::Value>>>::Pending
        });
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: 1,
            server: "127.0.0.1:1".to_owned(),
            secret: None,
        })
        .expect("API client should build");

        let receiver = forward_api_stream(api, stream, serialize_api_sample::<serde_json::Value>);
        drop(receiver);

        tokio::time::timeout(Duration::from_secs(1), upstream_dropped)
            .await
            .expect("closing the receiver should drop the upstream stream")
            .expect("upstream drop signal should be sent");
    }

    #[tokio::test]
    async fn lifecycle_revocation_interrupts_backpressured_stream_delivery() {
        struct DropProbe(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }

        let (dropped, upstream_dropped) = tokio::sync::oneshot::channel();
        let (blocked, delivery_blocked) = tokio::sync::oneshot::channel();
        let probe = DropProbe(Some(dropped));
        let mut blocked = Some(blocked);
        let mut next = 0;
        let stream = futures_util::stream::poll_fn(move |_| {
            let _ = &probe;
            let index = next;
            next += 1;
            if index == 32 {
                if let Some(blocked) = blocked.take() {
                    let _ = blocked.send(());
                }
            }
            std::task::Poll::<Option<chimera_clash_api::Result<serde_json::Value>>>::Ready(Some(
                Ok(serde_json::json!({ "index": index })),
            ))
        });
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: 1,
            server: "127.0.0.1:1".to_owned(),
            secret: None,
        })
        .expect("API client should build");

        let receiver = forward_api_stream(
            api.clone(),
            stream,
            serialize_api_sample::<serde_json::Value>,
        );
        tokio::time::timeout(Duration::from_secs(1), delivery_blocked)
            .await
            .expect("the forwarding channel should fill")
            .expect("blocked delivery signal should be sent");
        assert_eq!(receiver.len(), 32, "the consumer must remain backpressured");

        api.revoke();
        tokio::time::timeout(Duration::from_secs(1), upstream_dropped)
            .await
            .expect("revocation should stop the backpressured stream worker")
            .expect("upstream drop signal should be sent");
    }

    #[tokio::test]
    async fn readiness_requires_an_responding_controller() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("probe should connect");
            let mut request = [0_u8; 1024];
            let read = stream
                .read(&mut request)
                .await
                .expect("request should read");
            assert!(read > 0, "readiness probe must send an HTTP request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .expect("response should write");
            stream.shutdown().await.expect("response should close");
        });
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: address.port(),
            server: address.to_string(),
            secret: None,
        })
        .expect("API client should build");

        api.wait_until_ready(Duration::from_secs(1))
            .await
            .expect("controller response should satisfy readiness");
        server.await.expect("server task should finish");
    }

    #[test]
    fn runtime_tun_deserializes_mihomo_kebab_case_fields() {
        let config: ClashRuntimeConfig = serde_json::from_value(serde_json::json!({
            "mode": "rule",
            "geodata-mode": true,
            "tun": {
                "enable": true,
                "device": "Meta",
                "auto-route": true,
                "auto-detect-interface": true,
                "strict-route": false,
                "route-address": ["0.0.0.0/1"],
                "route-exclude-address": ["192.168.0.0/16"],
                "inet4-route-address": ["128.0.0.0/1"],
                "inet6-route-address": ["::/1", "8000::/1"]
            }
        }))
        .expect("runtime config should deserialize");

        assert_eq!(config.geodata_mode, Some(true));
        let tun = config.tun.expect("TUN runtime projection");
        assert!(tun.enable);
        assert_eq!(tun.device, "Meta");
        assert!(tun.auto_route);
        assert!(tun.auto_detect_interface);
        assert_eq!(tun.route_address, vec!["0.0.0.0/1"]);
        assert_eq!(tun.inet4_route_address, vec!["128.0.0.0/1"]);
        assert_eq!(tun.inet6_route_address, vec!["::/1", "8000::/1"]);
    }
}

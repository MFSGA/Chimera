//! App-owned CoreControl adapter and lifecycle notification monitor.
//! The complete instance-bound Clash API implementation lives in
//! `chimera_core::actor_v2::api`, with no Tauri dependency.

use std::{ops::Deref, sync::Arc};

use super::control_endpoint::ControlEndpoint;
use anyhow::Result;
use chimera_core::actor_v2::api::{
    ApiChanges as CoreApiChanges, ApiClient as CoreApiClient, ApiLease as CoreApiLease,
    CoreApiEndpoint,
};
use chimera_ipc::api::core::v2::CoreApiConnection;
use futures_util::StreamExt;

pub(crate) use chimera_core::actor_v2::api::ProxySnapshot;
#[cfg(test)]
use chimera_core::actor_v2::api::{forward_api_stream, serialize_api_sample};
#[cfg(test)]
use chimera_core::clash::api::ClashRuntimeConfig;

/// Bridges the application's full CoreControl endpoint to the core's narrow
/// read-only authority check. No endpoint lifecycle command enters the core.
struct CoreEndpointAdapter(Arc<dyn ControlEndpoint>);

#[async_trait::async_trait]
impl CoreApiEndpoint for CoreEndpointAdapter {
    async fn api_connection(&self) -> Result<Option<CoreApiConnection>> {
        self.0
            .api_connection()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn api_changes(&self) -> Result<Option<CoreApiChanges>> {
        let changes = self
            .0
            .api_changes()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(changes.map(|changes| {
            Box::pin(changes.map(|event| event.map_err(|error| anyhow::anyhow!(error.to_string()))))
                as CoreApiChanges
        }))
    }

    fn host_label(&self) -> String {
        format!("{:?}", self.0.host())
    }
}

/// Existing application-side constructor and lifecycle adapter. Deref keeps
/// callers working while the protocol/client methods now run in chimera-core.
#[derive(Clone)]
pub(crate) struct ApiClient {
    inner: CoreApiClient,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ApiClient").field(&self.inner).finish()
    }
}

impl Deref for ApiClient {
    type Target = CoreApiClient;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl ApiClient {
    pub(crate) fn new(info: crate::config::clash::ClashInfo) -> Result<Self> {
        Ok(Self {
            inner: CoreApiClient::new_unbound(info.server, info.secret)?,
        })
    }

    pub(crate) fn from_connection(
        binding: CoreApiConnection,
        endpoint: Arc<dyn ControlEndpoint>,
    ) -> Result<Self> {
        let check: Arc<dyn CoreApiEndpoint> = Arc::new(CoreEndpointAdapter(endpoint.clone()));
        Ok(Self {
            inner: CoreApiClient::from_connection(binding, check)?,
        })
    }

    /// Transfer the instance-bound core lease to core-owned actors without
    /// exposing the Tauri ControlEndpoint adapter.
    pub(crate) fn core_client(&self) -> CoreApiClient {
        self.inner.clone()
    }
}

/// Keep existing facade storage and API signatures; monitor ownership lives
/// in chimera-core's ApiLease and drop revokes the same shared capability.
pub(crate) struct ApiLease {
    pub(crate) client: ApiClient,
    _lease: CoreApiLease,
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
        let lease = CoreApiLease::new(client.inner.clone());
        Self {
            client,
            _lease: lease,
        }
    }
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
    async fn bound_client_reads_configs_rules_and_version_through_the_controller() {
        use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test controller should bind");
        let address = listener.local_addr().expect("controller address");
        let server = tokio::spawn(async move {
            for (path, body) in [
                (
                    "/configs",
                    r#"{"mode":"rule","mixed-port":7890,"ipv6":false}"#,
                ),
                (
                    "/rules/",
                    r#"{"rules":[{"type":"DOMAIN-SUFFIX","payload":"example.test","proxy":"DIRECT"}]}"#,
                ),
                (
                    "/version",
                    r#"{"version":"1.2.3","premium":false,"meta":true}"#,
                ),
            ] {
                let (mut socket, _) = listener.accept().await.expect("controller request");
                let mut bytes = [0_u8; 2048];
                let read = socket.read(&mut bytes).await.expect("request should read");
                let request = String::from_utf8_lossy(&bytes[..read]);
                assert!(request.starts_with(&format!("GET {path} ")), "{request}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("response should write");
            }
        });

        let binding = CoreApiConnection {
            instance_id: "api-queries".into(),
            controller: CoreControllerInfo::Http(format!("http://{address}/")),
            secret: None,
        };
        let endpoint = Arc::new(MutableApiEndpoint::new(binding.clone()));
        let api =
            ApiClient::from_connection(binding, endpoint).expect("bound API client should build");
        let config = api.configs().await.expect("typed config should be parsed");
        assert_eq!(
            config.mode.as_ref().map(|value| value.as_str()),
            Some("rule")
        );
        assert_eq!(config.mixed_port, Some(7890));
        assert_eq!(config.ipv6, Some(false));
        let rules = api.rules().await.expect("rules should be parsed");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].rule_type, "DOMAIN-SUFFIX");
        assert_eq!(rules[0].payload, "example.test");
        assert_eq!(rules[0].proxy, "DIRECT");
        let version = api.version().await.expect("version should be parsed");
        assert_eq!(version.version, "1.2.3");
        assert_eq!(version.premium, Some(false));
        assert!(version.meta);
        server.await.expect("controller server should complete");
    }

    #[tokio::test]
    async fn bound_client_reads_and_updates_providers_without_browser_http() {
        use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test controller should bind");
        let address = listener.local_addr().expect("controller address");
        let server = tokio::spawn(async move {
            for (method, path, response_body) in [
                (
                    "GET",
                    "/providers/rules/",
                    r#"{"providers":{"rules":{"name":"rules","behavior":"Domain","format":"TextRule","ruleCount":3,"type":"Rule","vehicleType":"HTTP"}}}"#,
                ),
                (
                    "GET",
                    "/providers/proxies",
                    r#"{"providers":{"proxies":{"name":"proxies","type":"Proxy","vehicleType":"HTTP","proxies":[],"testUrl":null,"expectedStatus":null}}}"#,
                ),
                ("PUT", "/providers/rules/rules/", ""),
                ("PUT", "/providers/proxies/proxies", ""),
            ] {
                let (mut socket, _) = listener.accept().await.expect("controller request");
                let mut bytes = [0_u8; 4096];
                let read = socket.read(&mut bytes).await.expect("request should read");
                let request = String::from_utf8_lossy(&bytes[..read]);
                assert!(
                    request.starts_with(&format!("{method} {path} ")),
                    "unexpected controller request: {request}"
                );
                let status = if method == "PUT" {
                    "204 No Content"
                } else {
                    "200 OK"
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                    response_body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("response should write");
            }
        });

        let binding = CoreApiConnection {
            instance_id: "api-provider-queries".into(),
            controller: CoreControllerInfo::Http(format!("http://{address}/")),
            secret: None,
        };
        let endpoint = Arc::new(MutableApiEndpoint::new(binding.clone()));
        let api =
            ApiClient::from_connection(binding, endpoint).expect("bound API client should build");

        let rules = api.rule_providers().await.expect("rule providers");
        let rules_item = &rules[&clash_api::RuleProviderName::new("rules")];
        assert_eq!(rules_item.name.as_str(), "rules");
        assert_eq!(rules_item.rule_count, Some(3));
        assert_eq!(
            rules_item.vehicle_type.as_ref().map(|value| value.as_str()),
            Some("HTTP")
        );

        let proxies = api.proxy_providers().await.expect("proxy providers");
        let proxy_item = &proxies[&clash_api::ProviderName::new("proxies")];
        assert!(proxy_item.proxies.is_empty());
        assert_eq!(proxy_item.vehicle_type.as_str(), "HTTP");

        api.update_rule_provider(&clash_api::RuleProviderName::new("rules"))
            .await
            .expect("rule provider update");
        api.update_proxy_provider(&clash_api::ProviderName::new("proxies"))
            .await
            .expect("proxy provider update");
        server.await.expect("controller server should complete");
    }

    #[tokio::test]
    async fn proxy_snapshot_reads_controller_views_and_caches_unsupported_group_route() {
        use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock controller should bind");
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            // The first snapshot probes all three endpoints; the second must
            // not re-probe /group once it has definitively returned 404.
            for _ in 0..5 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0_u8; 4096];
                let read = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..read]);
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .expect("request path")
                    .to_owned();
                let (status, body) = if path.starts_with("/providers/proxies") {
                    ("200 OK", r#"{"providers":{}}"#)
                } else if path.starts_with("/proxies") {
                    (
                        "200 OK",
                        r#"{"proxies":{"DIRECT":{"name":"DIRECT","type":"Direct","history":[],"udp":false},"REJECT":{"name":"REJECT","type":"Reject","history":[],"udp":false}}}"#,
                    )
                } else if path.starts_with("/group") {
                    ("404 Not Found", r#"{"error":"route is unsupported"}"#)
                } else {
                    panic!("unexpected proxy snapshot request: {request}");
                };
                paths.push(path);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            paths
        });

        let binding = CoreApiConnection {
            instance_id: "proxy-snapshot".into(),
            controller: CoreControllerInfo::Http(format!("http://{address}/")),
            secret: None,
        };
        let endpoint = Arc::new(MutableApiEndpoint::new(binding.clone()));
        let api = ApiClient::from_connection(binding, endpoint).unwrap();
        for _ in 0..2 {
            let snapshot = tokio::time::timeout(Duration::from_secs(5), api.proxy_snapshot())
                .await
                .expect("proxy snapshot should finish")
                .expect("controller snapshot should parse");
            assert_eq!(snapshot.proxies.len(), 2);
            assert!(snapshot.providers.is_empty());
            assert!(snapshot.groups.is_none());
        }
        let paths = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("controller should finish")
            .expect("mock controller should not panic");
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.starts_with("/group"))
                .count(),
            1,
            "a conclusive 404 must only probe the group route once"
        );
        assert_eq!(paths.len(), 5);
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
        assert!(
            matches!(
                error.downcast_ref::<chimera_core::actor_v2::api::ApiError>(),
                Some(chimera_core::actor_v2::api::ApiError::Stale)
            ),
            "expected a typed stale API capability error, got {error:#}"
        );
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
        assert!(
            matches!(
                error.downcast_ref::<chimera_core::actor_v2::api::ApiError>(),
                Some(chimera_core::actor_v2::api::ApiError::Stale)
            ),
            "expected a typed stale API capability error, got {error:#}"
        );
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
            std::task::Poll::<Option<clash_api::Result<serde_json::Value>>>::Pending
        });
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: 1,
            server: "127.0.0.1:1".to_owned(),
            secret: None,
        })
        .expect("API client should build");

        let receiver = forward_api_stream(
            api.inner.clone(),
            stream,
            serialize_api_sample::<serde_json::Value>,
        );
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
            std::task::Poll::<Option<clash_api::Result<serde_json::Value>>>::Ready(Some(Ok(
                serde_json::json!({ "index": index }),
            )))
        });
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: 1,
            server: "127.0.0.1:1".to_owned(),
            secret: None,
        })
        .expect("API client should build");

        let receiver = forward_api_stream(
            api.inner.clone(),
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

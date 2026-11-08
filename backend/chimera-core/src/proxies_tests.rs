use super::*;
use crate::actor_v2::api::{ApiChanges, CoreApiEndpoint};
use chimera_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DEADLINE: Duration = Duration::from_secs(5);

async fn controller(
    pinned: bool,
    fail_second: bool,
) -> (
    ApiClient,
    tokio::task::JoinHandle<Vec<String>>,
    std::net::SocketAddr,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test controller must bind");
    let address = listener.local_addr().unwrap();
    let endpoint = tokio::spawn(async move {
        let mut routes = Vec::new();
        // A snapshot needs proxies, providers and /group; then either one
        // group-delay or two per-node requests. Never replay an HTTP request.
        let expected = if pinned { 5 } else { 4 };
        for _ in 0..expected {
            let (mut stream, _) = tokio::time::timeout(DEADLINE, listener.accept())
                .await
                .expect("missing controller request")
                .unwrap();
            let mut bytes = [0_u8; 4096];
            let length = stream.read(&mut bytes).await.unwrap();
            let line = String::from_utf8_lossy(&bytes[..length])
                .lines()
                .next()
                .expect("request line")
                .to_owned();
            let fixed = if pinned { r#""node-a""# } else { "null" };
            let snapshot = format!(
                r#"{{"proxies":{{
                    "DIRECT":{{"name":"DIRECT","type":"Direct","history":[],"udp":false}},
                    "REJECT":{{"name":"REJECT","type":"Reject","history":[],"udp":false}},
                    "GLOBAL":{{"name":"GLOBAL","type":"Selector","history":[],"udp":false,"all":["Auto"]}},
                    "Auto":{{"name":"Auto","type":"URLTest","history":[],"udp":false,"all":["node-a","node-b"],"fixed":{fixed},"testUrl":"https://group.test/check","expectedStatus":"200/204"}},
                    "node-a":{{"name":"node-a","type":"Vless","history":[],"udp":false,"provider":"Subscription A"}},
                    "node-b":{{"name":"node-b","type":"Vless","history":[],"udp":false,"provider-name":"Subscription B"}}
                }}}}"#
            );
            let (status, body) = if line.starts_with("GET /proxies ") {
                ("200 OK", snapshot)
            } else if line.starts_with("GET /providers/proxies ") {
                ("200 OK", r#"{"providers":{}}"#.to_string())
            } else if line.starts_with("GET /group ") {
                (
                    "404 Not Found",
                    r#"{"message":"not supported"}"#.to_string(),
                )
            } else if line.starts_with("GET /group/Auto/delay?") {
                ("200 OK", r#"{"node-a":31,"node-b":42}"#.to_string())
            } else if line.contains("/healthcheck?") {
                if fail_second && line.contains("node-b/healthcheck") {
                    (
                        "503 Service Unavailable",
                        r#"{"message":"unreachable"}"#.to_string(),
                    )
                } else {
                    ("200 OK", r#"{"delay":37}"#.to_string())
                }
            } else {
                panic!("unexpected controller request: {line}");
            };
            routes.push(line);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        routes
    });
    let api = ApiClient::new_unbound(address.to_string(), None).unwrap();
    (api, endpoint, address)
}

#[tokio::test]
async fn unpinned_group_uses_only_group_delay_with_current_url_and_expected_status() {
    let (api, controller, _) = controller(false, false).await;
    let result = tokio::time::timeout(
        DEADLINE,
        test_group_delay(
            &api,
            &ProxyName::new("Auto"),
            "https://app.test/default",
            Duration::from_millis(3100),
        ),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(result[&ProxyName::new("node-a")], 31);
    assert_eq!(result[&ProxyName::new("node-b")], 42);
    let requests = controller.await.unwrap();
    assert_eq!(requests.len(), 4);
    let group = requests
        .iter()
        .find(|line| line.starts_with("GET /group/Auto/delay?"))
        .unwrap();
    assert!(
        group.contains("url=https%3A%2F%2Fgroup.test%2Fcheck"),
        "{group}"
    );
    assert!(group.contains("expected=200%2F204"), "{group}");
    assert!(group.contains("timeout=3100"), "{group}");
    assert_eq!(
        requests
            .iter()
            .filter(|line| line.contains("/delay?"))
            .count(),
        1
    );
}

#[tokio::test]
async fn pinned_group_uses_one_bound_client_for_member_provider_routes() {
    let (api, controller, _) = controller(true, true).await;
    let result = tokio::time::timeout(
        DEADLINE,
        test_group_delay(
            &api,
            &ProxyName::new("Auto"),
            "https://app.test/default",
            Duration::from_secs(2),
        ),
    )
    .await
    .unwrap()
    .unwrap();

    // A failed healthcheck is a zero, not an implicit retry.
    assert_eq!(result[&ProxyName::new("node-a")], 37);
    assert_eq!(result[&ProxyName::new("node-b")], 0);
    assert_eq!(
        result.keys().map(ProxyName::as_str).collect::<Vec<_>>(),
        ["node-a", "node-b"]
    );
    let requests = controller.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(
        !requests
            .iter()
            .any(|line| line.contains("/group/Auto/delay")),
        "the group delay endpoint can clear a pin"
    );
    assert!(
        requests
            .iter()
            .any(|line| line.contains("/providers/proxies/Subscription%20A/node-a/healthcheck?"))
    );
    assert!(
        requests
            .iter()
            .any(|line| line.contains("/providers/proxies/Subscription%20B/node-b/healthcheck?"))
    );
    for line in requests
        .iter()
        .filter(|line| line.contains("/healthcheck?"))
    {
        assert!(
            line.contains("url=https%3A%2F%2Fgroup.test%2Fcheck"),
            "{line}"
        );
        assert!(line.contains("expected=200%2F204"), "{line}");
        assert!(line.contains("timeout=2000"), "{line}");
    }
}

#[tokio::test]
async fn unavailable_controller_rejects_without_fabricating_delay_samples() {
    // No controller is listening. A missing snapshot must not be interpreted
    // as an unpinned group whose /group/delay is safe to invoke.
    let api = ApiClient::new_unbound("127.0.0.1:1".into(), None).unwrap();
    let result = tokio::time::timeout(
        DEADLINE,
        test_group_delay(
            &api,
            &ProxyName::new("Auto"),
            "https://app.test/default",
            Duration::from_secs(2),
        ),
    )
    .await
    .expect("the missing controller should not hang");
    assert!(result.is_err());
}

#[test]
fn cache_generation_rejects_old_profile_and_preserves_last_view() {
    let mut cache = ProxySnapshotState::new("original");
    assert_eq!(cache.generation(), 0);
    assert_eq!(
        cache.publish_if_generation(0, "profile-a", 10, 100),
        SnapshotPublish::Published
    );
    assert!(cache.is_fresh(102, 3));
    cache.invalidate();
    assert_eq!(cache.generation(), 1);
    assert!(!cache.is_fresh(102, 3));
    assert_eq!(cache.value(), &"profile-a");
    assert!(cache.ensure_generation(0).is_err());

    assert_eq!(
        cache.publish_if_generation(0, "obsolete-profile", 11, 103),
        SnapshotPublish::Stale
    );
    assert_eq!(cache.value(), &"profile-a");
    assert_eq!(cache.checksum(), None);

    assert_eq!(
        cache.publish_if_generation(1, "profile-b", 12, 104),
        SnapshotPublish::Published
    );
    assert_eq!(cache.value(), &"profile-b");
}

#[test]
fn unchanged_checksum_refreshes_freshness_without_republishing() {
    let mut cache = ProxySnapshotState::from_parts("original", 3, Some(50), 100);
    assert!(!cache.is_fresh(106, 3));
    assert_eq!(
        cache.publish_if_generation(3, "duplicate", 50, 106),
        SnapshotPublish::Unchanged
    );
    assert_eq!(cache.value(), &"original");
    assert!(cache.is_fresh(108, 3));
    assert_eq!(cache.generation(), 3);
    assert!(!cache.is_fresh(109, 2));
}

#[test]
fn proxy_cache_rejects_clock_rollback_and_empty_snapshots() {
    let mut empty = ProxySnapshotState::new(1);
    assert!(!empty.is_fresh(1, 3));
    empty.replace(2, 8, 100);
    assert!(!empty.is_fresh(99, 3), "future timestamps are not fresh");
    assert!(!empty.is_fresh(104, 3));
    assert!(empty.is_fresh(103, 3));
}

#[test]
fn late_success_cannot_overwrite_a_newer_snapshot_or_same_checksum_revision() {
    let mut cache = ProxySnapshotState::new("initial");
    cache.replace("original", 11, 100);
    let generation = cache.generation();
    let stale_revision = cache.revision();

    assert_eq!(
        cache.publish_if_generation_and_revision(generation, stale_revision, "latest", 12, 102,),
        SnapshotPublish::Published
    );
    assert_eq!(
        cache.publish_if_generation_and_revision(
            generation,
            stale_revision,
            "late previous",
            13,
            103,
        ),
        SnapshotPublish::Stale,
        "a slower GET must not roll back a newer controller observation"
    );
    assert_eq!(cache.value(), &"latest");
    assert_eq!(cache.checksum(), Some(12));
    assert!(cache.is_fresh(103, 3));

    // A fresh identical response also advances revision; the old request
    // cannot replace it with an outdated different value afterward.
    let identical_revision = cache.revision();
    assert_eq!(
        cache.publish_if_generation_and_revision(
            generation,
            identical_revision,
            "ignored identical",
            12,
            104,
        ),
        SnapshotPublish::Unchanged
    );
    assert_eq!(
        cache.publish_if_generation_and_revision(
            generation,
            identical_revision,
            "late wrong value",
            14,
            105,
        ),
        SnapshotPublish::Stale
    );
    assert_eq!(cache.value(), &"latest");
    assert!(cache.is_fresh(105, 3));
}

#[test]
fn failed_read_marks_only_its_own_revision_unverified() {
    let mut cache = ProxySnapshotState::new("initial");
    cache.replace("verified", 12, 100);
    let generation = cache.generation();
    let failed_read_revision = cache.revision();
    assert!(cache.is_fresh(101, 3));

    // An overlapping successful read, even with an unchanged checksum,
    // must win over the earlier failed request.
    assert_eq!(
        cache.publish_if_generation(generation, "same", 12, 102),
        SnapshotPublish::Unchanged
    );
    assert!(!cache.mark_unverified_if_revision(generation, failed_read_revision));
    assert!(cache.is_fresh(103, 3));

    let failed_read_revision = cache.revision();
    assert!(cache.mark_unverified_if_revision(generation, failed_read_revision));
    assert!(!cache.is_fresh(103, 3));
    assert_eq!(cache.value(), &"verified");
    assert!(!cache.mark_unverified_if_revision(generation, failed_read_revision));

    // A failed read from an old profile cannot stale a new profile.
    cache.invalidate();
    cache.replace("new profile", 23, 105);
    let revision = cache.revision();
    assert!(!cache.mark_unverified_if_revision(generation, revision));
    assert!(cache.is_fresh(105, 3));
    assert_eq!(cache.value(), &"new profile");
}

struct RetiringEndpoint {
    binding: Mutex<CoreApiConnection>,
    checks: AtomicUsize,
}

#[async_trait::async_trait]
impl CoreApiEndpoint for RetiringEndpoint {
    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
        // The proxy snapshot succeeds with the original instance. The first
        // per-member request sees the retired session and must not proceed.
        let call = self.checks.fetch_add(1, Ordering::SeqCst);
        Ok((call < 2).then(|| self.binding.lock().unwrap().clone()))
    }

    async fn api_changes(&self) -> anyhow::Result<Option<ApiChanges>> {
        Ok(None)
    }

    fn host_label(&self) -> String {
        "mock-group-delay-core".into()
    }
}

#[tokio::test]
async fn profile_switch_before_member_fanout_cannot_report_success_on_old_instance() {
    let (_, mock_server, address) = controller(true, false).await;
    let binding = CoreApiConnection {
        instance_id: "original-running-core".into(),
        controller: CoreControllerInfo::Http(format!("http://{address}/")),
        secret: None,
    };
    let endpoint = Arc::new(RetiringEndpoint {
        binding: Mutex::new(binding.clone()),
        checks: AtomicUsize::new(0),
    });
    let api = ApiClient::from_connection(binding, endpoint.clone()).unwrap();

    let result = tokio::time::timeout(
        DEADLINE,
        test_group_delay(
            &api,
            &ProxyName::new("Auto"),
            "https://app.test/default",
            Duration::from_secs(2),
        ),
    )
    .await
    .expect("profile transition must not hang group testing");

    assert!(
        result.is_err(),
        "a retired source instance must not publish success"
    );
    assert!(
        endpoint.checks.load(Ordering::SeqCst) >= 3,
        "the snapshot and member operation each check authority"
    );
    assert!(
        matches!(api.check_binding().await, Err(ApiError::Stale)),
        "old instance capability must remain revoked"
    );
    // The mock expects additional member requests that must never happen
    // after the authority check; stop its listener without awaiting a hang.
    mock_server.abort();
    let _ = mock_server.await;
}

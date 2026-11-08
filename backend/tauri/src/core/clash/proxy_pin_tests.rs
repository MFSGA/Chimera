//! Mock-controller guard for pinned automatic group mutations.
//! A successful DELETE and a rejected DELETE both trigger a true controller
//! reread. There is never an optimistic local fixed-state change.
use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEST_DEADLINE: Duration = Duration::from_secs(5);

fn guard() -> Arc<RwLock<ProxiesGuard>> {
    let (sender, _) = broadcast::channel(1);
    Arc::new(RwLock::new(ProxiesGuard::new(sender)))
}

async fn exercise_unpin(status: &str, success: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let response_status = status.to_owned();
    let server = tokio::spawn(async move {
        let mut paths = Vec::new();
        for _ in 0..4 {
            let (mut stream, _) = tokio::time::timeout(TEST_DEADLINE, listener.accept())
                .await
                .expect("expected DELETE and three snapshot reads")
                .unwrap();
            let mut buffer = [0u8; 8192];
            let size = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..size]);
            let line = request.lines().next().unwrap().to_owned();
            let (code, body) = if line.starts_with("DELETE /proxies/Auto ") {
                if response_status == "204 No Content" {
                    ("204 No Content", "")
                } else {
                    ("409 Conflict", r#"{"message":"cannot clear fixed"}"#)
                }
            } else if line.starts_with("GET /proxies ") {
                (
                    "200 OK",
                    r#"{"proxies":{"DIRECT":{"name":"DIRECT","type":"Direct","history":[],"udp":false},"REJECT":{"name":"REJECT","type":"Reject","history":[],"udp":false},"GLOBAL":{"name":"GLOBAL","type":"Selector","history":[],"udp":false,"all":["Auto"]},"Auto":{"name":"Auto","type":"URLTest","history":[],"udp":false,"all":["DIRECT"],"now":"DIRECT","fixed":null}}}"#,
                )
            } else if line.starts_with("GET /providers/proxies ") {
                ("200 OK", r#"{"providers":{}}"#)
            } else if line.starts_with("GET /group ") {
                ("404 Not Found", r#"{"message":"not supported"}"#)
            } else {
                panic!("unexpected controller route: {line}");
            };
            paths.push(line);
            let response = format!(
                "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        paths
    });

    let guard = guard();
    let generation = guard.read().generation();
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: address.port(),
        server: address.to_string(),
        secret: None,
    })
    .unwrap();

    let result = tokio::time::timeout(
        TEST_DEADLINE,
        (&guard).clear_proxy_fixed(&api, generation, "Auto"),
    )
    .await
    .expect("unpin must complete");
    assert_eq!(result.is_ok(), success, "{result:?}");
    assert_eq!(guard.read().generation(), generation);
    assert_eq!(guard.read().inner().groups[0].name, "Auto");
    let paths = tokio::time::timeout(TEST_DEADLINE, server)
        .await
        .expect("mock should finish")
        .unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(
        paths
            .iter()
            .filter(|line| line.starts_with("DELETE "))
            .count(),
        1,
        "a lost or rejected mutation must never be replayed"
    );
    assert!(paths.iter().any(|line| line.starts_with("GET /proxies ")));
    if !success {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("clear_proxy_selection"),
            "the original controller refusal should be preserved"
        );
    }
}

#[tokio::test]
async fn unpin_refreshes_controller_snapshot_after_success() {
    exercise_unpin("204 No Content", true).await;
}

#[tokio::test]
async fn rejected_unpin_refreshes_and_preserves_original_error() {
    exercise_unpin("409 Conflict", false).await;
}

#[tokio::test]
async fn rejected_selection_rereads_actual_route_without_replaying_the_put() {
    // Simulate a controller which applied the selection but then returned a
    // failure: the operation must return the PUT error AND publish the real
    // route obtained from a fresh GET, not keep a falsely fresh old route.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut paths = Vec::new();
        for _ in 0..4 {
            let (mut stream, _) = tokio::time::timeout(TEST_DEADLINE, listener.accept())
                .await
                .expect("expected PUT and three snapshot reads")
                .unwrap();
            let mut buffer = [0u8; 8192];
            let size = stream.read(&mut buffer).await.unwrap();
            let line = String::from_utf8_lossy(&buffer[..size])
                .lines()
                .next()
                .expect("request line")
                .to_owned();
            let (status, body) = if line.starts_with("PUT /proxies/Auto ") {
                ("409 Conflict", r#"{"message":"selection outcome unknown"}"#)
            } else if line.starts_with("GET /proxies ") {
                (
                    "200 OK",
                    r#"{"proxies":{"DIRECT":{"name":"DIRECT","type":"Direct","history":[],"udp":false},"REJECT":{"name":"REJECT","type":"Reject","history":[],"udp":false},"GLOBAL":{"name":"GLOBAL","type":"Selector","history":[],"udp":false,"all":["Auto"]},"Auto":{"name":"Auto","type":"Selector","history":[],"udp":false,"all":["DIRECT","REJECT"],"now":"DIRECT"}}}"#,
                )
            } else if line.starts_with("GET /providers/proxies ") {
                ("200 OK", r#"{"providers":{}}"#)
            } else if line.starts_with("GET /group ") {
                ("404 Not Found", r#"{"message":"not supported"}"#)
            } else {
                panic!("unexpected controller route: {line}");
            };
            paths.push(line);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        paths
    });

    let (sender, mut updates) = broadcast::channel(1);
    let mut previous = Proxies::default();
    previous.groups.push(ProxyGroupItem {
        name: "Auto".into(),
        now: Some("REJECT".into()),
        ..Default::default()
    });
    let guard = Arc::new(RwLock::new(ProxiesGuard::test_guard(
        previous,
        Some(42),
        chrono::Utc::now().timestamp() as u64,
        0,
        sender,
    )));
    let generation = guard.read().generation();
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: address.port(),
        server: address.to_string(),
        secret: None,
    })
    .unwrap();

    let error = tokio::time::timeout(
        TEST_DEADLINE,
        (&guard).select_proxy(&api, generation, "Auto", "DIRECT"),
    )
    .await
    .expect("selection must complete")
    .expect_err("the controller's rejected PUT must stay an error");
    assert!(
        error.to_string().contains("409") || error.to_string().contains("update_proxy"),
        "original PUT failure must be preserved: {error:#}"
    );
    assert_eq!(guard.read().generation(), generation);
    assert_eq!(
        guard.read().inner().groups[0].now.as_deref(),
        Some("DIRECT")
    );
    assert!(
        guard.read().is_updated(),
        "successful GET should refresh the cache"
    );
    updates
        .try_recv()
        .expect("the refreshed controller route must notify the UI subscriber");

    let paths = tokio::time::timeout(TEST_DEADLINE, server)
        .await
        .expect("mock server must finish")
        .unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(
        paths.iter().filter(|line| line.starts_with("PUT ")).count(),
        1
    );
    assert_eq!(
        paths.iter().filter(|line| line.starts_with("GET ")).count(),
        3
    );
}

#[tokio::test]
async fn failed_selection_reread_invalidates_freshness_but_preserves_last_view() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let mut paths = Vec::new();
        // Legacy fetch retries reads and can issue independent provider/group
        // GETs. Serve all of them until the mutation finishes; never replay PUT.
        loop {
            let (mut stream, _) = tokio::select! {
                accepted = listener.accept() => accepted.unwrap(),
                _ = &mut shutdown_rx => break,
            };
            let mut buffer = [0u8; 8192];
            let size = stream.read(&mut buffer).await.unwrap();
            let line = String::from_utf8_lossy(&buffer[..size])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            let (status, body) = if line.starts_with("PUT /proxies/Auto ") {
                ("409 Conflict", r#"{"message":"outcome unknown"}"#)
            } else if line.starts_with("GET ") {
                ("503 Service Unavailable", r#"{"message":"unavailable"}"#)
            } else {
                panic!("unexpected route: {line}");
            };
            paths.push(line);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        paths
    });

    let (sender, mut updates) = broadcast::channel(1);
    let mut previous = Proxies::default();
    previous.groups.push(ProxyGroupItem {
        name: "Auto".into(),
        now: Some("REJECT".into()),
        ..Default::default()
    });
    let guard = Arc::new(RwLock::new(ProxiesGuard::test_guard(
        previous,
        Some(42),
        chrono::Utc::now().timestamp() as u64,
        0,
        sender,
    )));
    let generation = guard.read().generation();
    assert!(guard.read().is_updated());
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: address.port(),
        server: address.to_string(),
        secret: None,
    })
    .unwrap();
    let error = tokio::time::timeout(
        TEST_DEADLINE,
        (&guard).select_proxy(&api, generation, "Auto", "DIRECT"),
    )
    .await
    .expect("mutation must complete")
    .expect_err("original rejected PUT remains an error");
    assert!(
        error.to_string().contains("select_proxy") && error.to_string().contains("409"),
        "original mutation error must survive the failed GET: {error:#}"
    );
    assert_eq!(guard.read().generation(), generation);
    assert!(
        !guard.read().is_updated(),
        "failed GET must retire freshness"
    );
    assert_eq!(
        guard.read().inner().groups[0].now.as_deref(),
        Some("REJECT")
    );
    updates
        .try_recv()
        .expect("subscribers must be notified that cached state is unverified");
    shutdown_tx
        .send(())
        .expect("server must still be listening");
    let paths = tokio::time::timeout(TEST_DEADLINE, server)
        .await
        .expect("mock server should finish")
        .unwrap();
    assert_eq!(paths.iter().filter(|p| p.starts_with("PUT ")).count(), 1);
    assert!(
        paths.iter().any(|p| p.starts_with("GET /proxies ")),
        "controller truth must be queried after the failed PUT"
    );
}

#[tokio::test]
async fn rejected_provider_update_rereads_controller_and_preserves_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut routes = Vec::new();
        for _ in 0..4 {
            let (mut stream, _) = tokio::time::timeout(TEST_DEADLINE, listener.accept())
                .await
                .expect("provider PUT plus snapshot GETs")
                .unwrap();
            let mut buffer = [0u8; 8192];
            let size = stream.read(&mut buffer).await.unwrap();
            let line = String::from_utf8_lossy(&buffer[..size])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            let (status, body) = if line.starts_with("PUT /providers/proxies/Subscription ") {
                ("409 Conflict", r#"{"message":"provider already updated"}"#)
            } else if line.starts_with("GET /proxies ") {
                (
                    "200 OK",
                    r#"{"proxies":{"DIRECT":{"name":"DIRECT","type":"Direct","history":[],"udp":false},"REJECT":{"name":"REJECT","type":"Reject","history":[],"udp":false},"GLOBAL":{"name":"GLOBAL","type":"Selector","history":[],"udp":false,"all":["Auto"]},"Auto":{"name":"Auto","type":"Selector","history":[],"udp":false,"all":["DIRECT"],"now":"DIRECT"}}}"#,
                )
            } else if line.starts_with("GET /providers/proxies ") {
                ("200 OK", r#"{"providers":{}}"#)
            } else if line.starts_with("GET /group ") {
                ("404 Not Found", r#"{"message":"not supported"}"#)
            } else {
                panic!("unexpected route: {line}");
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
    let (sender, mut changes) = broadcast::channel(2);
    let guard = Arc::new(RwLock::new(ProxiesGuard::new(sender)));
    let generation = guard.read().generation();
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: address.port(),
        server: address.to_string(),
        secret: None,
    })
    .unwrap();
    let error = tokio::time::timeout(
        TEST_DEADLINE,
        (&guard).update_provider(&api, generation, "Subscription"),
    )
    .await
    .expect("provider update should complete")
    .unwrap_err();
    assert!(
        error.to_string().contains("409") || error.to_string().contains("update_proxy_provider"),
        "{error:#}"
    );
    assert_eq!(guard.read().inner().groups[0].name, "Auto");
    changes
        .try_recv()
        .expect("provider refresh should notify subscribers");
    let routes = tokio::time::timeout(TEST_DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        routes
            .iter()
            .filter(|route| route.starts_with("PUT "))
            .count(),
        1
    );
    assert_eq!(
        routes
            .iter()
            .filter(|route| route.starts_with("GET "))
            .count(),
        3
    );
}

#[tokio::test]
async fn stale_proxy_generation_rejects_provider_update_before_any_http_request() {
    let guard = guard();
    let previous = guard.read().generation();
    guard.write().invalidate();
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: 1,
        server: "127.0.0.1:1".into(),
        secret: None,
    })
    .unwrap();
    let error = (&guard)
        .update_provider(&api, previous, "Subscription")
        .await
        .expect_err("stale generation cannot dispatch provider update");
    assert!(error.to_string().contains("profile change"));
}

#[tokio::test]
async fn stale_proxy_generation_rejects_unpin_before_any_http_request() {
    let guard = guard();
    let previous = guard.read().generation();
    guard.write().invalidate();
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: 1,
        server: "127.0.0.1:1".into(),
        secret: None,
    })
    .unwrap();

    let error = (&guard)
        .clear_proxy_fixed(&api, previous, "Auto")
        .await
        .expect_err("stale generation cannot dispatch a mutation");
    assert!(error.to_string().contains("profile change"));
}

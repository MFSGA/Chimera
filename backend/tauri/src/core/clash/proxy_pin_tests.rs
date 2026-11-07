//! Mock-controller guard for pinned automatic group mutations.
//! A successful DELETE and a rejected DELETE both trigger a true controller
//! reread. There is never an optimistic local fixed-state change.
use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEST_DEADLINE: Duration = Duration::from_secs(5);

fn guard() -> Arc<RwLock<ProxiesGuard>> {
    let (sender, _) = broadcast::channel(1);
    Arc::new(RwLock::new(ProxiesGuard {
        inner: Proxies::default(),
        checksum: None,
        updated_at: 0,
        generation: 0,
        sender,
    }))
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

//! Core-owned connection interruption after proxy, profile and mode changes.
//! Matches ref's `nyanpasu-core/src/connections.rs` ownership: the
//! application supplies a scope, never a Tauri window or runtime handle.

use anyhow::Result;

use crate::{actor_v2::api::ApiClient, clash::api::ConnectionItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionScope {
    All,
    ProxyGroup { name: String },
}

/// Close the selected scope using the already instance-bound Clash API
/// capability. Do not retry deletes: a lost reply does not prove the
/// connection is still open.
pub async fn interrupt_connections(api: &ApiClient, scope: &ConnectionScope) -> Result<()> {
    match scope {
        ConnectionScope::All => Ok(api.delete_connections(None).await?),
        ConnectionScope::ProxyGroup { name } => interrupt_by_chains(api, &[name.as_str()]).await,
    }
}

/// Chimera's existing multi-group compatibility entrypoint. The model still
/// lives in the independent core; the Tauri adapter supplies only policy.
pub async fn interrupt_by_chains(api: &ApiClient, chains: &[&str]) -> Result<()> {
    if chains.is_empty() {
        return Ok(());
    }
    let connections = api.get_connections().await?.connections;
    for id in connection_ids_for_chain(&connections, chains) {
        // Like ref, close only connections whose chain matches exactly.
        // Sequential operations preserve the first failure without replaying
        // a successful (possibly already closed) connection delete.
        api.delete_connections(Some(&id)).await?;
    }
    Ok(())
}

/// One close request per connection, even when multiple requested chains
/// appear in its path. Only exact group names match.
pub fn connection_ids_for_chain(connections: &[ConnectionItem], chains: &[&str]) -> Vec<String> {
    connections
        .iter()
        .filter(|connection| {
            connection
                .chains
                .iter()
                .any(|name| chains.iter().any(|expected| name == expected))
        })
        .map(|connection| connection.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(id: &str, chains: &[&str]) -> ConnectionItem {
        ConnectionItem {
            id: id.into(),
            chains: chains.iter().map(|s| (*s).into()).collect(),
        }
    }

    #[tokio::test]
    async fn proxy_group_scope_closes_only_matching_controller_connections() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local mock controller should bind");
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0_u8; 8192];
                let length = socket.read(&mut bytes).await.unwrap();
                let line = String::from_utf8_lossy(&bytes[..length]);
                let request = line.lines().next().expect("request line").to_owned();
                let (status, body) = if request.starts_with("GET /connections ") {
                    (
                        "200 OK",
                        serde_json::json!({
                            "downloadTotal": 0,
                            "uploadTotal": 0,
                            "connections": [
                                {
                                    "id": "11111111-1111-1111-1111-111111111111",
                                    "metadata": null,
                                    "download": 100,
                                    "upload": 20,
                                    "start": "2024-01-01T00:00:00Z",
                                    "chains": ["Auto", "GLOBAL"],
                                    "rule": "MATCH",
                                    "rulePayload": ""
                                },
                                {
                                    "id": "22222222-2222-2222-2222-222222222222",
                                    "metadata": null,
                                    "download": 20,
                                    "upload": 10,
                                    "start": "2024-01-01T00:00:00Z",
                                    "chains": ["Auto Backup", "GLOBAL"],
                                    "rule": "MATCH",
                                    "rulePayload": ""
                                },
                                {
                                    "id": "33333333-3333-3333-3333-333333333333",
                                    "metadata": null,
                                    "download": 40,
                                    "upload": 10,
                                    "start": "2024-01-01T00:00:00Z",
                                    "chains": ["Auto", "GLOBAL"],
                                    "rule": "MATCH",
                                    "rulePayload": ""
                                },
                            ]
                        })
                        .to_string(),
                    )
                } else {
                    assert!(request.starts_with("DELETE /connections/"), "{request}");
                    ("204 No Content", String::new())
                };
                requests.push(request);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        let client = ApiClient::new_unbound(address.to_string(), None).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            interrupt_connections(
                &client,
                &ConnectionScope::ProxyGroup {
                    name: "Auto".into(),
                },
            ),
        )
        .await
        .expect("scope close should complete")
        .expect("matching deletes should succeed");
        let requests = tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .expect("mock server should finish")
            .expect("mock server should not panic");
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /connections "));
        assert!(requests[1].contains("11111111-1111-1111-1111-111111111111"));
        assert!(requests[2].contains("33333333-3333-3333-3333-333333333333"));
        assert!(
            !requests.iter().any(|request| request.contains("22222222")),
            "similarly named group must not have its connection closed"
        );
    }

    #[tokio::test]
    async fn all_scope_closes_without_reading_connection_list() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local mock controller should bind");
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0_u8; 4096];
            let length = socket.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..length])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            socket
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            request
        });
        let client = ApiClient::new_unbound(address.to_string(), None).unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            interrupt_connections(&client, &ConnectionScope::All),
        )
        .await
        .expect("all scope should complete")
        .expect("delete should succeed");
        let request = tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        assert!(request.starts_with("DELETE /connections "), "{request}");
    }

    #[test]
    fn chain_filter_only_matches_exact_names() {
        let list = vec![
            connection("a", &["Node A", "Auto", "GLOBAL"]),
            connection("b", &["Node B", "Auto Backup", "GLOBAL"]),
            connection("c", &["DIRECT"]),
        ];
        assert_eq!(connection_ids_for_chain(&list, &["Auto"]), ["a"]);
    }

    #[test]
    fn chain_filter_matches_any_scope_only_once() {
        let list = vec![
            connection("a", &["Auto", "GLOBAL"]),
            connection("b", &["Auto Backup", "GLOBAL"]),
            connection("c", &["Fallback", "GLOBAL"]),
        ];
        assert_eq!(
            connection_ids_for_chain(&list, &["Auto", "GLOBAL", "Fallback"]),
            ["a", "b", "c"]
        );
        assert!(connection_ids_for_chain(&list, &[]).is_empty());
    }
}

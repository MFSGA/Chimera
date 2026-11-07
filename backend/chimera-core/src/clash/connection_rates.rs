//! Core-owned derivation of per-connection transfer rates from successive
//! controller frames. The WebSocket actor and Tauri Channel remain app-owned.
//! This is intentionally pure and mirrors ref's clash/connection_rates.rs.

use std::{collections::HashMap, time::Instant};

use serde::Serialize;
use specta::Type;

/// One core connection plus its derived rates, as in ref's detail contract.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnection {
    #[serde(flatten)]
    pub connection: clash_api::Connection,
    pub download_speed: u64,
    pub upload_speed: u64,
}

/// Latest full connection details, produced only while a UI subscriber exists.
/// A single latest frame (not an unbounded event queue) follows ref's watch semantics.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionDetails {
    pub sequence: u64,
    pub connections: Vec<ClashConnection>,
}

#[derive(Default)]
pub struct DetailCounters {
    at: Option<Instant>,
    connections: HashMap<String, (i64, i64)>,
}

/// Pure per-connection byte/sec derivation: time and the previous sample's
/// counters are explicit, as in ref. Preserve the core model's extra fields.
pub fn derive_connection_details(
    counters: &mut DetailCounters,
    rows: &[serde_json::Value],
    at: Instant,
) -> Vec<ClashConnection> {
    let elapsed = counters
        .at
        .and_then(|last| at.checked_duration_since(last))
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default();
    let mut next = HashMap::with_capacity(rows.len());
    let details = rows
        .iter()
        .filter_map(|raw| {
            let connection: clash_api::Connection = match serde_json::from_value(raw.clone()) {
                Ok(value) => value,
                Err(error) => {
                    tracing::debug!(%error, "skipping malformed connection detail");
                    return None;
                }
            };
            let rate = |previous: i64, current: i64| -> u64 {
                if elapsed <= 0.0 || current <= previous {
                    return 0;
                }
                ((current.saturating_sub(previous)) as u64 as f64 / elapsed) as u64
            };
            let (download_speed, upload_speed) = counters
                .connections
                .get(&connection.id.to_string())
                .map(|(before_down, before_up)| {
                    (
                        rate(*before_down, connection.download),
                        rate(*before_up, connection.upload),
                    )
                })
                .unwrap_or_default();
            next.insert(
                connection.id.to_string(),
                (connection.download, connection.upload),
            );
            Some(ClashConnection {
                connection,
                download_speed,
                upload_speed,
            })
        })
        .collect();
    counters.at = Some(at);
    counters.connections = next;
    details
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn connection(download: i64, upload: i64) -> serde_json::Value {
        serde_json::json!({
            "id": "11111111-1111-1111-1111-111111111111",
            "download": download,
            "upload": upload,
            "metadata": { "host": "example.test", "unknownGeo": {"region": "test"} },
            "customTag": "retain-me",
            "chains": ["DIRECT"],
            "start": "2024-01-01T00:00:00+00:00",
            "rule": "MATCH",
            "rulePayload": "",
        })
    }

    #[test]
    fn core_rate_derivation_keeps_protocol_fields_and_scales_by_elapsed_time() {
        let now = Instant::now();
        let mut counters = DetailCounters::default();

        let first = derive_connection_details(&mut counters, &[connection(100, 50)], now);
        assert_eq!(first[0].download_speed, 0);
        assert_eq!(first[0].upload_speed, 0);

        let next = derive_connection_details(
            &mut counters,
            &[connection(700, 250)],
            now + Duration::from_millis(500),
        );
        assert_eq!(next[0].download_speed, 1200);
        assert_eq!(next[0].upload_speed, 400);
        let wire = serde_json::to_value(&next[0]).unwrap();
        assert_eq!(wire["_extra"]["customTag"], "retain-me");
        assert_eq!(wire["metadata"]["_extra"]["unknownGeo"]["region"], "test");
    }

    #[test]
    fn stale_connections_do_not_leak_counters_across_disconnection() {
        let now = Instant::now();
        let mut counters = DetailCounters::default();
        derive_connection_details(&mut counters, &[connection(900, 100)], now);
        derive_connection_details(&mut counters, &[], now + Duration::from_secs(1));
        let new_session = derive_connection_details(
            &mut counters,
            &[connection(10, 5)],
            now + Duration::from_secs(2),
        );
        assert_eq!(new_session[0].download_speed, 0);
        assert_eq!(new_session[0].upload_speed, 0);
    }

    #[test]
    fn malformed_rows_do_not_block_valid_connection_updates() {
        let now = Instant::now();
        let mut counters = DetailCounters::default();
        let results = derive_connection_details(
            &mut counters,
            &[serde_json::json!({"invalid": true}), connection(42, 8)],
            now,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].connection.download, 42);
    }
}

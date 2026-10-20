//! Converts the raw Clash connections feed into accounting frames.
use std::time::Duration;

use super::{LocalSourceIps, geo::CountryLookup};
use chimera_clash_api::{ConfigEnum, Connection, ConnectionNetwork};
use chimera_traffic::{Bytes, Dimensions, Frame, RuleKey, Sample};

use super::geo;
use crate::core::clash::ws::ClashConnectionsFrame;

const UNKNOWN: &str = "unknown";

pub(crate) fn frame_from_snapshot(
    frame: &ClashConnectionsFrame,
    wall_ms: i64,
    mono: Duration,
    index: Option<&dyn CountryLookup>,
    local_source: LocalSourceIps,
) -> Frame {
    let snapshot = &frame.snapshot;
    let connections = snapshot
        .connections
        .iter()
        .flatten()
        .filter_map(
            |raw| match serde_json::from_value::<Connection>(raw.clone()) {
                Ok(connection) => Some(sample(connection, index, local_source)),
                Err(error) => {
                    tracing::debug!(%error, "skipping malformed Clash connection in traffic feed");
                    None
                }
            },
        )
        .collect();

    Frame {
        instance_id: frame.instance_id.clone(),
        wall_ms,
        mono,
        totals: Bytes {
            upload: snapshot.upload_total,
            download: snapshot.download_total,
        },
        connections,
    }
}

fn sample(
    connection: Connection,
    index: Option<&dyn CountryLookup>,
    local_source: LocalSourceIps,
) -> Sample {
    let metadata = connection.metadata.as_ref();
    let process = metadata
        .and_then(|meta| text(meta.process_path.as_ref()))
        .or_else(|| metadata.and_then(|meta| text(meta.process.as_ref())));
    let target = metadata
        .and_then(|meta| text(meta.host.as_ref()))
        .or_else(|| metadata.and_then(|meta| text(meta.destination_ip.as_ref())));
    let inbound = metadata
        .and_then(|meta| text(meta.inbound_user.as_ref()))
        .or_else(|| metadata.and_then(|meta| text(meta.inbound_name.as_ref())));
    let source_region = metadata.map_or_else(
        || UNKNOWN.to_owned(),
        |meta| geo::locate_source(meta, index, local_source),
    );
    let (destination_region, destination_basis) = metadata.map_or_else(
        || (UNKNOWN.to_owned(), None),
        |meta| geo::locate_destination(meta, connection.chains.first().map(String::as_str), index),
    );

    Sample {
        id: connection.id.to_string(),
        started_at: connection.start.timestamp_millis(),
        counters: counters(connection.upload, connection.download),
        dimensions: Dimensions {
            process: process.unwrap_or(UNKNOWN).replace('\\', "/"),
            source: metadata
                .and_then(|meta| text(meta.source_ip.as_ref()))
                .unwrap_or(UNKNOWN)
                .to_owned(),
            target: target.unwrap_or(UNKNOWN).to_owned(),
            protocol: metadata
                .and_then(|meta| meta.network.as_ref())
                .map(network)
                .filter(|value| !value.is_empty())
                .unwrap_or(UNKNOWN)
                .to_owned(),
            rule: RuleKey {
                kind: connection.rule,
                payload: connection.rule_payload,
            },
            chains: connection.chains,
            inbound: inbound.unwrap_or(UNKNOWN).to_owned(),
            profile: None,
            source_region,
            destination_region,
            destination_basis,
        },
    }
}

/// The core never reports negative counters; clamp rather than trust it.
fn counters(upload: i64, download: i64) -> Bytes {
    Bytes {
        upload: u64::try_from(upload).unwrap_or(0),
        download: u64::try_from(download).unwrap_or(0),
    }
}

fn text(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|value| !value.is_empty())
}

fn network(network: &ConfigEnum<ConnectionNetwork>) -> &str {
    match network {
        ConfigEnum::Known(ConnectionNetwork::Tcp) => "tcp",
        ConfigEnum::Known(ConnectionNetwork::Udp) => "udp",
        ConfigEnum::Known(ConnectionNetwork::All) => "all",
        ConfigEnum::Known(ConnectionNetwork::Invalid) => "invalid",
        ConfigEnum::Unknown(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        clash::ws::{ClashConnectionsFrame, ClashWsConnectionSnapshot},
        traffic::geo::tests::Countries,
    };

    #[test]
    fn frame_uses_the_core_country_index_for_each_connection() {
        let countries = Countries::of(&[("203.0.113.7", "JP"), ("8.8.8.8", "US")]);
        let frame = ClashConnectionsFrame {
            instance_id: "core-1".into(),
            snapshot: ClashWsConnectionSnapshot {
                download_total: 0,
                upload_total: 0,
                download_speed: 0,
                upload_speed: 0,
                memory: None,
                connections: Some(vec![serde_json::json!({
                    "id": "00000000-0000-0000-0000-000000000001",
                    "metadata": {
                        "network": "tcp",
                        "host": "example.com",
                        "sourceIP": "203.0.113.7",
                        "sourceGeoIP": ["cn"],
                        "destinationIP": "8.8.8.8",
                        "destinationGeoIP": ["cn"],
                    },
                    "upload": 0,
                    "download": 0,
                    "start": "2026-10-19T00:00:00Z",
                    "chains": ["Proxy"],
                    "rule": "MATCH",
                    "rulePayload": "Proxy"
                })]),
            },
        };

        let recorded = frame_from_snapshot(
            &frame,
            0,
            Duration::ZERO,
            Some(&countries),
            LocalSourceIps::default(),
        );

        assert_eq!(recorded.connections.len(), 1);
        assert_eq!(recorded.connections[0].dimensions.source_region, "JP");
        assert_eq!(recorded.connections[0].dimensions.destination_region, "US");
    }
}

//! Public port resolution failures.

use chimera_config::clash::config::clash_strategy::port::PickPortError;
use serde::Serialize;
use snafu::Snafu;

/// The port a resolution was picking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PortField {
    Mixed,
    Http,
    Socks,
    Redir,
    Tproxy,
    ExternalController,
}

/// A failure of resolving the ports a candidate runtime would bind.
#[derive(Debug, Snafu, Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum PortResolveError {
    #[snafu(display("failed to resolve the {field:?} port: {source}"))]
    ResolvePort {
        field: PortField,
        source: PickPortError,
    },
    #[snafu(display("the {first:?} and {second:?} listeners both use port {port}"))]
    PortConflict {
        first: PortField,
        second: PortField,
        port: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_conflict_preserves_upstream_fields_and_wire_shape() {
        let error = PortResolveError::PortConflict {
            first: PortField::Mixed,
            second: PortField::Redir,
            port: 7890,
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "port_conflict",
                "first": "mixed",
                "second": "redir",
                "port": 7890,
            })
        );
        assert!(matches!(
            serde_json::from_value::<PortResolveError>(serde_json::to_value(&error).unwrap())
                .unwrap(),
            PortResolveError::PortConflict {
                first: PortField::Mixed,
                second: PortField::Redir,
                port: 7890
            }
        ));
    }

    #[test]
    fn typed_port_failure_keeps_source_error_not_a_string() {
        let error = PortResolveError::ResolvePort {
            field: PortField::Socks,
            source: PickPortError::PortNotAvailable { port: 1080 },
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "resolve_port",
                "field": "socks",
                "source": {"kind": "port_not_available", "port": 1080},
            })
        );
        assert!(matches!(
            serde_json::from_value::<PortResolveError>(serde_json::to_value(&error).unwrap())
                .unwrap(),
            PortResolveError::ResolvePort {
                field: PortField::Socks,
                source: PickPortError::PortNotAvailable { port: 1080 },
            }
        ));
    }
}

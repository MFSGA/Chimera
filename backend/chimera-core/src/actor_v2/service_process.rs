//! Typed OS service-process control contract, adapted from
//! ref/backend/nyanpasu-contracts/src/service.rs.
//! The core owns command/error semantics; Tauri supplies the elevated host
//! executor and never manufactures a stopped state from a failed probe.

use std::{fmt, io, string::FromUtf8Error};

use async_trait::async_trait;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCommand {
    Install,
    Update,
    Uninstall,
    Start,
    Stop,
    Restart,
}

impl fmt::Display for ServiceCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        })
    }
}

#[derive(Debug, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceCommandError {
    #[error("could not resolve service command arguments: {message}")]
    ResolveServiceDirs { message: String },
    #[error("could not run the service {command} command: {source}")]
    RunElevated {
        command: ServiceCommand,
        #[source]
        #[serde(skip)]
        #[specta(skip)]
        source: io::Error,
    },
    #[error("the service {command} command failed, exit code: {exit_code:?}, signal: {signal:?}")]
    ServiceCommandExit {
        command: ServiceCommand,
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    #[error("could not query the service status: {source}")]
    RunServiceStatus {
        #[source]
        #[serde(skip)]
        #[specta(skip)]
        source: io::Error,
    },
    #[error("the service status query failed, exit code: {exit_code:?}, signal: {signal:?}")]
    ServiceStatusExit {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    #[error("the service status is not valid UTF-8")]
    ServiceStatusNotUtf8 {
        #[source]
        #[serde(skip)]
        #[specta(skip)]
        source: FromUtf8Error,
    },
    #[error("could not parse the service status: {source}")]
    ParseServiceStatus {
        #[source]
        #[serde(skip)]
        #[specta(skip)]
        source: serde_json::Error,
    },
    #[error("timed out after {limit_ms} ms")]
    TimedOut { limit_ms: u64 },
    #[error("timed out after {limit_ms} ms; the command is still running")]
    StillRunning { limit_ms: u64 },
    #[error("the command's task was cancelled")]
    TaskCancelled,
}

/// Upstream's narrow boundary between the ServiceActor and its elevated OS
/// executor. An unsuccessful `probe` remains an error, not a false "stopped"
/// observation. Chimera currently offers a native restart CLI command as well.
#[async_trait]
pub trait ServiceProcessControl: Send + Sync + 'static {
    /// Unparsed UTF-8 output after the host checked the command's exit code.
    async fn probe(&self) -> Result<String, ServiceCommandError>;
    async fn install(&self) -> Result<(), ServiceCommandError>;
    async fn uninstall(&self) -> Result<(), ServiceCommandError>;
    async fn start_daemon(&self) -> Result<(), ServiceCommandError>;
    async fn stop_daemon(&self) -> Result<(), ServiceCommandError>;
    async fn update(&self) -> Result<(), ServiceCommandError>;
    async fn restart_daemon(&self) -> Result<(), ServiceCommandError>;
}

/// The core, not the Tauri host adapter, owns JSON status decoding.
pub fn parse_service_status(
    raw: &str,
) -> Result<chimera_ipc::types::StatusInfo<'static>, ServiceCommandError> {
    serde_json::from_str(raw).map_err(|source| ServiceCommandError::ParseServiceStatus { source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_typed_and_stable() {
        for (command, expected) in [
            (ServiceCommand::Install, "install"),
            (ServiceCommand::Update, "update"),
            (ServiceCommand::Uninstall, "uninstall"),
            (ServiceCommand::Start, "start"),
            (ServiceCommand::Stop, "stop"),
            (ServiceCommand::Restart, "restart"),
        ] {
            assert_eq!(command.to_string(), expected);
            assert_eq!(serde_json::to_value(command).unwrap(), expected);
        }
    }

    #[test]
    fn typed_failure_is_not_inferred_from_untrusted_text() {
        let exit = ServiceCommandError::ServiceStatusExit {
            exit_code: Some(102),
            signal: None,
        };
        assert_eq!(
            serde_json::to_value(exit).unwrap()["kind"],
            "service_status_exit"
        );
        assert!(matches!(
            parse_service_status(r#"{"status":"stopped"}"#),
            Err(ServiceCommandError::ParseServiceStatus { .. })
        ));
    }

    #[test]
    fn an_unreadable_probe_is_not_a_stopped_observation() {
        let err = parse_service_status("not JSON").unwrap_err();
        assert!(matches!(
            err,
            ServiceCommandError::ParseServiceStatus { .. }
        ));
        assert_eq!(
            serde_json::to_value(err).unwrap()["kind"],
            "parse_service_status"
        );
    }

    #[test]
    fn timeout_and_still_running_are_distinct() {
        let timeout = ServiceCommandError::TimedOut { limit_ms: 5000 };
        let outstanding = ServiceCommandError::StillRunning { limit_ms: 5000 };
        assert_ne!(
            serde_json::to_value(timeout).unwrap()["kind"],
            serde_json::to_value(outstanding).unwrap()["kind"]
        );
    }
}

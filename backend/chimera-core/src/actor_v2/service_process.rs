//! Service process protocol is defined once by the shared OS contract.
//! The independent core retains only its Chimera IPC status decoder.

pub use chimera_contracts::service::{ServiceCommand, ServiceCommandError, ServiceProcessControl};

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

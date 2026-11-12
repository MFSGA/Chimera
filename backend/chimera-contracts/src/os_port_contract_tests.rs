//! Contract regression tests extracted from the pinned ref OS port suite.

use std::{error::Error, io};

use serde_json::json;

use crate::{
    elevation::{ElevationConsentError, ElevationOperation},
    service::{ServiceCommand, ServiceCommandError},
    system_dns::SystemDnsError,
};

#[test]
fn dns_errors_preserve_sources_without_serializing_io_details() {
    let error = SystemDnsError::RunFlushCommand {
        command: "ipconfig.exe",
        source: io::Error::other("private command output"),
    };
    assert_eq!(
        error.to_string(),
        "could not run ipconfig.exe to flush the system DNS cache"
    );
    assert_eq!(
        error.source().unwrap().to_string(),
        "private command output"
    );
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({"kind": "run_flush_command", "command": "ipconfig.exe"})
    );

    let rejected = SystemDnsError::FlushRejected {
        command: "ipconfig.exe",
        code: Some(5),
    };
    assert_eq!(
        serde_json::to_value(rejected).unwrap(),
        json!({"kind": "flush_rejected", "command": "ipconfig.exe", "code": 5})
    );
    assert_eq!(
        serde_json::to_value(SystemDnsError::Unsupported).unwrap(),
        json!({"kind": "unsupported"})
    );
}

#[test]
fn elevated_action_contract_keeps_portable_wire_shape() {
    for (operation, expected) in [
        (
            ElevationOperation::Service {
                command: ServiceCommand::Restart,
            },
            json!({"kind": "service", "command": "restart"}),
        ),
        (ElevationOperation::DnsFlush, json!({"kind": "dns_flush"})),
        (
            ElevationOperation::CoreBinaryCopy,
            json!({"kind": "core_binary_copy"}),
        ),
    ] {
        let error = ElevationConsentError::AuthorizeElevation { operation };
        assert_eq!(serde_json::to_value(operation).unwrap(), expected);
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({"kind": "authorize_elevation", "operation": expected})
        );
    }
}

#[test]
fn service_and_dns_authorization_failures_keep_typed_sources() {
    let service = ServiceCommandError::AuthorizeServiceCommand {
        command: ServiceCommand::Install,
        source: ElevationConsentError::AuthorizeElevation {
            operation: ElevationOperation::Service {
                command: ServiceCommand::Install,
            },
        },
    };
    assert!(service.source().unwrap().is::<ElevationConsentError>());
    assert_eq!(
        serde_json::to_value(service).unwrap(),
        json!({
            "kind": "authorize_service_command",
            "command": "install",
            "source": {
                "kind": "authorize_elevation",
                "operation": {"kind": "service", "command": "install"}
            }
        })
    );

    let dns = SystemDnsError::AuthorizeDnsFlush {
        source: ElevationConsentError::AuthorizeElevation {
            operation: ElevationOperation::DnsFlush,
        },
    };
    assert!(dns.source().unwrap().is::<ElevationConsentError>());
    assert_eq!(
        serde_json::to_value(dns).unwrap(),
        json!({
            "kind": "authorize_dns_flush",
            "source": {
                "kind": "authorize_elevation",
                "operation": {"kind": "dns_flush"}
            }
        })
    );
}

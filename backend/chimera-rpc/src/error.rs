//! Typed error boundary. Unknown failures remain redacted rather than guessing a source.
use chimera_contracts::{runtime_error::RuntimeError, system_dns::SystemDnsError};
use chimera_core::{client::clash_api::ClashReadError, state::profiles::ProfilesError};
use chimera_rpc_protocol::{
    ResponseVisibility, RpcError, RpcValue,
    types::{PublicClashReadError, PublicFailure, PublicSystemDnsError},
};

pub fn application_error_for(
    visibility: ResponseVisibility,
    error: impl Into<anyhow::Error>,
) -> RpcError {
    let error = error.into();
    let mut result = RpcError::new(
        "application_error",
        match visibility {
            ResponseVisibility::Full => error.to_string(),
            ResponseVisibility::Public => "application command failed".into(),
        },
    );
    // Walk real source errors before encoding. No formatted diagnostics enter public values.
    for cause in error.chain() {
        if let Some(runtime) = cause.downcast_ref::<RuntimeError>() {
            if let Some(failure) = runtime.core_failure() {
                result.code = failure.kind.map(|kind| kind.to_string());
                result.retryable = Some(failure.retryable);
                result.operation_id = failure.operation_id.clone();
            }
            if visibility == ResponseVisibility::Public {
                result.domain_error = encode(PublicFailure::Runtime(
                    crate::privacy::project_runtime_error(runtime),
                ));
            } else {
                result.domain_error = encode(runtime);
            }
            break;
        }
        if let Some(profile) = cause.downcast_ref::<ProfilesError>() {
            let failure = profile.typed_failure();
            result.domain_error = if visibility == ResponseVisibility::Public {
                encode(PublicFailure::Profiles(
                    crate::privacy::project_profile_failure(&failure),
                ))
            } else {
                encode(failure)
            };
            break;
        }
        if let Some(clash) = cause.downcast_ref::<ClashReadError>() {
            if visibility == ResponseVisibility::Public {
                result.domain_error =
                    encode(PublicFailure::ClashRead(project_clash_read_error(clash)));
            }
            break;
        }
        if let Some(log) = cause.downcast_ref::<chimera_core::logs::query::CoreLogError>() {
            use chimera_core::logs::query::CoreLogError as E;
            use chimera_rpc_protocol::types::PublicCoreLogReadError as P;
            let projected = match log {
                E::InvalidRequest => P::InvalidRequest,
                E::CursorExpired => P::CursorExpired,
                E::RecordGone => P::RecordGone,
                E::TooLarge => P::TooLarge,
                E::Unavailable(_) => P::Unavailable,
            };
            if visibility == ResponseVisibility::Public {
                result.domain_error = encode(PublicFailure::CoreLogRead(projected));
            }
            break;
        }
        if let Some(dns) = cause.downcast_ref::<SystemDnsError>() {
            if visibility == ResponseVisibility::Public {
                result.domain_error =
                    encode(PublicFailure::SystemDns(project_system_dns_error(dns)));
            }
            break;
        }
    }
    result
}

fn encode(value: impl serde::Serialize) -> Option<Box<RpcValue>> {
    serde_json::to_value(value)
        .ok()
        .map(|value| Box::new(RpcValue(value)))
}

fn project_clash_read_error(error: &ClashReadError) -> PublicClashReadError {
    match error {
        ClashReadError::SettingsUnavailable => PublicClashReadError::SettingsUnavailable,
        ClashReadError::ProxiesUnavailable => PublicClashReadError::ProxiesUnavailable,
        ClashReadError::UnknownGroup => PublicClashReadError::UnknownGroup,
        ClashReadError::DelayTestFailed => PublicClashReadError::DelayTestFailed,
        ClashReadError::VersionUnavailable => PublicClashReadError::VersionUnavailable,
        ClashReadError::RulesUnavailable => PublicClashReadError::RulesUnavailable,
        ClashReadError::RuleProvidersUnavailable => PublicClashReadError::RuleProvidersUnavailable,
        ClashReadError::ProxyProvidersUnavailable => {
            PublicClashReadError::ProxyProvidersUnavailable
        }
        ClashReadError::StreamsUnavailable => PublicClashReadError::StreamsUnavailable,
    }
}

fn project_system_dns_error(error: &SystemDnsError) -> PublicSystemDnsError {
    match error {
        SystemDnsError::AuthorizeDnsFlush { .. } => PublicSystemDnsError::AuthorizeDnsFlush,
        SystemDnsError::RunFlushCommand { .. } => PublicSystemDnsError::RunFlushCommand,
        SystemDnsError::FlushRejected { code, .. } => {
            PublicSystemDnsError::FlushRejected { exit_code: *code }
        }
        SystemDnsError::Unsupported => PublicSystemDnsError::Unsupported,
    }
}

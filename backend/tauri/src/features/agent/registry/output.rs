use serde::Serialize;
use serde_json::Value;

use super::{AgentToolError, AgentToolErrorCode};

pub(super) fn serialize_tool_result<T: Serialize>(
    result: T,
    error_message: &'static str,
    allowed_top_level_fields: &[&str],
) -> Result<Value, AgentToolError> {
    let value = serde_json::to_value(result)
        .map_err(|_| AgentToolError::new(AgentToolErrorCode::ExecutionFailed, error_message))?;
    validate_public_output_shape(&value)?;
    validate_top_level_output_fields(&value, allowed_top_level_fields)?;
    Ok(value)
}

fn validate_top_level_output_fields(
    value: &Value,
    allowed_fields: &[&str],
) -> Result<(), AgentToolError> {
    let Value::Object(fields) = value else {
        return Err(privacy_contract_error());
    };
    if fields
        .keys()
        .any(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(privacy_contract_error());
    }
    Ok(())
}

fn validate_public_output_shape(value: &Value) -> Result<(), AgentToolError> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_public_output_shape(value)?;
            }
        }
        Value::Object(fields) => {
            for (name, value) in fields {
                if is_privacy_assertion_key(name) {
                    if value != &Value::Bool(false) {
                        return Err(privacy_contract_error());
                    }
                } else if is_sensitive_output_key(name) {
                    return Err(privacy_contract_error());
                }
                validate_public_output_shape(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_privacy_assertion_key(name: &str) -> bool {
    matches!(
        name,
        "contains_raw_logs"
            | "contains_profile_names"
            | "contains_profile_urls"
            | "contains_connection_targets"
            | "contains_controller_secret"
    )
}

fn is_sensitive_output_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "token", "secret", "url", "host", "address", "target", "log", "bypass",
    ]
    .into_iter()
    .any(|sensitive| name.contains(sensitive))
}

fn privacy_contract_error() -> AgentToolError {
    AgentToolError::new(
        AgentToolErrorCode::ExecutionFailed,
        "agent tool output violates the privacy contract",
    )
}

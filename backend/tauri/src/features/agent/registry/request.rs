use serde::Deserialize;

use super::{AgentToolError, AgentToolErrorCode};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArguments {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionalToolEnvelope<T> {
    #[serde(default)]
    arguments: T,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequiredToolEnvelope<T> {
    pub(super) arguments: T,
}

pub(super) fn parse_empty_request(body: &[u8]) -> Result<(), AgentToolError> {
    let request: OptionalToolEnvelope<EmptyArguments> = parse_body(body)?;
    let EmptyArguments {} = request.arguments;
    Ok(())
}

pub(super) fn parse_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, AgentToolError> {
    let body = if body.is_empty() { b"{}" } else { body };
    serde_json::from_slice(body).map_err(|_| {
        AgentToolError::new(
            AgentToolErrorCode::InvalidRequest,
            "invalid agent tool request",
        )
    })
}

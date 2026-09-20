//! Daemon core-control adapter.
//!
//! The app talks to Chimera Service through the additive v2 submit/wait wire.
//! The daemon owns operation admission and execution; this adapter waits on
//! that durable registry and marks transport loss after admission as uncertain.

use std::{
    borrow::Cow,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use chimera_ipc::{
    api::{
        core::v2::{
            CoreApiConnection, CoreCommandInfo, CoreOperationReq, CoreSubmitReq,
            OperationOutputInfo, OperationPhase, payload_digest,
        },
        status::{CoreInfos, CoreState, RevisionIdInfo},
    },
    client::{ClientError, shortcuts::Client},
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct ServiceCoreOutcomeUncertain(String);

pub(crate) fn is_outcome_uncertain(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<ServiceCoreOutcomeUncertain>()
            .is_some()
    })
}

fn uncertain(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(ServiceCoreOutcomeUncertain(message.into()))
}

fn next_operation_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let counter = NEXT.fetch_add(1, Ordering::Relaxed) as u128;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let process = (std::process::id() as u128) << 64;
    format!("{:032x}", now ^ process ^ counter)
}

fn submit_error(error: ClientError<'_>) -> anyhow::Error {
    if matches!(&error, ClientError::ServerResponseFailed(_)) {
        anyhow::anyhow!(error.to_string())
    } else {
        uncertain(format!(
            "service core operation admission reply was lost; outcome is uncertain: {error}"
        ))
    }
}

fn expected_applied_from_status(info: &CoreInfos) -> anyhow::Result<Option<RevisionIdInfo>> {
    match info.state {
        CoreState::Running => info
            .revision
            .as_ref()
            .map(|revision| revision.id())
            .map(Some)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "service core is running without a v2 applied revision; refusing an unconditional reconcile"
                )
            }),
        CoreState::Stopped(_) => Ok(None),
    }
}

fn query_error(error: ClientError<'_>) -> anyhow::Error {
    uncertain(format!(
        "service core operation was admitted but its terminal state could not be queried: {error}"
    ))
}

async fn submit_and_wait(command: CoreCommandInfo<'static>) -> anyhow::Result<OperationOutputInfo> {
    let client = Client::service_default();
    let operation_id = next_operation_id();
    let request = CoreSubmitReq {
        operation_id: Cow::Owned(operation_id.clone()),
        command,
    };
    let mut info = client
        .submit_core_v2(&request)
        .await
        .map_err(submit_error)?;

    loop {
        match info.phase {
            OperationPhase::Succeeded => {
                return info.output.ok_or_else(|| {
                    anyhow::anyhow!(
                        "service core operation {} succeeded without an output",
                        info.id
                    )
                });
            }
            OperationPhase::Failed => {
                let message = info.error.map(|error| error.message).unwrap_or_else(|| {
                    "service core operation failed without an error".to_string()
                });
                anyhow::bail!("service core operation {} failed: {message}", info.id);
            }
            OperationPhase::Queued | OperationPhase::Running => {}
        }

        let query = CoreOperationReq {
            operation_id: Cow::Owned(info.id.clone()),
            wait_ms: Some(60_000),
        };
        info = client
            .core_operation_v2(&query)
            .await
            .map_err(query_error)?;
    }
}

#[async_trait]
pub(crate) trait ServiceCoreHost: Send + Sync + std::fmt::Debug {
    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>>;
    async fn status(&self) -> anyhow::Result<(CoreState, i64)>;
    async fn start(
        &self,
        config_path: &Path,
        core_type: &chimera_utils::core::CoreType,
    ) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Default)]
pub(crate) struct IpcServiceCoreHost;

#[async_trait]
impl ServiceCoreHost for IpcServiceCoreHost {
    async fn api_connection(&self) -> anyhow::Result<Option<CoreApiConnection>> {
        Client::service_default()
            .core_api_v2()
            .await
            .map_err(anyhow::Error::from)
    }

    async fn status(&self) -> anyhow::Result<(CoreState, i64)> {
        let info = Client::service_default()
            .core_status_v2()
            .await
            .map_err(anyhow::Error::from)?;
        Ok((info.state, info.state_changed_at))
    }

    async fn start(
        &self,
        config_path: &Path,
        core_type: &chimera_utils::core::CoreType,
    ) -> anyhow::Result<()> {
        let config = tokio::fs::read_to_string(config_path)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "failed to read promoted service config {}: {error}",
                    config_path.display()
                )
            })?;
        let digest = payload_digest(config.as_bytes());
        let status = Client::service_default()
            .core_status_v2()
            .await
            .map_err(anyhow::Error::from)?;
        let expected_applied = expected_applied_from_status(&status)?;

        let output = submit_and_wait(CoreCommandInfo::Reconcile {
            core_type: Cow::Owned(core_type.clone()),
            config: Cow::Owned(config),
            expected_digest: Some(Cow::Owned(digest.clone())),
            expected_applied,
        })
        .await?;
        let OperationOutputInfo::Reconciled(outcome) = output else {
            anyhow::bail!("service reconcile returned an unexpected terminal output: {output:?}");
        };
        anyhow::ensure!(
            outcome.revision.source_hash == digest,
            "service reconcile applied a different config digest: expected {digest}, got {}",
            outcome.revision.source_hash
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let output = submit_and_wait(CoreCommandInfo::Stop).await?;
        anyhow::ensure!(
            matches!(output, OperationOutputInfo::Stopped),
            "service stop returned an unexpected terminal output: {output:?}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_operation_ids_match_the_daemon_contract() {
        let first = next_operation_id();
        let second = next_operation_id();
        assert_eq!(first.len(), 32);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_ne!(first, second);
    }

    #[test]
    fn stopped_service_reconcile_is_unconditional() {
        let info = CoreInfos {
            r#type: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            config_path: None,
            revision: None,
        };
        assert_eq!(expected_applied_from_status(&info).unwrap(), None);
    }

    #[test]
    fn running_service_reconcile_uses_daemon_revision_cas() {
        let revision = chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 4,
            generation: 2,
            source_hash: "source".to_string(),
            effective_hash: "effective".to_string(),
        };
        let expected = revision.id();
        let info = CoreInfos {
            r#type: None,
            state: CoreState::Running,
            state_changed_at: 0,
            config_path: None,
            revision: Some(revision),
        };
        assert_eq!(expected_applied_from_status(&info).unwrap(), Some(expected));
    }

    #[test]
    fn running_service_without_revision_fails_closed() {
        let info = CoreInfos {
            r#type: None,
            state: CoreState::Running,
            state_changed_at: 0,
            config_path: None,
            revision: None,
        };
        let error = expected_applied_from_status(&info).unwrap_err();
        assert!(error.to_string().contains("without a v2 applied revision"));
    }

    #[test]
    fn uncertain_marker_is_discoverable_through_anyhow_context_chain() {
        let error = uncertain("query lost").context("service apply failed");
        assert!(is_outcome_uncertain(&error));
        assert!(!is_outcome_uncertain(&anyhow::anyhow!(
            "known terminal failure"
        )));
    }
}

use std::future::Future;

use anyhow::{Result, anyhow};
use serde_yaml::Mapping;
use tokio::sync::Mutex;

use super::api::ClashConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransactionOutcome {
    Committed,
    Rejected {
        primary_error: String,
    },
    RolledBack {
        primary_error: String,
    },
    RollbackFailed {
        primary_error: String,
        rollback_error: String,
    },
}

impl TransactionOutcome {
    pub(crate) fn into_result(self) -> Result<()> {
        match self {
            Self::Committed => Ok(()),
            Self::Rejected { primary_error } | Self::RolledBack { primary_error } => {
                Err(anyhow!(primary_error))
            }
            Self::RollbackFailed {
                primary_error,
                rollback_error,
            } => Err(anyhow!(
                "{primary_error}; rollback failed: {rollback_error}"
            )),
        }
    }
}

/// Serializes running-core patch transactions so snapshots and compensations
/// cannot interleave across concurrent IPC requests.
#[derive(Default)]
pub(crate) struct RuntimePatchCoordinator {
    gate: Mutex<()>,
}

impl RuntimePatchCoordinator {
    pub(crate) async fn apply<R, RFut, P, PFut, S, SFut>(
        &self,
        requested: Mapping,
        read_core: R,
        patch_core: P,
        persist: S,
    ) -> TransactionOutcome
    where
        R: FnMut() -> RFut,
        RFut: Future<Output = Result<ClashConfig>>,
        P: FnMut(Mapping) -> PFut,
        PFut: Future<Output = Result<()>>,
        S: FnMut(Mapping) -> SFut,
        SFut: Future<Output = Result<()>>,
    {
        let _guard = self.gate.lock().await;
        apply_runtime_patch_outcome(requested, read_core, patch_core, persist).await
    }
}

fn config_mapping(config: &ClashConfig) -> Result<Mapping> {
    let value = serde_yaml::to_value(config)?;
    value
        .as_mapping()
        .cloned()
        .ok_or_else(|| anyhow!("Clash config must serialize to a mapping"))
}

fn values_for_patch(config: &ClashConfig, patch: &Mapping) -> Result<Mapping> {
    let current = config_mapping(config)?;
    let mut selected = Mapping::new();

    for key in patch.keys() {
        let value = current
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("core config does not contain field {key:?}"))?;
        selected.insert(key.clone(), value);
    }

    Ok(selected)
}

fn patch_matches_config(config: &ClashConfig, patch: &Mapping) -> Result<bool> {
    Ok(values_for_patch(config, patch)? == *patch)
}

async fn rollback_after_failure<R, RFut, P, PFut>(
    read_core: &mut R,
    patch_core: &mut P,
    previous: &ClashConfig,
    requested: &Mapping,
    primary: anyhow::Error,
) -> TransactionOutcome
where
    R: FnMut() -> RFut,
    RFut: Future<Output = Result<ClashConfig>>,
    P: FnMut(Mapping) -> PFut,
    PFut: Future<Output = Result<()>>,
{
    let primary_error = primary.to_string();
    let rollback = match values_for_patch(previous, requested) {
        Ok(rollback) => rollback,
        Err(error) => {
            return TransactionOutcome::RollbackFailed {
                primary_error,
                rollback_error: format!("cannot build rollback patch: {error}"),
            };
        }
    };

    if let Err(error) = patch_core(rollback.clone()).await {
        return TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error: format!("rollback patch failed: {error}"),
        };
    }

    let restored = match read_core().await {
        Ok(restored) => restored,
        Err(error) => {
            return TransactionOutcome::RollbackFailed {
                primary_error,
                rollback_error: format!("rollback read-back failed: {error}"),
            };
        }
    };

    match patch_matches_config(&restored, &rollback) {
        Ok(true) => TransactionOutcome::RolledBack { primary_error },
        Ok(false) => TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error: "rollback verification failed".to_string(),
        },
        Err(error) => TransactionOutcome::RollbackFailed {
            primary_error,
            rollback_error: format!(
                "rollback verification could not inspect the restored fields: {error}"
            ),
        },
    }
}

/// Applies a running-core patch, verifies it, and only then persists desired state.
///
/// Every failure after the initial snapshot triggers a field-scoped compensation.
/// The compensation is also read back, so a successful return from the rollback
/// endpoint is not treated as proof that the running core was restored.
async fn apply_runtime_patch_outcome<R, RFut, P, PFut, S, SFut>(
    requested: Mapping,
    mut read_core: R,
    mut patch_core: P,
    mut persist: S,
) -> TransactionOutcome
where
    R: FnMut() -> RFut,
    RFut: Future<Output = Result<ClashConfig>>,
    P: FnMut(Mapping) -> PFut,
    PFut: Future<Output = Result<()>>,
    S: FnMut(Mapping) -> SFut,
    SFut: Future<Output = Result<()>>,
{
    if requested.is_empty() {
        return TransactionOutcome::Committed;
    }

    let previous = match read_core().await {
        Ok(previous) => previous,
        Err(error) => {
            return TransactionOutcome::Rejected {
                primary_error: error.to_string(),
            };
        }
    };

    if let Err(error) = patch_core(requested.clone()).await {
        return rollback_after_failure(
            &mut read_core,
            &mut patch_core,
            &previous,
            &requested,
            error,
        )
        .await;
    }

    let current = match read_core().await {
        Ok(config) => config,
        Err(error) => {
            return rollback_after_failure(
                &mut read_core,
                &mut patch_core,
                &previous,
                &requested,
                error,
            )
            .await;
        }
    };

    match patch_matches_config(&current, &requested) {
        Ok(true) => {}
        Ok(false) => {
            return rollback_after_failure(
                &mut read_core,
                &mut patch_core,
                &previous,
                &requested,
                anyhow!("core config did not apply the requested runtime patch"),
            )
            .await;
        }
        Err(error) => {
            return rollback_after_failure(
                &mut read_core,
                &mut patch_core,
                &previous,
                &requested,
                error,
            )
            .await;
        }
    }

    if let Err(error) = persist(requested.clone()).await {
        return rollback_after_failure(
            &mut read_core,
            &mut patch_core,
            &previous,
            &requested,
            error,
        )
        .await;
    }

    TransactionOutcome::Committed
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod tests;

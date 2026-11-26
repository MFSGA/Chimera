//! Committed startup S3/S4 convergence through the existing selected owner.
//! No new actor, lease, or runtime product writer is created.
use super::{
    apply_executor::ApplyHost, attempt::CommittedTargetAdvanced,
    convergence_executor::ConvergenceHost, decision::same_core_status, plan::Trigger,
    startup::CommittedRound,
};
use crate::client::core_lifecycle::{
    ports::CoreLifecyclePort,
    status::{CoreStatusSnapshot, RunType},
};
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use std::{collections::BTreeMap, sync::Arc};

/// Preserve the fallible frozen-file read and exact target identity.
pub async fn capture_target(
    host: &(impl ConvergenceHost + ?Sized),
    app: ChimeraAppConfig,
    clash: ClashConfig,
) -> anyhow::Result<(
    String,
    ChimeraAppConfig,
    ClashConfig,
    Arc<Profiles>,
    BTreeMap<String, String>,
)> {
    let inputs = host
        .capture_committed_profile_inputs_for(app, clash)
        .await?;
    let target = inputs.checked_target_key()?;
    let (app, clash, profiles, content) = inputs.into_parts()?;
    Ok((target, app, clash, profiles, content))
}

/// Startup S2/S3 established and, if necessary, retired the Local Core
/// before reaching here. Reuse the exact committed-source preparation
/// instead of the old inline YAML construction whenever the sole Core
/// owner and authoritative source reader are available.
pub async fn try_startup_prepared_selected(
    host: &(impl ConvergenceHost + ApplyHost + ?Sized),
    app: &ChimeraAppConfig,
    run_type: RunType,
) -> Option<anyhow::Result<()>> {
    if !host.canonical_convergence_session() || host.convergence_authority().is_none() {
        return None;
    }
    let clash = match host.convergence_clash() {
        Ok(clash) => clash,
        Err(error) => return Some(Err(error)),
    };
    let (target, app, clash, ..) = match capture_target(host, app.clone(), clash).await {
        Ok(committed) => committed,
        Err(error) => return Some(Err(error)),
    };
    match run_type {
        RunType::Normal => {
            host.try_reconcile_prepared_local(&target, &app, &clash, run_type)
                .await
        }
        RunType::Service => {
            host.try_reconcile_prepared_service(&target, &app, &clash)
                .await
        }
        RunType::Elevated => None,
    }
}

/// Recovery after S2 proved the selected owner. Reuse the normal ref
/// committed S3/S4 planner, without a new lease or host selection.
/// None is reserved for the pre-canonical compatibility path.
pub async fn recover_committed_startup(
    host: &(impl ConvergenceHost + ApplyHost + ?Sized),
    observed: &CoreStatusSnapshot,
) -> Option<anyhow::Result<()>> {
    if !host.canonical_convergence_session()
        || host.convergence_authority().is_none()
        || observed.run_type == RunType::Elevated
        || observed.state_changed_at == 0
    {
        return None;
    }
    let selected_host = crate::client::reconciler::proof::host_for(observed.run_type);
    let result = async {
        let clash = host.convergence_clash()?;
        let (target, app, clash, ..) =
            capture_target(host, host.startup_application_config(), clash).await?;
        if !same_core_status(observed, &host.core().status().await?) {
            return Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "the S2-proven owner changed before startup convergence",
                )
                .into(),
            );
        }
        let result = match super::committed_round::planned_committed_round(
            host,
            &target,
            selected_host,
            Trigger::Startup,
        )
        .await?
        {
            CommittedRound::Settled(result) => result,
            CommittedRound::Apply => {
                host.progress()
                    .lock()
                    .allow(&target, selected_host, Trigger::Startup)?;
                host.progress().lock().stage(target.clone(), selected_host);
                super::apply_executor::reconcile_prepared_selected_with_policy(
                    host,
                    &target,
                    &app,
                    &clash,
                    observed.run_type,
                    false,
                )
                .await
                .unwrap_or_else(|| {
                    Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "the S4-admitted startup candidate lost its owner or source proof",
                        )
                        .into(),
                    )
                })
            }
            CommittedRound::LegacyRecovery => Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "canonical startup recovery has no settled S3/S4 plan",
                )
                .into(),
            ),
        };
        if host.startup_target_identity().await? != target {
            return Err(CommittedTargetAdvanced.into());
        }
        result
    }
    .await;
    if result.is_err() {
        host.progress().lock().invalidate_selection();
    }
    host.refresh_service_recovery_host().await;
    Some(result)
}

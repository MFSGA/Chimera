//! Committed startup S3/S4 convergence through the existing selected owner.
//! No new actor, lease, or runtime product writer is created.
use super::{
    apply_executor::ApplyHost, attempt::CommittedTargetAdvanced,
    convergence_executor::ConvergenceHost, decision::same_core_status, plan::Trigger,
    startup::CommittedRound,
};
use crate::client::core_lifecycle::{
    ports::CoreLifecyclePort,
    state::CoreLifecycleState,
    status::{CoreStatusSnapshot, RunType},
};
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use chimera_contracts::execution_host::ExecutionHost;
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

/// The caller holds the original transition lease and has already proved Service release.
pub async fn reconcile_application_after_service_release(
    state: &CoreLifecycleState,
    host: &(impl ConvergenceHost + ApplyHost + ?Sized),
    allow_legacy_fixture: bool,
    app: ChimeraAppConfig,
    expected_target: Option<&str>,
) -> anyhow::Result<()> {
    // The earlier rejected Service candidate is not the Local fallback's
    // terminal; a second admitted Core command must supersede its verdict.
    state.take_prepared_outcome();
    use crate::client::reconciler::attempt::{CommittedTargetAdvanced, OwnershipTransitionPending};
    if let Some(expected) = expected_target
        && host.startup_target_identity().await? != expected
    {
        return Err(CommittedTargetAdvanced.into());
    }
    let clash = host.convergence_clash()?;
    if allow_legacy_fixture && state.profiles.is_none() {
        if let Some(target) = expected_target {
            state
                .progress
                .lock()
                .stage(target.to_owned(), ExecutionHost::Local);
        }
        return state
            .core
            .reconcile_after_service_release(clash, app.core)
            .await;
    }
    anyhow::ensure!(
        host.canonical_convergence_session() || allow_legacy_fixture,
        "Service release requires the canonical Reconciler"
    );
    let (target, app, clash, profiles, staged_content) = capture_target(host, app, clash).await?;

    if expected_target.is_some_and(|expected| expected != target) {
        return Err(CommittedTargetAdvanced.into());
    }

    if state.checked_gate.get().is_some()
        && let Some(authority) = state.source_authority.as_ref()
        && state.core.runtime_preparation_session().is_some()
    {
        anyhow::ensure!(
            !state.core.outcome_uncertain(),
            "Service-to-Local handback cannot begin while the Core outcome is uncertain"
        );
        // A newer committed source may have reversed the preferred host
        // while the Service release was being proved. Do not change the
        // host at all for a target that has already been superseded.
        anyhow::ensure!(
            authority
                .verify_committed_prepared_target(&target, None)
                .await?
                .is_some(),
            "authoritative source cannot prove the released Service fallback target"
        );
        // The caller holds the transition lease and has *already* proved
        // that the rejected Service Core was released. Preserve this
        // evidence when selecting Local; do not submit via the router's
        // old combined host-handoff + inline-build path.
        state
            .core
            .select_local_after_confirmed_service_release()
            .await?;
        state
            .progress
            .lock()
            .stage(target.clone(), ExecutionHost::Local);
        // Once S2 has moved the owner, missing Core/source proof must be
        // Waiting, not a license for a second legacy submit or handoff.
        return host
            .try_reconcile_prepared_local(&target, &app, &clash, RunType::Normal)
            .await
            .unwrap_or_else(|| {
                Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "Local Core ownership was not proven after the Service release handback",
                    )
                    .into(),
                )
            });
    }

    if host.canonical_convergence_session() {
        return Err(OwnershipTransitionPending::waiting(
            "Service release lost the canonical preparation or source authority",
        )
        .into());
    }

    if !allow_legacy_fixture {
        return Err(OwnershipTransitionPending::waiting(
            "Service release requires prepared runtime admission",
        )
        .into());
    }

    state
        .core
        .reconcile_profiles_after_service_release(
            clash,
            app.core,
            profiles,
            app,
            staged_content,
            Some(target),
        )
        .await
}

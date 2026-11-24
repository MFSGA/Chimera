//! Core binary replacement, retaining the ref stop/recover/install/restart order.
use super::{
    executor::LifecycleHost,
    ports::PreparedCoreBinary,
    workflow::{RestartIntent, binary_replacement},
};
use crate::client::{core_spec::core_kind, reconciler::plan::Trigger};
use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::api::status::CoreState;

pub async fn replace_binary(
    host: &(impl LifecycleHost + ?Sized),
    artifact: PreparedCoreBinary,
) -> anyhow::Result<()> {
    let desired_core = host.application_config().core;

    // The selected core is desired state, not proof of what is currently
    // running. A confirmed live receipt identifies the active executable
    // even when an older host omits its applied-kind field. If the host is
    // running but cannot provide that proof, stop conservatively before
    // replacing a binary that may be in use.
    let active_receipt = match host.core().active_runtime_receipt().await {
        Ok(receipt) => receipt,
        Err(error) => {
            tracing::warn!(
                %error,
                "could not verify the active core before binary replacement; treating its identity as unknown"
            );
            None
        }
    };
    let status = host.core().status().await?;
    // Service status failures are projected to Stopped with timestamp 0;
    // that compatibility value is unknown, not proof that the process exited.
    let not_proven_stopped = active_receipt.is_some()
        || status.state_changed_at == 0
        || !matches!(status.state, CoreState::Stopped(_));
    let target_kind = core_kind(artifact.target);
    let active_kind = active_receipt.map(|receipt| receipt.core_spec.kind);
    let (needs_stop, restart) = binary_replacement(
        not_proven_stopped,
        active_kind,
        target_kind,
        desired_core == artifact.target,
    );

    if needs_stop {
        match host.core().stop().await {
            Ok(()) => {}
            Err(error)
                if !host.core().outcome_uncertain()
                    && error
                        .downcast_ref::<CoreError>()
                        .is_some_and(|error| error.kind == Some(CoreErrorKind::NotStarted)) => {}
            Err(error) => return Err(error),
        }
    }
    // Ref: even a confirmed Stopped result does not prove quarantined
    // epochs are gone. Recover the selected endpoint before touching the
    // binary. A lost terminal must not authorize the install.
    host.core().recover_before_binary_install().await?;
    anyhow::ensure!(
        !host.core().outcome_uncertain(),
        "Core quarantine recovery is uncertain; refusing binary replacement"
    );

    let request = chimera_contracts::core_install::CoreBinaryInstallRequest {
        target: artifact.target,
        source: artifact.source.clone(),
        destination: artifact.destination.clone(),
        staging: artifact.staging.clone(),
        // Chimera's currently distributed core archives contain no companion
        // directory. Keep the upstream optional contract without inventing
        // Meow-specific or unsupported release contents.
        companion: None,
    };
    host.installer().install(&request).await?;
    if restart == RestartIntent::Owed {
        artifact.progress.restarting();
        host.reconcile_with_trigger(Trigger::ServiceOperation)
            .await?;
    }
    Ok(())
}

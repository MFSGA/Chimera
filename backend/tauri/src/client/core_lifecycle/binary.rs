//! Core binary replacement; restart intent follows ref lifecycle policy.
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::core_lifecycle::ports::PreparedCoreBinary;
use chimera_core::client::core_lifecycle::workflow::{RestartIntent, binary_replacement};
use chimera_core::client::core_spec::core_kind;
use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::api::status::CoreState;

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn replace_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        let desired_core = self.application.get_typed().core;

        // The selected core is desired state, not proof of what is currently
        // running. A confirmed live receipt identifies the active executable
        // even when an older host omits its applied-kind field. If the host is
        // running but cannot provide that proof, stop conservatively before
        // replacing a binary that may be in use.
        let active_receipt = match self.core.active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "could not verify the active core before binary replacement; treating its identity as unknown"
                );
                None
            }
        };
        let status = self.core.status().await?;
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
            match self.core.stop().await {
                Ok(()) => {}
                Err(error)
                    if !self.core.outcome_uncertain()
                        && error
                            .downcast_ref::<CoreError>()
                            .is_some_and(|error| error.kind == Some(CoreErrorKind::NotStarted)) => {
                }
                Err(error) => return Err(error),
            }
        }
        // Ref: even a confirmed Stopped result does not prove quarantined
        // epochs are gone. Recover the selected endpoint before touching the
        // binary. A lost terminal must not authorize the install.
        self.core.recover_before_binary_install().await?;
        anyhow::ensure!(
            !self.core.outcome_uncertain(),
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
        self.installer.install(&request).await?;
        if restart == RestartIntent::Owed {
            artifact.progress.restarting();
            self.reconcile().await?;
        }
        Ok(())
    }
}

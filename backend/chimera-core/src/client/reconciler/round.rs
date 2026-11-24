//! Settlement of the single runtime owner's observed terminal.
use super::{
    apply::{CheckedConnectionEffect, finish_connection_effect},
    authority::RuntimeSourceAuthority,
    proof::verified_applied_runtime,
    state::{RuntimeProgress, proven_selection_from_receipt},
};
use crate::client::core_lifecycle::ports::CoreLifecyclePort;

/// One completed runtime round, observed only by the existing mailbox after
/// the worker has joined. This object does not own or mutate a Core session.
pub struct Round {
    pub verified_running: bool,
    verified_stopped: bool,
    pub connection_effect_observed: bool,
    pub connection_degradation: Option<chimera_contracts::runtime::Degradation>,
    verdict: super::attempt::LegacyApplyVerdict,
}

impl Round {
    #[allow(clippy::too_many_arguments)]
    pub async fn observe<F: std::future::Future<Output = anyhow::Result<String>>>(
        core: &(impl CoreLifecyclePort + ?Sized),
        target_identity: impl Fn() -> F,
        progress: &parking_lot::Mutex<RuntimeProgress>,
        authority: Option<&dyn RuntimeSourceAuthority>,
        take_effect: impl FnOnce() -> Option<CheckedConnectionEffect>,
        result: &anyhow::Result<()>,
        trigger: &str,
        reconciled: bool,
        checked_apply: bool,
        source_advanced: bool,
        final_uncertain: bool,
        prepared_outcome: Option<&super::mutation::ApplyOutcome>,
    ) -> Self {
        // A successful command alone is not proof that the Core
        // reached Running or Stopped. Publish Converged/Inactive
        // only after sampling the runtime owner.
        let observed_core = if result.is_ok() && (reconciled || trigger == "Stop") {
            core.status().await.ok()
        } else {
            None
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_core_status_observed", "CoreLifecycle completion stage");
        let verified_running = if reconciled && result.is_ok() {
            if let Some(observed) = observed_core.as_ref() {
                verified_applied_runtime(core, &target_identity, observed).await
            } else {
                false
            }
        } else {
            false
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_running_verified", "CoreLifecycle completion stage");
        let verified_stopped = trigger == "Stop"
            && observed_core.as_ref().is_some_and(|snapshot| {
                matches!(
                    snapshot.state,
                    chimera_ipc::api::status::CoreState::Stopped(_)
                )
            });
        // The upstream reconciler records selection after a
        // confirmed runtime apply, not when a source saves.
        // Legacy startup, readiness and Retry use a Result<()>
        // Core port; recover only if both Core and source target
        // still match. Checked Apply has already done its own
        // exact-version selection check and must not be reset.
        if reconciled && !checked_apply {
            if result.is_ok() && verified_running && !source_advanced && !final_uncertain {
                let selection = proven_selection_from_receipt(core, authority).await;
                progress.lock().set_selection(selection);
            } else {
                progress.lock().invalidate_selection();
            }
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_selection_recovery_finished", "CoreLifecycle completion stage");
        // Unlike the worker, Completed owns the final observed
        // source-version fence and Running evidence. The previous
        // Core's connections may only be interrupted here; a
        // superseded checked product is never an effect success.
        let mut round_connection_degradation = None;
        let mut connection_effect_observed = false;
        if let Some(effect) = take_effect() {
            let issue = finish_connection_effect(
                effect,
                result,
                verified_running && !final_uncertain,
                source_advanced,
            )
            .await;
            if let Some(ref degradation) = issue {
                tracing::warn!(reason = ?degradation.reason, message = %degradation.message, "checked connection effect degraded");
            }
            connection_effect_observed = true;
            round_connection_degradation = issue;
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_core_mailbox", stage = "completed_connection_effect_finished", "CoreLifecycle completion stage");

        let verdict = super::attempt::observed_apply_verdict(
            result,
            final_uncertain,
            source_advanced,
            prepared_outcome,
        );
        Self {
            verified_running,
            verified_stopped,
            connection_effect_observed,
            connection_degradation: round_connection_degradation,
            verdict,
        }
    }

    /// One atomic status publication and one attempt settlement, from the
    /// exact same terminal. Never derive Applied from a successful reply.
    #[allow(clippy::too_many_arguments)]
    pub fn settle(
        self,
        progress: &parking_lot::Mutex<RuntimeProgress>,
        status: &tokio::sync::watch::Sender<super::status::ReconcilerStatus>,
        operation_id: u64,
        trigger: &str,
        observed_sources: &super::committed::SourceVersions,
        intent: crate::client::reconciler::plan::RunIntent,
        reconciled: bool,
    ) {
        progress
            .lock()
            .settle_with_verdict(&self.verdict, self.verified_running);
        status.send_modify(|status| {
            status.finish_with_verdict(
                operation_id,
                trigger,
                observed_sources,
                intent,
                reconciled,
                self.verified_running,
                self.verified_stopped,
                &self.verdict,
            );
            if let Some(degradation) = self.connection_degradation {
                status.record_system_degradation(degradation);
            }
        });
    }
}

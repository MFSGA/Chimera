//! Ref-aligned S4 candidate, Core submission, and post-apply maintenance.
//! The only Core command is submitted by the existing CoreLifecycle worker
//! through its sole CoreLifecyclePort; this module never creates a second
//! owner, revision allocator, RuntimeProduct writer, or action mailbox.
use std::{future::Future, sync::Arc};

use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use chimera_config::application::ChimeraAppConfig;
use chimera_contracts::execution_host::ExecutionHost;
use chimera_contracts::runtime_error::{CoreFailure, RuntimeError};
use chimera_core::{
    actor_v2::facade::{ReconcileReport, ReconcileResult},
    client::reconciler::plan::Trigger,
};
use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::api::core::v2::{OperationOutputInfo, ReconcileOutcomeKind};

use super::{
    committed::{Selection, SelectionChange},
    inputs::RuntimeInputs,
    mutation::{AppliedCandidate, ApplyOutcome},
    preparation::RuntimePreparation,
};
use crate::client::core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort};
use crate::client::runtime::RuntimeSnapshot;

pub use chimera_core::client::reconciler::apply::*;

impl chimera_core::client::reconciler::apply_executor::ApplyHost for CoreLifecycleWorkflow {
    fn convergence_authority(
        &self,
    ) -> Option<&dyn crate::client::reconciler::authority::RuntimeSourceAuthority> {
        CoreLifecycleWorkflow::convergence_authority(self)
    }
    fn convergence_candidate_check(
        &self,
    ) -> Option<&crate::client::reconciler::check::CandidateCheck> {
        CoreLifecycleWorkflow::convergence_candidate_check(self)
    }
    fn stage_prepared_outcome(&self, outcome: Option<ApplyOutcome>) {
        CoreLifecycleWorkflow::stage_prepared_outcome(self, outcome);
    }
    fn stage_connection_effect(&self, effect: CheckedConnectionEffect) {
        CoreLifecycleWorkflow::stage_connection_effect(self, effect);
    }
}

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn apply_checked_runtime(
        &self,
        prepared: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        domain: crate::client::reconciler::committed::SourceDomain,
        version: u64,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        interrupt_mode: bool,
    ) -> anyhow::Result<()> {
        chimera_core::client::reconciler::apply_executor::apply_checked_runtime(
            self,
            prepared,
            core_spec,
            domain,
            version,
            authority,
            interrupt_mode,
        )
        .await
    }

    /// Ref's prepared submission for an S2-selected and S3-proven Core owner.
    /// Startup and Service handoffs still use their explicit S2 transitions:
    /// a prepared submission may *not* select or retire a Core host.
    ///
    /// This returns None only before touching Core, when the canonical
    /// session or the selected Local owner is unavailable. A proven stopped
    /// Local owner has no old connections to interrupt; a live Local owner
    /// requires an instance-bound receipt; a prior selection may be unknown. After admission,
    /// all errors are returned to the one Actor as a real apply outcome;
    /// no second legacy submit may replay the candidate.
    pub(in crate::client) async fn try_reconcile_prepared_local(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        if run_type != crate::core::RunType::Normal {
            return None;
        }
        self.try_reconcile_prepared_selected(target, app, clash, run_type)
            .await
    }

    /// The selected Service endpoint uses the same exact PreparedRuntime
    /// product and Core manager operation as Local. Only S2-proven Service
    /// readiness can authorize admission; no host selection occurs here.
    pub(in crate::client) async fn try_reconcile_prepared_service(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
    ) -> Option<anyhow::Result<()>> {
        self.try_reconcile_prepared_selected(target, app, clash, crate::core::RunType::Service)
            .await
    }

    pub(in crate::client) async fn try_reconcile_prepared_selected(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        self.reconcile_prepared_selected_with_policy(target, app, clash, run_type, false)
            .await
    }

    pub(in crate::client) async fn try_reconcile_deferred_prepared(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
    ) -> Option<anyhow::Result<()>> {
        self.reconcile_prepared_selected_with_policy(target, app, clash, run_type, true)
            .await
    }

    async fn reconcile_prepared_selected_with_policy(
        &self,
        target: &str,
        app: &ChimeraAppConfig,
        clash: &chimera_config::clash::config::ClashConfig,
        run_type: crate::core::RunType,
        deferred: bool,
    ) -> Option<anyhow::Result<()>> {
        chimera_core::client::reconciler::apply_executor::reconcile_prepared_selected_with_policy(
            self, target, app, clash, run_type, deferred,
        )
        .await
    }
}

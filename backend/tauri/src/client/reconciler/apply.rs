//! Ref-aligned S4 candidate, Core submission, and post-apply maintenance.
//! The only Core command is submitted by the existing CoreLifecycle worker
//! through its sole CoreLifecyclePort; this module never creates a second
//! owner, revision allocator, RuntimeProduct writer, or action mailbox.
use std::{future::Future, sync::Arc};

use crate::client::{
    core_lifecycle::{CoreLifecycleWorkflow, ports::PreparedCoreSettlement},
    reconciler::{
        attempt::{CommittedTargetAdvanced, OwnershipTransitionPending},
        decision::{same_active_receipt, same_core_status, service_prepared_owner_proven},
    },
};
use chimera_config::application::ChimeraAppConfig;
use chimera_contracts::execution_host::ExecutionHost;
use chimera_contracts::runtime_error::{CoreFailure, RuntimeError};
use chimera_core::{
    actor_v2::facade::{ReconcileReport, ReconcileResult},
    client::reconciler::plan::Trigger,
};
use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::api::core::v2::{OperationOutputInfo, ReconcileOutcomeKind};
use chimera_ipc::api::status::CoreState;

use super::{
    committed::{Selection, SelectionChange},
    inputs::RuntimeInputs,
    mutation::{AppliedCandidate, ApplyOutcome},
    preparation::RuntimePreparation,
};
use crate::client::core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort};
use crate::client::runtime::RuntimeSnapshot;

pub use chimera_core::client::reconciler::apply::*;

impl CoreLifecycleWorkflow {
    /// Preserve the Core's exact terminal verdict through the sole lower-host
    /// operation registry. RolledBack is a completed refusal with the old
    /// revision still running; Unknown has a Core operation ID and must not
    /// be submitted again. Legacy ports keep their explicit Unobserved result.
    async fn submit_prepared_verdict(
        &self,
        prepared: PreparedRuntime,
        spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<PreparedCoreSettlement> {
        let (mapped, result) = crate::client::reconciler::apply::submit_prepared(
            self.convergence_core(),
            prepared,
            spec,
        )
        .await;
        // Only the single existing CoreLifecycle worker mutates the runtime.
        // Keep ref's observed terminal for the Actor's final source/receipt
        // fence, including rollback and unknown-operation evidence.
        self.stage_prepared_outcome(mapped);
        result
    }

    /// The sole Core owner's checked runtime execution. Carry the pre-apply
    /// source-instance API and Core replacement verdict through this one
    /// operation; never issue a second Core submit or interrupt a newly
    /// started instance.
    pub(in crate::client) async fn apply_checked_runtime(
        &self,
        prepared: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        domain: crate::client::reconciler::committed::SourceDomain,
        version: u64,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        interrupt_mode: bool,
    ) -> anyhow::Result<()> {
        let previous = self.convergence_progress().lock().selection();
        let result = apply_checked(
            self.convergence_core(),
            prepared,
            core_spec,
            domain,
            version,
            authority.as_ref(),
            interrupt_mode,
            previous.as_ref(),
            |mapped| self.stage_prepared_outcome(mapped),
        )
        .await;
        match result {
            Ok((selection, interruption, source, replaced)) => {
                self.convergence_progress().lock().set_selection(selection);
                // The worker cannot know about source notices that reached
                // the Actor after its final read. Defer the one-shot mode
                // effect to Completed; only that mailbox can settle it.
                if let Some(change) = interruption {
                    self.stage_connection_effect(CheckedConnectionEffect {
                        change,
                        replaced,
                        prior: source.unwrap_or(Ok(None)),
                    });
                }
                Ok(())
            }
            Err(error) => {
                self.convergence_progress().lock().invalidate_selection();
                Err(error)
            }
        }
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
        if self.convergence_core().outcome_uncertain() {
            return None;
        }
        let authority = self.convergence_authority()?;
        let gate = self.convergence_candidate_check()?;
        let before = self.convergence_core().status().await.ok()?;
        if before.run_type != run_type {
            return None;
        }
        // An unverified Service probe is not proof of a stopped Core. This
        // check is independent from the Core router's status projection.
        let service_before = if run_type == crate::core::RunType::Service {
            let (service, generation) = self.service_evidence().await;
            if !service_prepared_owner_proven(&before, &service) {
                return Some(Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "Service owner is not Ready or does not match the selected Core state",
                    )
                    .into(),
                ));
            }
            Some((service, generation))
        } else {
            None
        };
        let current_receipt = self
            .convergence_core()
            .active_runtime_receipt()
            .await
            .ok()?;
        let (receipt, previous) = match &before.state {
            CoreState::Running => {
                let receipt = current_receipt?;
                let expected_host = match run_type {
                    crate::core::RunType::Normal => ExecutionHost::Local,
                    crate::core::RunType::Service => ExecutionHost::Service,
                    crate::core::RunType::Elevated => return None,
                };
                if receipt.host != expected_host {
                    return None;
                }
                // Upstream selection.interruption accepts an unknown prior
                // selection. It must not block a valid live apply or invent
                // a Mode/Profile change when the old selection is unknown.
                (
                    Some(receipt),
                    self.convergence_progress().lock().selection(),
                )
            }
            CoreState::Stopped(_) if current_receipt.is_none() => (None, None),
            _ => return None,
        };

        let expected = match authority
            .verify_committed_prepared_target(target, previous.as_ref())
            .await
        {
            Ok(Some(expected)) => expected,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let result = async {
            let (candidate, core_spec) = if deferred {
                gate.prepare_deferred_committed_candidate(
                    app.clone(), clash.clone(), target, previous.as_ref(),
                ).await?
            } else {
                gate.prepare_committed_candidate(
                    app.clone(), clash.clone(), target, previous.as_ref(),
                ).await?
            };
            let current = authority
                .verify_committed_prepared_target(target, previous.as_ref())
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "committed source authority cannot prove the prepared target"
                    )
                })?;
            // ref's prepared S4 candidate carries selection and interruption
            // from the same frozen inputs as its product. The independent
            // source authority must agree before that candidate can submit.
            if current != expected
                || candidate.selection != expected.0
                || candidate.interruption != expected.1
            {
                return Err(anyhow::Error::new(CommittedTargetAdvanced).context(
                    "committed selection or interruption policy advanced during runtime build",
                ));
            }
            let observed = self.convergence_core().status().await?;
            if let Some((service_before, generation_before)) = service_before.as_ref() {
                let (service_after, generation_after) = self.service_evidence().await;
                if generation_after != *generation_before
                    || service_after != *service_before
                    || !service_prepared_owner_proven(&observed, &service_after)
                {
                    return Err(
                        crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                            "Service readiness or Core ownership changed while building the committed product",
                        )
                        .into(),
                    );
                }
            }
            let latest_receipt = self.convergence_core().active_runtime_receipt().await?;
            let still_receipted = same_active_receipt(receipt.as_ref(), latest_receipt.as_ref());
            if !same_core_status(&before, &observed) || !still_receipted {
                return Err(
                    crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "selected Core owner changed while preparing the committed product",
                    )
                    .into(),
                );
            }
            let old_api = if candidate.interruption.is_some() {
                Some(self.convergence_core().capture_interruption_source().await)
            } else {
                None
            };
            let final_owner = self.convergence_core().status().await?;
            let final_receipt = self.convergence_core().active_runtime_receipt().await?;
            if self.outcome_uncertain()
                || !same_core_status(&before, &final_owner)
                || !same_active_receipt(receipt.as_ref(), final_receipt.as_ref())
            {
                return Err(OwnershipTransitionPending::waiting(
                    "the selected Core changed immediately before S4 submission",
                ).into());
            }
            if authority.verify_committed_prepared_target(target, previous.as_ref()).await?
                .as_ref() != Some(&expected)
            {
                return Err(CommittedTargetAdvanced.into());
            }
            let applied = self
                .submit_prepared_verdict(candidate.runtime, core_spec)
                .await?;
            applied.validate_identity(target)?;
            let latest = authority
                .verify_committed_prepared_target(target, previous.as_ref())
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("committed source authority was lost after Core apply")
                })?;
            if latest != expected {
                return Err(anyhow::Error::new(CommittedTargetAdvanced)
                    .context("committed selection or policy advanced after prepared Core apply"));
            }
            self.convergence_progress().lock().set_selection(Some(candidate.selection));
            if let Some(change) = candidate.interruption {
                self.stage_connection_effect(CheckedConnectionEffect {
                    change,
                    replaced: applied.replaced,
                    prior: old_api.unwrap_or(Ok(None)),
                });
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            self.convergence_progress().lock().invalidate_selection();
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::runtime::RuntimeRevisionAllocator;
    use crate::client::{
        ports::SessionPortResolver, reconciler::ports::RuntimeBuildPort, runtime::RuntimeLifecycle,
    };
    use crate::state::mutation::{ProfileContentLock, ProfileContentReader};
    use chimera_config::application::ClashCore;
    use chimera_config::{
        application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
    };
    use chimera_contracts::execution_host::ExecutionHost;
    use chimera_core::actor_v2::{
        CoreStatusProjection, EndpointConnectivity,
        facade::{AppliedConfigBinding, RolledBackReport, UncertainReconcile},
    };
    use chimera_core::state::PersistentStateManagerSetup;
    use chimera_core_manager::OperationId;
    use chimera_ipc::api::{
        core::v2::{ReconcileOutcomeInfo, ReconcileOutcomeKind},
        status::ConfigRevisionInfo,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn ref_apply_without_proven_prepared_candidate_never_uses_legacy_core() {
        let CommittedPreparedDispatch::Observed(Err(error)) = route_committed_prepared(true, None)
        else {
            panic!("ref Step::Apply must not become an inline Core submit");
        };
        assert!(
            error
                .downcast_ref::<crate::client::reconciler::attempt::OwnershipTransitionPending>()
                .is_some()
        );
        assert!(crate::client::reconciler::attempt::waiting_dependency(
            &error
        ));
        assert!(
            error
                .to_string()
                .contains("legacy Core submit is forbidden")
        );
    }

    #[test]
    fn only_unproven_legacy_planning_can_rebuild_and_submit_inline() {
        assert!(matches!(
            route_committed_prepared(false, None),
            CommittedPreparedDispatch::LegacyRecovery
        ));
        assert!(matches!(
            route_committed_prepared(true, Some(Ok(()))),
            CommittedPreparedDispatch::Observed(Ok(()))
        ));
        let refused = route_committed_prepared(
            false,
            Some(Err(anyhow::anyhow!("candidate preparation refused"))),
        );
        assert!(matches!(
            refused,
            CommittedPreparedDispatch::Observed(Err(_))
        ));
    }

    fn product() -> Arc<RuntimeSnapshot> {
        Arc::new(RuntimeSnapshot::new(
            RuntimeRevisionAllocator::new().allocate().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            serde_yaml::Mapping::new(),
        ))
    }

    fn status() -> CoreStatusProjection {
        CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 3,
            connectivity: EndpointConnectivity::Connected,
            snapshot: None,
        }
    }

    fn revision() -> ConfigRevisionInfo {
        ConfigRevisionInfo {
            epoch: 1,
            generation: 2,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        }
    }

    fn checked_fixture(
        target: Option<&str>,
    ) -> crate::client::core_lifecycle::ports::PreparedRuntime {
        use chimera_core::actor_v2::intent::RuntimeIntentBuilder;
        use chimera_core_manager::{LocalIpcPolicy, LocalIpcSettings};
        let snapshot = product();
        let core_type = (&ClashCore::Mihomo).into();
        let intent = Arc::new(RuntimeIntentBuilder::build(
            core_type,
            Arc::from(std::str::from_utf8(snapshot.product_bytes()).unwrap()),
            LocalIpcSettings {
                policy: LocalIpcPolicy::Prefer,
                keep_http_controller: true,
            },
        ));
        crate::client::core_lifecycle::ports::PreparedRuntime {
            snapshot,
            intent,
            ports: crate::client::ports::CandidatePortBindings::default(),
            target: target.map(str::to_string),
        }
    }

    #[test]
    fn checked_product_is_consumed_only_for_the_exact_committed_target() {
        let mut checked = Some(checked_fixture(Some("candidate-a")));
        let snapshot = checked.as_ref().unwrap().snapshot.clone();
        assert!(take_checked_for_target(&mut checked, None).is_none());
        assert!(take_checked_for_target(&mut checked, Some("candidate-b")).is_none());
        assert!(
            checked.is_some(),
            "a mismatched check is not discarded or applied"
        );
        let accepted = take_checked_for_target(&mut checked, Some("candidate-a")).unwrap();
        assert!(Arc::ptr_eq(&accepted.snapshot, &snapshot));
        assert_eq!(
            accepted.intent.config_text.as_bytes(),
            accepted.snapshot.product_bytes()
        );
        assert!(checked.is_none());
        assert!(take_checked_for_target(&mut checked, Some("candidate-a")).is_none());
        let mut missing_target = Some(checked_fixture(None));
        assert!(take_checked_for_target(&mut missing_target, None).is_none());
        assert!(
            missing_target.is_some(),
            "absent identities cannot validate each other"
        );
    }

    #[test]
    fn prepared_applied_candidate_requires_exact_core_report_and_product_binding() {
        use crate::client::core_lifecycle::ports::{
            PreparedCoreSettlement, PreparedProductPublication, PreparedSubmission,
        };
        use chimera_core::actor_v2::endpoint::CoreStatusSnapshot;
        use chimera_core_manager::CoreKind;
        use chimera_ipc::api::status::CoreStateDetail;

        let product = product();
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.target = Some("committed-source-9".into());
        receipt.revision = product.revision;
        receipt.config_digest = chimera_core_manager::payload_digest(product.product_bytes());
        receipt.applied_revision = revision();
        receipt.controller_generation = Some(3);
        receipt.artifact = Some(product.clone());
        let receipt = Arc::new(receipt);
        let mut router_status = status();
        router_status.snapshot = Some(CoreStatusSnapshot {
            instance_id: Some("core-instance".into()),
            controller: None,
            state: Some(CoreStateDetail::Running { epoch: 1, pid: 27 }),
            state_changed_at: 14,
            revision: Some(revision().id()),
            source_hash: Some(revision().source_hash.clone()),
            healthy: Some(true),
            applied_kind: Some(CoreKind::Mihomo),
        });
        let outcome = ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::Started,
            revision: revision(),
            warning: Some("the Core applied with a durability warning".into()),
            failed_apply: None,
        };
        let report = ReconcileReport {
            applied: AppliedConfigBinding {
                revision: revision(),
                host: ExecutionHost::Local,
                generation: 3,
            },
            effective_config: None,
            output: OperationOutputInfo::Reconciled(outcome.clone()),
            status: router_status,
        };
        let candidate = PreparedCoreSettlement {
            report: Some(Box::new(report.clone())),
            outcome: Some(outcome),
            replaced: Some(true),
            confirmed: Some(receipt),
            product: Some(product.clone()),
            publication: PreparedProductPublication::Published,
        };
        candidate.validate_identity("committed-source-9").unwrap();
        match prepared_submission_outcome(&PreparedSubmission::Applied(candidate.clone())) {
            Some(ApplyOutcome::Applied(applied)) => {
                assert!(applied.replaced);
                assert!(Arc::ptr_eq(&applied.product, &product));
                assert_eq!(applied.report.output, report.output);
            }
            other => panic!(
                "real terminal Core proof must become the upstream AppliedCandidate: {other:?}"
            ),
        }
        let mut stale = candidate;
        stale.outcome.as_mut().unwrap().revision.generation += 1;
        assert!(
            prepared_submission_outcome(&PreparedSubmission::Applied(stale)).is_none(),
            "a mismatched Core terminal must not become an AppliedCandidate"
        );
    }

    #[test]
    fn prepared_terminal_outcomes_preserve_ref_rollback_unknown_and_unobserved() {
        use crate::client::core_lifecycle::ports::{
            PreparedCoreSettlement, PreparedProductPublication, PreparedSubmission,
        };
        let rollback = PreparedSubmission::RolledBack(ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::RolledBack,
            revision: revision(),
            warning: None,
            failed_apply: Some("old Core config was restored".into()),
        });
        assert!(matches!(
            prepared_submission_outcome(&rollback),
            Some(ApplyOutcome::Refused {cause})
            if matches!(cause.as_ref(), RuntimeError::CoreRolledBack {reason: Some(reason)}
                if reason == "old Core config was restored")
        ));

        let operation_id = OperationId::generate();
        let unknown = PreparedSubmission::Unknown(
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "lost operation terminal",
                true,
            )
            .with_operation(operation_id.clone()),
        );
        assert!(matches!(
            prepared_submission_outcome(&unknown),
            Some(ApplyOutcome::Unknown(cause))
            if matches!(cause.as_ref(), RuntimeError::SubmissionUnobserved {failure}
                if failure.kind == Some(CoreErrorKind::BackendUnavailable)
                    && failure.retryable
                    && failure.operation_id.as_deref() == Some(operation_id.to_string().as_str()))
        ));

        let compat = PreparedSubmission::Applied(PreparedCoreSettlement {
            report: None,
            outcome: None,
            replaced: None,
            confirmed: None,
            product: None,
            publication: PreparedProductPublication::Unobserved,
        });
        assert!(
            prepared_submission_outcome(&compat).is_none(),
            "the legacy Core bridge must not invent a ReconcileReport or AppliedCandidate"
        );
    }

    #[test]
    fn production_s4_terminal_preserves_rollback_unknown_and_unobserved() {
        use crate::client::core_lifecycle::ports::{
            PreparedCoreSettlement, PreparedProductPublication, PreparedRolledBack,
            PreparedSubmission,
        };
        let rollback = PreparedSubmission::RolledBack(ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::RolledBack,
            revision: revision(),
            warning: None,
            failed_apply: Some("rolled back to previous runtime".into()),
        });
        let (outcome, result) = settle_prepared_submission(rollback);
        assert!(matches!(
            outcome,
            Some(ApplyOutcome::Refused {cause})
                if matches!(cause.as_ref(), RuntimeError::CoreRolledBack {reason: Some(reason)}
                    if reason == "rolled back to previous runtime")
        ));
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<PreparedRolledBack>()
                .is_some()
        );

        let operation = OperationId::generate();
        let (outcome, result) = settle_prepared_submission(PreparedSubmission::Unknown(
            CoreError::new(CoreErrorKind::BackendUnavailable, "unknown operation", true)
                .with_operation(operation.clone()),
        ));
        assert!(matches!(
            outcome,
            Some(ApplyOutcome::Unknown(cause))
                if matches!(cause.as_ref(), RuntimeError::SubmissionUnobserved {failure}
                    if failure.operation_id.as_deref() == Some(operation.to_string().as_str()))
        ));
        assert_eq!(
            result
                .unwrap_err()
                .downcast_ref::<CoreError>()
                .and_then(|error| error.operation_id.as_ref()),
            Some(&operation),
        );

        let (outcome, result) =
            settle_prepared_submission(PreparedSubmission::Applied(PreparedCoreSettlement {
                report: None,
                outcome: None,
                replaced: None,
                confirmed: None,
                product: None,
                publication: PreparedProductPublication::Unobserved,
            }));
        assert!(
            outcome.is_none(),
            "an unobserved compatibility terminal cannot claim Applied"
        );
        assert!(
            result.is_ok(),
            "the legacy port retains its original Result semantics"
        );
    }

    #[tokio::test]
    async fn completion_does_not_interrupt_without_final_running_and_source_proof() {
        use chimera_contracts::runtime::{DegradationPhase, DegradationReason, InterruptFailure};
        let effect = || CheckedConnectionEffect {
            change: SelectionChange::Mode,
            replaced: Some(false),
            prior: Err(anyhow::Error::new(
                chimera_core::actor_v2::api::ApiError::Stale,
            )),
        };

        let stale = finish_connection_effect(effect(), &Ok(()), true, true)
            .await
            .expect("superseded completion must remain typed effect debt");
        assert!(matches!(
            stale.reason,
            DegradationReason::ModeInterruptionFailed {
                cause: InterruptFailure::Unavailable
            }
        ));
        assert_eq!(stale.phase, DegradationPhase::SystemEffect);
        assert!(!stale.retryable);
        assert!(stale.message.contains("superseded"));

        let unverified = finish_connection_effect(effect(), &Ok(()), false, false)
            .await
            .expect("unverified Running receipt must defer interruption");
        assert!(matches!(
            unverified.reason,
            DegradationReason::ModeInterruptionFailed {
                cause: InterruptFailure::Unavailable
            }
        ));
        let refused = finish_connection_effect(
            effect(),
            &Err(anyhow::anyhow!("Core refused candidate")),
            true,
            false,
        )
        .await
        .expect("rejected Core terminal cannot finish interruption");
        assert!(refused.message.contains("not settled"));
    }

    #[tokio::test]
    async fn confirmed_s4_effect_uses_previous_instance_and_typed_errors() {
        use chimera_contracts::runtime::{DegradationReason, InterruptFailure};
        let already_replaced = CheckedConnectionEffect {
            change: SelectionChange::Profile,
            replaced: Some(true),
            prior: Err(anyhow::anyhow!("old API already gone")),
        };
        assert!(
            finish_connection_effect(already_replaced, &Ok(()), true, false)
                .await
                .is_none(),
            "a replaced Core already retired the old connections"
        );
        let in_place = CheckedConnectionEffect {
            change: SelectionChange::Profile,
            replaced: Some(false),
            prior: Err(anyhow::Error::new(
                chimera_core::actor_v2::api::ApiError::Stale,
            )),
        };
        let failed = finish_connection_effect(in_place, &Ok(()), true, false)
            .await
            .expect("failed old-instance API must preserve ref's typed error");
        assert!(matches!(
            failed.reason,
            DegradationReason::ProfileInterruptionFailed {
                cause: InterruptFailure::Stale
            }
        ));

        let replacement_unknown = CheckedConnectionEffect {
            change: SelectionChange::Profile,
            replaced: None,
            prior: Ok(None),
        };
        let degraded = finish_connection_effect(replacement_unknown, &Ok(()), true, false)
            .await
            .expect("unknown replacement must never infer successful interruption");
        assert!(matches!(
            degraded.reason,
            DegradationReason::ProfileInterruptionFailed {
                cause: InterruptFailure::Unavailable
            }
        ));
    }

    #[test]
    fn not_submitted_waits_only_for_upstream_dependency_kinds() {
        for kind in [
            None,
            Some(CoreErrorKind::Internal),
            Some(CoreErrorKind::ConfigCheckFailed),
            Some(CoreErrorKind::ShuttingDown),
            Some(CoreErrorKind::RevisionConflict),
            Some(CoreErrorKind::OperationConflict),
            Some(CoreErrorKind::QueueFull),
            Some(CoreErrorKind::BackendUnavailable),
        ] {
            for retryable in [false, true] {
                let failure = CoreError {
                    kind,
                    message: "original failure".into(),
                    retryable,
                    operation_id: Some(OperationId::generate()),
                };
                let expected_id = failure.operation_id.unwrap().to_string();
                let outcome = not_submitted_outcome(failure);
                let waiting = matches!(
                    kind,
                    Some(
                        CoreErrorKind::RevisionConflict
                            | CoreErrorKind::OperationConflict
                            | CoreErrorKind::QueueFull
                            | CoreErrorKind::BackendUnavailable
                    )
                );
                let cause = match outcome {
                    ApplyOutcome::Waiting(cause) => {
                        assert!(waiting, "{kind:?}");
                        cause
                    }
                    ApplyOutcome::Refused { cause } => {
                        assert!(!waiting, "{kind:?}");
                        cause
                    }
                    outcome => panic!("unexpected classification: {outcome:?}"),
                };
                assert_eq!(cause.core_kind(), kind);
                assert_eq!(
                    cause.core_failure().unwrap().operation_id.as_deref(),
                    Some(expected_id.as_str())
                );
                assert_eq!(cause.core_failure().unwrap().retryable, retryable);
            }
        }
    }

    #[test]
    fn a_confirmed_reconcile_preserves_product_and_replacement_evidence() {
        for kind in [
            ReconcileOutcomeKind::Started,
            ReconcileOutcomeKind::Noop,
            ReconcileOutcomeKind::Restarted,
            ReconcileOutcomeKind::Switched,
        ] {
            let source = product();
            let report = ReconcileReport {
                applied: AppliedConfigBinding {
                    revision: revision(),
                    host: ExecutionHost::Local,
                    generation: 3,
                },
                effective_config: None,
                output: OperationOutputInfo::Reconciled(ReconcileOutcomeInfo {
                    outcome: kind,
                    revision: revision(),
                    warning: None,
                    failed_apply: None,
                }),
                status: status(),
            };
            let expected_replaced = matches!(
                kind,
                ReconcileOutcomeKind::Started
                    | ReconcileOutcomeKind::Restarted
                    | ReconcileOutcomeKind::Switched
            );
            match submission_outcome(ReconcileResult::Reconciled(report.clone()), source.clone()) {
                ApplyOutcome::Applied(applied) => {
                    assert!(Arc::ptr_eq(&applied.product, &source));
                    assert_eq!(applied.replaced, expected_replaced);
                    assert_eq!(applied.report.output, report.output);
                }
                outcome => panic!("confirmed reconcile was lost: {outcome:?}"),
            }
        }
    }

    #[test]
    fn rollback_and_unobserved_outcome_never_claim_apply_success() {
        let report = RolledBackReport {
            restored: AppliedConfigBinding {
                revision: revision(),
                host: ExecutionHost::Local,
                generation: 3,
            },
            failed_apply: Some("invalid candidate".into()),
            output: OperationOutputInfo::Stopped,
            status: status(),
        };
        assert!(
            matches!(submission_outcome(ReconcileResult::RolledBack(report), product()),
            ApplyOutcome::Refused {cause} if matches!(cause.as_ref(), RuntimeError::CoreRolledBack {reason: Some(reason)} if reason == "invalid candidate"))
        );
        let uncertainty = UncertainReconcile {
            error: CoreError::new(CoreErrorKind::BackendUnavailable, "lost reply", true)
                .with_operation(OperationId::generate()),
            status: status(),
        };
        assert!(
            matches!(submission_outcome(ReconcileResult::Unknown(uncertainty), product()),
            ApplyOutcome::Unknown(cause) if matches!(cause.as_ref(), RuntimeError::SubmissionUnobserved {failure} if failure.kind == Some(CoreErrorKind::BackendUnavailable) && failure.retryable && failure.operation_id.is_some()))
        );
        assert!(
            matches!(submission_outcome(ReconcileResult::Unchanged(CoreError::new(CoreErrorKind::ApplyFailed, "unchanged", false)), product()),
            ApplyOutcome::Refused {cause} if matches!(cause.as_ref(), RuntimeError::ApplyRuntime {failure} if failure.kind == Some(CoreErrorKind::ApplyFailed)))
        );
    }

    #[derive(Default)]
    struct CountingBuildPort(AtomicUsize);

    #[async_trait::async_trait]
    impl RuntimeBuildPort for CountingBuildPort {
        async fn capture_content(
            &self,
            _: &Profiles,
        ) -> super::super::inputs::FrozenProfileContent {
            Default::default()
        }
        fn core_spec(
            &self,
            core: &ClashCore,
        ) -> Result<chimera_core_manager::CoreSpec, chimera_core::client::core_spec::CoreSpecError>
        {
            chimera_core::client::core_spec::resolve_with(core, |_, _| {
                Ok(std::path::PathBuf::from("/tmp/ref-core"))
            })
        }
        async fn build(
            &self,
            revision: crate::client::runtime::RuntimeRevision,
            inputs: RuntimeInputs,
            _: chimera_config::runtime::executor::ResolvedPortBindings,
            _: bool,
        ) -> Result<Arc<RuntimeSnapshot>, chimera_core::client::runtime_build::RuntimeBuildError>
        {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(RuntimeSnapshot::new(
                revision,
                inputs.app.core,
                b"mode: rule\n".to_vec(),
                serde_yaml::Mapping::new(),
            )))
        }
        async fn publish(
            &self,
            _: &RuntimeSnapshot,
        ) -> Result<(), chimera_contracts::publish::PublishRuntimeError> {
            panic!("a candidate preparation must not publish runtime")
        }
    }

    async fn build_preparation() -> (RuntimePreparation, Arc<CountingBuildPort>, RuntimeInputs) {
        let temp = tempfile::tempdir().unwrap();
        let manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(
                camino::Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap(),
            )
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let builder = Arc::new(CountingBuildPort::default());
        let lifecycle = Arc::new(RuntimeLifecycle::default());
        let preparation = RuntimePreparation::new(
            ProfileContentReader::new(manager.snapshot_handle(), ProfileContentLock::default()),
            builder.clone(),
            crate::client::ports::RuntimePreparationSession {
                revisions: lifecycle.revision_allocator(),
                ports: Arc::new(SessionPortResolver::new(lifecycle)),
            },
        );
        let inputs = RuntimeInputs {
            app: ChimeraAppConfig {
                core: ClashCore::Mihomo,
                ..Default::default()
            },
            clash: ClashConfig::default(),
            profiles: Arc::new(Profiles::default()),
            content: Default::default(),
        };
        (preparation, builder, inputs)
    }

    #[tokio::test]
    async fn upstream_prepare_candidate_reuses_checked_product_only_for_exact_target() {
        let (preparation, builder, inputs) = build_preparation().await;
        let mut checked = Some(checked_fixture(inputs.target_key().as_deref()));
        let original = checked.as_ref().unwrap().snapshot.clone();
        let accepted = prepare_candidate(&preparation, &mut checked, None, inputs.clone())
            .await
            .unwrap_or_else(|outcome| panic!("checked candidate was not accepted: {outcome:?}"));
        assert!(Arc::ptr_eq(&accepted.runtime.snapshot, &original));
        assert!(checked.is_none());
        assert!(accepted.interruption.is_none());
        assert_eq!(builder.0.load(Ordering::Relaxed), 0);

        let mut stale = Some(checked_fixture(Some("stale-target")));
        let accepted = prepare_candidate(&preparation, &mut stale, None, inputs)
            .await
            .unwrap_or_else(|outcome| panic!("fresh target was not rebuilt: {outcome:?}"));
        assert_eq!(builder.0.load(Ordering::Relaxed), 1);
        assert_eq!(accepted.runtime.snapshot.product_bytes(), b"mode: rule\n");
        assert!(
            stale.is_some(),
            "mismatched checked document must not be consumed"
        );
    }
}

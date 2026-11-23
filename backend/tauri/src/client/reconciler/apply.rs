//! The pinned-ref submission verdict boundary, adapted to Chimera's existing
//! `CoreFacade::ReconcileResult`. This module does not send Core commands.
//! Once the one Reconciler mailbox owns `CoreFacade`, it will consume these
//! verdicts from the same `PreparedRuntime` validated before the source commit.
use std::sync::Arc;

use chimera_contracts::runtime_error::{CoreFailure, RuntimeError};
use chimera_core::actor_v2::facade::{ReconcileReport, ReconcileResult};
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

/// Ref's exact source-carrying conversion (the Core manager owns retryable
/// and operation_id; the application must never guess either).
fn core_failure(error: CoreError) -> CoreFailure {
    CoreFailure {
        kind: error.kind,
        message: error.message,
        retryable: error.retryable,
        operation_id: error.operation_id.map(|operation| operation.to_string()),
    }
}

/// Pinned ref's `PreparedCandidate` preserves the selected profile/mode and
/// interruption decision; it has not yet submitted anything to the Core.
pub(super) struct PreparedCandidate {
    pub runtime: PreparedRuntime,
    pub selection: Selection,
    pub interruption: Option<SelectionChange>,
}

/// Pinned ref `RuntimeReconciler::prepare_candidate` body with its owner
/// dependencies explicit, pending the single Actor mailbox cutover.
pub(super) async fn prepare_candidate(
    preparation: &RuntimePreparation,
    checked: &mut Option<PreparedRuntime>,
    applied_selection: Option<&Selection>,
    inputs: RuntimeInputs,
) -> Result<PreparedCandidate, ApplyOutcome> {
    let selection = Selection::of(&inputs);
    let interruption = selection.interruption(applied_selection, &inputs.clash);
    let target = inputs.target_key();
    let runtime = match take_checked_for_target(checked, target.as_deref()) {
        Some(prepared) => prepared,
        None => {
            preparation
                .prepare_inputs(inputs)
                .await
                .map_err(|error| ApplyOutcome::Refused {
                    cause: Arc::new(RuntimeError::new(error.to_string())),
                })?
        }
    };
    preparation
        .core_spec(&runtime.snapshot.target_core)
        .map_err(|source| ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::ResolveCoreBinary { source }),
        })?;
    Ok(PreparedCandidate {
        runtime,
        selection,
        interruption,
    })
}

/// The exact `checked` reuse fence from pinned ref
/// `RuntimeReconciler::prepare_candidate`: use a prechecked document only when
/// its target is present and exactly matches fresh committed input identity.
/// Mismatches leave the checked candidate intact while the caller rebuilds;
/// they never turn a failed/absent check into an implicit success.
pub(super) fn take_checked_for_target(
    checked: &mut Option<crate::client::core_lifecycle::ports::PreparedRuntime>,
    target: Option<&str>,
) -> Option<crate::client::core_lifecycle::ports::PreparedRuntime> {
    if target.is_some()
        && checked
            .as_ref()
            .is_some_and(|prepared| prepared.target.as_deref() == target)
    {
        checked.take()
    } else {
        None
    }
}

// Pinned ref/client/reconciler/apply.rs::not_submitted_outcome.
// Only four dependency kinds are waiting, regardless of retryable hint.
pub(super) fn not_submitted_outcome(error: CoreError) -> ApplyOutcome {
    let waiting = matches!(
        error.kind,
        Some(
            CoreErrorKind::RevisionConflict
                | CoreErrorKind::OperationConflict
                | CoreErrorKind::QueueFull
                | CoreErrorKind::BackendUnavailable
        )
    );
    let error = Arc::new(RuntimeError::ApplyRuntime {
        failure: core_failure(error),
    });
    if waiting {
        ApplyOutcome::Waiting(error)
    } else {
        ApplyOutcome::Refused { cause: error }
    }
}

fn replaced_runtime(report: &ReconcileReport) -> bool {
    matches!(
        &report.output,
        OperationOutputInfo::Reconciled(outcome)
            if matches!(
                outcome.outcome,
                ReconcileOutcomeKind::Started
                    | ReconcileOutcomeKind::Restarted
                    | ReconcileOutcomeKind::Switched
            )
    )
}

/// Ref's `submit_prepared` outcome projection against Chimera's existing Core
/// facade. This is a classification of evidence, *not* a submission, an ACK,
/// or a producer of the published runtime document.
pub(super) fn submission_outcome(
    result: ReconcileResult,
    product: Arc<RuntimeSnapshot>,
) -> ApplyOutcome {
    match result {
        ReconcileResult::Reconciled(report) => ApplyOutcome::Applied(AppliedCandidate {
            replaced: replaced_runtime(&report),
            product,
            report: Box::new(report),
        }),
        ReconcileResult::RolledBack(report) => ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::CoreRolledBack {
                reason: report.failed_apply,
            }),
        },
        ReconcileResult::Unknown(uncertain) => {
            ApplyOutcome::Unknown(Arc::new(RuntimeError::SubmissionUnobserved {
                failure: core_failure(uncertain.error),
            }))
        }
        ReconcileResult::NotSubmitted(error) => not_submitted_outcome(error),
        ReconcileResult::Unchanged(error) => ApplyOutcome::Refused {
            cause: Arc::new(RuntimeError::ApplyRuntime {
                failure: core_failure(error),
            }),
        },
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

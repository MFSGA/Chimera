//! StartupReconcile and owner reestablishment orchestration for ApplicationWorkflow.
//!
//! CoreLifecycleWorkflow supplies typed Core/Service adapter operations.
//! Startup evidence and owner decisions live here so setup cannot silently use
//! ordinary reconciliation when another host may still own a core.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
use crate::client::convergence::{OutcomeClass, RetryBudget, next_wait};
use crate::client::{
    application_workflow::{
        attempt::{AttemptOrigin, AttemptStage, LiveAttempt},
        mutation::{DeferredTarget, ReestablishCause, RuntimeCheckFailure, TargetOrigin},
        policy::TryCauseKind,
    },
    convergence::ConvergenceHealth,
    core_lifecycle::{CoreLifecycleWorkflow, ServiceTransitionLease, ports::CoreStatusSnapshot},
};
use chimera_core::{
    actor_v2::{
        endpoint::ExecutionHost, service_policy::ServicePhase, service_status::ServiceHostStatus,
    },
    client::reconciler::plan::{
        OwnerPlan, ServiceEvidence as ReconcilerServiceEvidence, Step,
        plan_owner as reconcile_plan_owner,
    },
};
use chimera_core_manager::{CoreError, CoreErrorKind, CoreOwner, Epoch, OperationId, RevisionId};
use chimera_ipc::{
    api::status::{ConfigRevisionInfo, CoreState},
    types::ServiceStatus,
};

use super::workflow::ApplicationWorkflow;

#[cfg(test)]
pub use chimera_core::client::reconciler::startup_policy::StartupRetryTarget;
pub use chimera_core::client::reconciler::startup_policy::{
    RuntimeEvidence, ServiceEvidence, StartupObservation, StartupOutcome, StartupOwnerFailure,
    StartupReport, StartupWorkflowState, effective_host, host_for_run_type,
    owner_followup_required, plan_owner, reconciler_service_evidence, runtime_evidence, s2_step,
    startup_failure_is_retryable, startup_failure_waits_for_dependency, startup_owner_outcome,
    startup_service_fallback_refused,
};

impl ApplicationWorkflow {
    async fn startup_reconcile_once(
        &mut self,
        operation_id: u64,
        allow_receipt_reuse: bool,
    ) -> StartupReport {
        use crate::core::actor_v2::control_endpoint::ExecutionHost;

        let lifecycle = self.lifecycle_handle();
        self.startup.mark_owner_unproven();
        let app = lifecycle.startup_application_config();
        let current = match lifecycle.startup_core_status().await {
            Ok(current) => current,
            Err(error) => {
                return StartupReport {
                    operation_id,
                    observation: None,
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: format!("the current Core owner could not be read: {error:#}"),
                    },
                };
            }
        };
        let initial_owner = host_for_run_type(current.run_type);
        let runtime = Some(runtime_evidence(&current));

        if lifecycle.outcome_uncertain() {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired: effective_host(app.enable_service_mode, None, initial_owner),
                    service: None,
                    runtime,
                }),
                outcome: StartupOutcome::RecoveryRequired {
                    reason: "a previous Core mutation has an uncertain outcome; startup will not issue another mutation".into(),
                },
            };
        }

        if !lifecycle.startup_service_command_settled().await {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired: effective_host(app.enable_service_mode, None, initial_owner),
                    service: None,
                    runtime,
                }),
                outcome: StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason: "a Service command started earlier is still running".into(),
                },
            };
        }

        let (service, service_ready_generation) = lifecycle.service_evidence().await;
        let (service, service_ready_generation) = match lifecycle
            .recover_service_error_stop(service.clone(), service_ready_generation)
            .await
        {
            Ok(recovered) => recovered,
            Err(failure) => {
                return self.startup_owner_failure_report(
                    operation_id,
                    StartupObservation {
                        desired: effective_host(
                            app.enable_service_mode,
                            service.phase().readiness(),
                            initial_owner,
                        ),
                        service: Some(service),
                        runtime,
                    },
                    failure,
                );
            }
        };
        let current = match lifecycle.startup_core_status().await {
            Ok(current) => current,
            Err(error) => {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired: effective_host(
                            app.enable_service_mode,
                            service.phase().readiness(),
                            initial_owner,
                        ),
                        service: Some(service),
                        runtime: None,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: format!("the Core owner could not be refreshed: {error:#}"),
                    },
                };
            }
        };
        let initial_owner = host_for_run_type(current.run_type);
        let runtime = Some(runtime_evidence(&current));
        let desired = crate::client::reconciler::workflow::desired_host(
            app.enable_service_mode,
            &service,
            service_ready_generation,
            lifecycle.startup_failed_generation(),
            initial_owner,
        );

        // The legacy Core status projection uses timestamp zero when the
        // selected Service endpoint could not answer. Do not read its
        // synthetic Stopped state as a proof that the owner is idle.
        if initial_owner == ExecutionHost::Service
            && runtime.as_ref().is_some_and(|r| r.state.is_none())
        {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired,
                    service: Some(service),
                    runtime,
                }),
                outcome: StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason: "the Service host did not publish a current Core state".into(),
                },
            };
        }

        let plan = plan_owner(desired, initial_owner, &service);
        let mut current = current;
        let mut service = service;
        match plan {
            OwnerPlan::WaitForService { phase } => {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: format!(
                            "the Service host is {phase:?}; startup waits for readiness"
                        ),
                    },
                };
            }
            OwnerPlan::Unproven { phase } => {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::RecoveryRequired,
                        reason: format!(
                            "the Service host ({phase:?}) may still hold a Core, so startup did not start another host"
                        ),
                    },
                };
            }
            OwnerPlan::RetireServiceThenLocal => {
                service = match lifecycle
                    .retire_service_startup_owner(&service, false)
                    .await
                {
                    Ok(service) => service,
                    Err(failure) => {
                        return self.startup_owner_failure_report(
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            failure,
                        );
                    }
                };
                current = match lifecycle.startup_core_status().await {
                    Ok(current) => current,
                    Err(error) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime: None,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "the Local Core owner could not be refreshed after Service retirement: {error:#}"
                                ),
                            },
                        };
                    }
                };
            }
            OwnerPlan::HandBackToLocal => {
                match &service {
                    ServiceEvidence::CoreRunning { .. } => {
                        service = match lifecycle
                            .retire_service_startup_owner(&service, false)
                            .await
                        {
                            Ok(service) => service,
                            Err(failure) => {
                                return self.startup_owner_failure_report(
                                    operation_id,
                                    StartupObservation {
                                        desired,
                                        service: Some(service),
                                        runtime,
                                    },
                                    failure,
                                );
                            }
                        };
                    }
                    ServiceEvidence::Absent { .. }
                    | ServiceEvidence::CoreStopped { .. }
                    | ServiceEvidence::ForeignCore { .. } => {}
                    ServiceEvidence::CoreStoppedWithReason { .. }
                    | ServiceEvidence::Unreadable { .. } => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::RecoveryRequired {
                                reason: "the Service Core owner is unreadable; startup will not stop Service or start Local".into(),
                            },
                        };
                    }
                }
                if let Err(error) = lifecycle
                    .startup_select_execution_host(ExecutionHost::Local)
                    .await
                {
                    return self
                        .startup_failure(
                            &lifecycle,
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            error,
                        )
                        .await;
                }
                current = match lifecycle.startup_core_status().await {
                    Ok(current) => current,
                    Err(error) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime: None,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "the Local Core owner could not be read after handback: {error:#}"
                                ),
                            },
                        };
                    }
                };
            }
            OwnerPlan::Owned => {
                if initial_owner == ExecutionHost::Service
                    && matches!(
                        &service,
                        ServiceEvidence::Unreadable { .. } | ServiceEvidence::ForeignCore { .. }
                    )
                {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::RecoveryRequired {
                            reason: "the current Service Core owner is not verified; startup did not submit a Service mutation".into(),
                        },
                    };
                }
                if initial_owner == ExecutionHost::Service {
                    let state_matches_service = matches!(
                        (
                            &service,
                            runtime.as_ref().and_then(|runtime| runtime.state.as_ref())
                        ),
                        (
                            ServiceEvidence::CoreRunning { .. },
                            Some(CoreState::Running)
                        ) | (
                            ServiceEvidence::CoreStopped { .. },
                            Some(CoreState::Stopped(_))
                        )
                    );
                    if !state_matches_service {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: "the Service probe and Core status do not describe the same settled owner state".into(),
                            },
                        };
                    }
                }
            }
            OwnerPlan::AdoptService => {
                if matches!(
                    &service,
                    ServiceEvidence::ForeignCore { .. } | ServiceEvidence::Unreadable { .. }
                ) {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::RecoveryRequired {
                            reason:
                                "the Service Core owner is not verified; startup did not adopt it"
                                    .into(),
                        },
                    };
                }
                if matches!(current.state, CoreState::Running) {
                    if let Err(failure) = lifecycle.confirm_local_owner_stopped(&current).await {
                        return self.startup_owner_failure_report(
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            failure,
                        );
                    }
                } else if current.state_changed_at > 0
                    && let Err(reason) = lifecycle.confirm_local_stopped_baseline(&current).await
                {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        }),
                        outcome: StartupOutcome::ReadyDegraded {
                            health:
                                crate::client::convergence::ConvergenceHealth::WaitingDependency,
                            reason,
                        },
                    };
                }
                if let Err(error) = lifecycle
                    .startup_select_execution_host(ExecutionHost::Service)
                    .await
                {
                    return self
                        .startup_failure(
                            &lifecycle,
                            operation_id,
                            StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            },
                            error,
                        )
                        .await;
                }
                current = match lifecycle.startup_core_status().await {
                    Ok(current) => current,
                    Err(error) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime: None,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "the Service Core owner could not be read after adoption: {error:#}"
                                ),
                            },
                        };
                    }
                };
                let (fresh_service, _) = lifecycle.service_evidence().await;
                service = fresh_service;
                if matches!(
                    &service,
                    ServiceEvidence::ForeignCore { .. } | ServiceEvidence::Unreadable { .. }
                ) {
                    return StartupReport {
                        operation_id,
                        observation: Some(StartupObservation {
                            desired,
                            service: Some(service),
                            runtime: Some(runtime_evidence(&current)),
                        }),
                        outcome: StartupOutcome::RecoveryRequired {
                            reason: "the adopted Service Core owner changed or is not verified"
                                .into(),
                        },
                    };
                }
            }
        };

        let owner = host_for_run_type(current.run_type);
        let runtime = Some(runtime_evidence(&current));
        if owner != desired {
            return StartupReport {
                operation_id,
                observation: Some(StartupObservation {
                    desired,
                    service: Some(service),
                    runtime,
                }),
                outcome: StartupOutcome::RecoveryRequired {
                    reason: format!(
                        "startup handoff selected {owner:?}, but the committed target requires {desired:?}"
                    ),
                },
            };
        }
        if owner == ExecutionHost::Service {
            let state_matches_service = matches!(
                (
                    &service,
                    runtime.as_ref().and_then(|runtime| runtime.state.as_ref())
                ),
                (
                    ServiceEvidence::CoreRunning { .. },
                    Some(CoreState::Running)
                ) | (
                    ServiceEvidence::CoreStopped { .. },
                    Some(CoreState::Stopped(_))
                )
            );
            if !state_matches_service {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason: "the Service probe and selected Core status do not describe the same settled owner state".into(),
                    },
                };
            }
        }

        // Recovery uses the shared S3/S4 executor after S2 has proved
        // either selected owner. The existing Service handback branch below
        // consumes its refusal while this same transition lease is held.
        let recovery_target = if allow_receipt_reuse
            && desired == ExecutionHost::Service
            && lifecycle.canonical_convergence_session()
            && current.state_changed_at > 0
        {
            lifecycle.startup_target_identity().await.ok()
        } else {
            None
        };
        let canonical_recovery = if allow_receipt_reuse && current.state_changed_at > 0 {
            lifecycle.recover_committed_startup(&current).await
        } else {
            None
        };

        let used_canonical_recovery = canonical_recovery.is_some();
        let mut result = if let Some(result) = canonical_recovery {
            if !lifecycle.outcome_uncertain()
                && !result
                    .as_ref()
                    .err()
                    .is_some_and(startup_failure_waits_for_dependency)
            {
                self.startup.mark_owner_proven();
            }
            result
        } else {
            // Every settled owner plan now enters the same S3 receipt/retirement
            // checks before the committed target is applied in S4.
            let is_running = runtime
                .as_ref()
                .is_some_and(|runtime| matches!(runtime.state.as_ref(), Some(CoreState::Running)));
            if is_running {
                match lifecycle
                    .recovery_receipt_matches(&current, desired, allow_receipt_reuse)
                    .await
                {
                    Ok(true) => {
                        self.startup.mark_owner_proven();
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::Ready,
                        };
                    }
                    Ok(false) => {}
                    Err(reason) => {
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason,
                            },
                        };
                    }
                }

                let retired = if desired == ExecutionHost::Service {
                    match lifecycle.retire_service_startup_owner(&service, true).await {
                        Ok(fresh_service) => {
                            service = fresh_service;
                            Ok(())
                        }
                        Err(failure) => Err(failure),
                    }
                } else {
                    lifecycle.confirm_local_owner_stopped(&current).await
                };
                if let Err(failure) = retired {
                    return self.startup_owner_failure_report(
                        operation_id,
                        StartupObservation {
                            desired,
                            service: Some(service),
                            runtime,
                        },
                        failure,
                    );
                }
            } else if desired == ExecutionHost::Local
                && runtime.as_ref().is_some_and(|runtime| {
                    runtime.state_changed_at > 0
                        && matches!(runtime.state.as_ref(), Some(CoreState::Stopped(_)))
                })
                && let Err(reason) = lifecycle.confirm_local_stopped_baseline(&current).await
            {
                return StartupReport {
                    operation_id,
                    observation: Some(StartupObservation {
                        desired,
                        service: Some(service),
                        runtime,
                    }),
                    outcome: StartupOutcome::ReadyDegraded {
                        health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                        reason,
                    },
                };
            }

            self.startup.mark_owner_proven();
            lifecycle
                .startup_reconcile_application(
                    lifecycle.startup_application_config(),
                    desired == ExecutionHost::Service,
                )
                .await
        };

        if desired == ExecutionHost::Service
            && startup_service_fallback_refused(&result, lifecycle.outcome_uncertain())
        {
            let failed_owner = lifecycle
                .startup_core_status()
                .await
                .ok()
                .map(|current| host_for_run_type(current.run_type));
            if failed_owner == Some(ExecutionHost::Service)
                && lifecycle.startup_service_refusal_stopped().await
            {
                // A Core submission can succeed and then fail to publish
                // RuntimeProduct. Never release a Service host still running
                // that accepted Core, even if the outer result is Refused.
                if used_canonical_recovery {
                    let (fresh_service, fresh_generation) = lifecycle.service_evidence().await;
                    if fresh_generation != service_ready_generation
                        || !matches!(
                            &fresh_service,
                            ServiceEvidence::CoreStopped {
                                ready: true,
                                runtime_owned: true,
                                ..
                            }
                        )
                        || recovery_target.is_none()
                        || lifecycle.startup_target_identity().await.ok() != recovery_target
                    {
                        self.startup.mark_owner_unproven();
                        return self.finish_startup_report(
                            &lifecycle, operation_id,
                            StartupObservation { desired, service: Some(fresh_service), runtime },
                            Err(crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                                "the refused Service target or owner changed before startup handback",
                            ).into()),
                        );
                    }
                }
                match lifecycle.release_service_owner().await {
                    Ok(fresh_service) => {
                        lifecycle.set_startup_failed_generation(service_ready_generation);
                        self.startup.mark_owner_proven();
                        result = lifecycle
                            .startup_reconcile_application_after_service_release(
                                lifecycle.startup_application_config(),
                            )
                            .await;
                        return self.finish_startup_report(
                            &lifecycle,
                            operation_id,
                            StartupObservation {
                                desired: ExecutionHost::Local,
                                service: Some(fresh_service),
                                runtime,
                            },
                            result,
                        );
                    }
                    Err(reason) => {
                        self.startup.mark_owner_unproven();
                        return StartupReport {
                            operation_id,
                            observation: Some(StartupObservation {
                                desired,
                                service: Some(service),
                                runtime,
                            }),
                            outcome: StartupOutcome::ReadyDegraded {
                                health:
                                    crate::client::convergence::ConvergenceHealth::WaitingDependency,
                                reason: format!(
                                    "Service was ready but its Core did not reconcile, and Local fallback could not prove release: {reason}"
                                ),
                            },
                        };
                    }
                }
            }
        }

        self.finish_startup_report(
            &lifecycle,
            operation_id,
            StartupObservation {
                desired,
                service: Some(service),
                runtime,
            },
            result,
        )
    }

    fn finish_startup_report(
        &self,
        lifecycle: &CoreLifecycleWorkflow,
        operation_id: u64,
        observation: StartupObservation,
        result: anyhow::Result<()>,
    ) -> StartupReport {
        let outcome = match result {
            Ok(()) => StartupOutcome::Ready,
            Err(error) if lifecycle.outcome_uncertain() => {
                self.startup.mark_owner_unproven();
                StartupOutcome::RecoveryRequired {
                    reason: format!(
                        "startup Core reconciliation has an uncertain outcome: {error:#}"
                    ),
                }
            }
            Err(error) if startup_failure_waits_for_dependency(&error) => {
                StartupOutcome::ReadyDegraded {
                    health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                    reason: format!(
                        "startup Core reconciliation is waiting for a dependency: {error:#}"
                    ),
                }
            }
            Err(error) if startup_failure_is_retryable(&error) => StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                reason: format!("startup Core reconciliation failed transiently: {error:#}"),
            },
            Err(error) => StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                reason: format!("startup Core reconciliation failed: {error:#}"),
            },
        };
        if outcome == StartupOutcome::Ready
            && (observation.desired
                == crate::core::actor_v2::control_endpoint::ExecutionHost::Service
                || !lifecycle.startup_application_config().enable_service_mode)
        {
            lifecycle.set_startup_failed_generation(0);
        }
        StartupReport {
            operation_id,
            observation: Some(observation),
            outcome,
        }
    }

    async fn startup_failure(
        &self,
        lifecycle: &CoreLifecycleWorkflow,
        operation_id: u64,
        observation: StartupObservation,
        error: anyhow::Error,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        let outcome = if lifecycle.outcome_uncertain() {
            StartupOutcome::RecoveryRequired {
                reason: format!("startup owner transition has an uncertain outcome: {error:#}"),
            }
        } else if startup_failure_waits_for_dependency(&error) {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
                reason: format!("startup owner transition is waiting for a dependency: {error:#}"),
            }
        } else if startup_failure_is_retryable(&error) {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                reason: format!("startup owner transition failed transiently: {error:#}"),
            }
        } else {
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::Blocked,
                reason: format!("startup owner transition failed: {error:#}"),
            }
        };
        StartupReport {
            operation_id,
            observation: Some(observation),
            outcome,
        }
    }

    fn startup_owner_failure_report(
        &self,
        operation_id: u64,
        observation: StartupObservation,
        failure: StartupOwnerFailure,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        StartupReport {
            operation_id,
            observation: Some(observation),
            outcome: startup_owner_outcome(failure),
        }
    }

    fn begin_reestablish_attempt(&mut self, cause: ReestablishCause) {
        if let Some(attempt) = &mut self.live {
            if let Some(target) = attempt.target_mut() {
                target.origin = TargetOrigin::Reestablish(cause);
                attempt.stage = if cause == ReestablishCause::Recovery {
                    AttemptStage::Recovering
                } else {
                    AttemptStage::Preparing
                };
                attempt.unresolved = None;
                return;
            }
            debug_assert!(false, "an unresolved attempt is never replaced");
            return;
        }

        let mut target = self.deferred.take().unwrap_or_else(|| {
            DeferredTarget::reestablish(OperationId::generate(), String::new(), cause)
        });
        target.origin = TargetOrigin::Reestablish(cause);
        let operation_id = target.operation_id;
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
    }

    async fn update_reestablish_attempt(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        report: StartupReport,
        automatic: bool,
        cause: ReestablishCause,
    ) -> StartupReport {
        let target = self
            .live
            .as_ref()
            .and_then(|attempt| match &attempt.origin {
                AttemptOrigin::CommittedTarget(target) => Some(target.as_ref().clone()),
                AttemptOrigin::SourceDecision { .. } | AttemptOrigin::Lifecycle => None,
            })
            .expect("startup reestablishment runs on its committed target");
        let mut deferred = Some(target);
        let report =
            update_reestablish_target(lifecycle, report, automatic, cause, &mut deferred).await;
        if let Some(target) = deferred
            && let Some(live_target) = self.live.as_mut().and_then(LiveAttempt::target_mut)
        {
            *live_target = target;
        }

        match &report.outcome {
            StartupOutcome::Ready => {
                self.live = None;
                self.deferred = None;
            }
            StartupOutcome::RecoveryRequired { reason } | StartupOutcome::Unsettled { reason } => {
                if let Some(attempt) = &mut self.live {
                    attempt.unresolved = Some(reason.clone());
                }
                self.deferred = None;
                self.publish_recovery_context().await;
            }
            StartupOutcome::ReadyDegraded { .. } => {
                self.conclude_attempt(None);
            }
        }
        report
    }

    pub(in crate::client) async fn startup_reconcile(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        if let Some(report) = &self.startup_report {
            return report.clone();
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Startup);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Startup)
            .await;
        self.startup_report = Some(report.clone());
        report
    }

    pub(in crate::client) async fn recover_startup(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::Recovery);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::Recovering;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, true).await;
        let report = self
            .update_reestablish_attempt(&lifecycle, report, false, ReestablishCause::Recovery)
            .await;
        if self.startup_report.is_none() {
            self.startup_report = Some(report.clone());
        }
        report
    }

    pub(in crate::client) async fn retry_startup(
        &mut self,
        operation_id: u64,
        identity: String,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> StartupReport {
        let target = self.deferred.as_ref().filter(|target| {
            target.is_reestablish()
                && target.identity == identity
                && target.health == ConvergenceHealth::Pending
        });
        let Some(target) = target else {
            return StartupReport {
                operation_id,
                observation: None,
                outcome: StartupOutcome::Ready,
            };
        };
        let TargetOrigin::Reestablish(cause) = target.origin else {
            unreachable!("filtered reestablish target")
        };
        let allow_receipt_reuse = target.allows_receipt_reuse();

        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(cause);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self
            .startup_reconcile_once(operation_id, allow_receipt_reuse)
            .await;
        self.update_reestablish_attempt(&lifecycle, report, true, cause)
            .await
    }

    pub(in crate::client) async fn service_host_change_due(&self) -> bool {
        if self.startup_report.is_none() || self.live.is_some() || self.startup.owner_unproven() {
            return false;
        }
        if self.deferred.as_ref().is_some_and(|target| {
            target.is_reestablish()
                && (matches!(
                    target.health,
                    crate::client::convergence::ConvergenceHealth::Blocked
                        | crate::client::convergence::ConvergenceHealth::RecoveryRequired
                ) || target
                    .next_attempt
                    .is_some_and(|next_attempt| next_attempt > tokio::time::Instant::now()))
        }) {
            return false;
        }
        let lifecycle = self.lifecycle();
        if lifecycle.outcome_uncertain() {
            return false;
        }
        let app = lifecycle.startup_application_config();
        let Ok(current) = lifecycle.startup_core_status().await else {
            return false;
        };
        // Ref's recovery-priority rule is evaluated by plan(Facts), rather
        // than a separate S2 readiness shortcut.
        let recovering = self.service_recovery_blocks_handoff();
        // Explicit RunIntent is already enforced by CoreLifecycle's sole
        // mailbox before this method. Here the ref S2 planner decides whether
        // a host transition is owed; ready Service evidence can require
        // retiring another owned Core even when Local is already selected.
        let owner = host_for_run_type(current.run_type);
        let (service, generation) = lifecycle.service_evidence().await;
        // S2 cannot queue a host handoff from a single stale Core/Service
        // pairing. Verify that both independently owned facts stayed fixed
        // across the asynchronous readiness observation, just as S3/S4 do.
        let Ok(resampled_core) = lifecycle.startup_core_status().await else {
            return false;
        };
        let (resampled_service, resampled_generation) = lifecycle.service_evidence().await;
        let Ok(final_core) = lifecycle.startup_core_status().await else {
            return false;
        };
        if !crate::client::reconciler::decision::stable_owner_facts(
            &current,
            &resampled_core,
            &service,
            generation,
            &resampled_service,
            resampled_generation,
        ) || !crate::client::reconciler::decision::same_core_status(&current, &final_core)
        {
            return false;
        }
        let desired = crate::client::reconciler::workflow::desired_host(
            app.enable_service_mode,
            &service,
            generation,
            lifecycle.startup_failed_generation(),
            owner,
        );
        owner_followup_required(desired, owner, &service, recovering)
    }

    pub(in crate::client) async fn follow_service(
        &mut self,
        operation_id: u64,
        _transition: Box<dyn ServiceTransitionLease>,
    ) -> anyhow::Result<()> {
        if !self.service_host_change_due().await {
            return Ok(());
        }
        self.startup.mark_owner_unproven();
        self.begin_reestablish_attempt(ReestablishCause::ServiceReadiness);
        if let Some(attempt) = &mut self.live {
            attempt.stage = AttemptStage::TryingCritical;
        }
        let lifecycle = self.lifecycle_handle();
        let report = self.startup_reconcile_once(operation_id, false).await;
        let report = self
            .update_reestablish_attempt(
                &lifecycle,
                report,
                false,
                ReestablishCause::ServiceReadiness,
            )
            .await;
        super::workflow::startup_report_result(&report)
    }

    pub(super) async fn refresh_startup_retry_target_after_reconcile(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
        result: &anyhow::Result<()>,
    ) {
        let Some(target) = self
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if result.is_ok() {
            self.clear_startup_retry_target();
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };

        let error = result
            .as_ref()
            .expect_err("failed reconciliation has an error");
        let outcome = if lifecycle.outcome_uncertain() {
            StartupOutcome::RecoveryRequired {
                reason: format!("runtime target reconciliation is uncertain: {error:#}"),
            }
        } else if startup_failure_waits_for_dependency(error) {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::WaitingDependency,
                reason: format!(
                    "runtime target reconciliation is waiting for a dependency: {error:#}"
                ),
            }
        } else if startup_failure_is_retryable(error) {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::RetryScheduled,
                reason: format!("runtime target reconciliation failed transiently: {error:#}"),
            }
        } else {
            StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason: format!("runtime target reconciliation failed: {error:#}"),
            }
        };
        let report = StartupReport {
            operation_id: 0,
            observation: None,
            outcome,
        };
        update_reestablish_target(lifecycle, report, false, cause, &mut self.deferred).await;
    }

    pub(super) async fn reschedule_startup_retry_after_committed_config_patch(
        &mut self,
        lifecycle: &CoreLifecycleWorkflow,
    ) {
        let Some(target) = self
            .deferred
            .as_ref()
            .filter(|target| target.is_reestablish())
        else {
            return;
        };
        if target.health == ConvergenceHealth::RecoveryRequired {
            return;
        }
        let cause = match target.origin {
            TargetOrigin::Reestablish(cause) => cause,
            TargetOrigin::Mutation => unreachable!("filtered reestablish target"),
        };
        let identity = match lifecycle.startup_target_identity().await {
            Ok(identity) => identity,
            Err(_) => match lifecycle.unresolved_startup_target_identity().await {
                Some(identity) => identity,
                None => return,
            },
        };
        let target = self
            .deferred
            .as_mut()
            .filter(|target| target.is_reestablish())
            .expect("reestablish target remains attached to the workflow");
        if target.identity != identity {
            *target = DeferredTarget::reestablish(OperationId::generate(), identity, cause);
        } else if target.attempts_remaining == 0 {
            return;
        }
        target.health = ConvergenceHealth::RetryScheduled;
        target.next_attempt = Some(tokio::time::Instant::now());
    }
}

async fn update_reestablish_target(
    lifecycle: &CoreLifecycleWorkflow,
    report: StartupReport,
    automatic: bool,
    cause: ReestablishCause,
    deferred: &mut Option<DeferredTarget>,
) -> StartupReport {
    chimera_core::client::reconciler::reestablish::update_reestablish_target(
        || lifecycle.startup_target_identity(),
        || lifecycle.unresolved_startup_target_identity(),
        report,
        automatic,
        cause,
        deferred,
    )
    .await
}

#[cfg(test)]
pub(in crate::client) async fn update_startup_retry_target(
    lifecycle: &CoreLifecycleWorkflow,
    mut report: StartupReport,
    automatic: bool,
    allow_receipt_reuse: bool,
    retry_target: &mut Option<StartupRetryTarget>,
) -> StartupReport {
    match &report.outcome {
        StartupOutcome::Ready => {
            *retry_target = None;
            return report;
        }
        StartupOutcome::RecoveryRequired { .. } | StartupOutcome::Unsettled { .. } => {
            if let Some(target) = retry_target.as_mut() {
                target.health = ConvergenceHealth::RecoveryRequired;
                target.next_attempt = None;
            }
            return report;
        }
        StartupOutcome::ReadyDegraded { .. } => {}
    }

    let identity = match lifecycle.startup_target_identity().await {
        Ok(identity) => identity,
        Err(error) => {
            let reason = format!(
                "{}; committed runtime target identity is unavailable: {error:#}",
                report
                    .outcome
                    .failure_reason()
                    .unwrap_or("startup did not converge")
            );
            if let Some(identity) = lifecycle.unresolved_startup_target_identity().await {
                if retry_target
                    .as_ref()
                    .is_none_or(|target| target.identity != identity)
                {
                    *retry_target = Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
                }
                if let Some(target) = retry_target.as_mut() {
                    report.outcome = target.schedule_dependency(reason);
                    return report;
                }
            }
            report.outcome = StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason,
            };
            return report;
        }
    };
    if retry_target
        .as_ref()
        .is_none_or(|target| target.identity != identity)
    {
        *retry_target = Some(StartupRetryTarget::new(identity, allow_receipt_reuse));
    }
    let target = retry_target
        .as_mut()
        .expect("startup retry target was initialized");
    target.allow_receipt_reuse = allow_receipt_reuse;
    let reason = report
        .outcome
        .failure_reason()
        .unwrap_or("startup did not converge")
        .to_owned();
    report.outcome = match report.outcome {
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::WaitingDependency | ConvergenceHealth::RecoveryRequired,
            ..
        } => target.schedule_dependency(reason),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::RetryScheduled,
            ..
        } => target.schedule_application(reason, automatic),
        StartupOutcome::ReadyDegraded {
            health: ConvergenceHealth::Blocked,
            ..
        }
        | StartupOutcome::ReadyDegraded { .. } => target.block(reason),
        StartupOutcome::Ready
        | StartupOutcome::RecoveryRequired { .. }
        | StartupOutcome::Unsettled { .. } => {
            unreachable!("handled above")
        }
    };
    report
}

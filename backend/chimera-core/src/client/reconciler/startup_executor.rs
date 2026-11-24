//! Serialized startup executor. Runs within the sole reconciler worker.
//! The host supplies only the existing selected-Core and Service operations;
//! owner retirement, proof, fallback admission and reporting are shared.
use super::{
    plan::OwnerPlan,
    proof,
    startup_policy::{
        ServiceEvidence, StartupObservation, StartupOutcome, StartupOwnerFailure, StartupReport,
        StartupWorkflowState, effective_host, host_for_run_type, owner_followup_required,
        plan_owner, runtime_evidence, startup_failure_is_retryable,
        startup_failure_waits_for_dependency, startup_owner_outcome,
        startup_service_fallback_refused,
    },
};
use crate::{
    actor_v2::endpoint::ExecutionHost,
    client::{
        convergence::ConvergenceHealth,
        core_lifecycle::{ownership, ports::CoreLifecyclePort, status::CoreStatusSnapshot},
    },
};
use chimera_config::application::ChimeraAppConfig;
use chimera_ipc::api::status::CoreState;

#[async_trait::async_trait]
pub trait StartupHost: Send + Sync {
    type Core: CoreLifecyclePort + ?Sized;
    fn core(&self) -> &Self::Core;
    fn startup_application_config(&self) -> ChimeraAppConfig;
    fn startup_failed_generation(&self) -> u64;
    fn set_startup_failed_generation(&self, generation: u64);
    fn canonical_convergence_session(&self) -> bool;
    fn progress(&self) -> &parking_lot::Mutex<super::state::RuntimeProgress>;
    async fn fallback_after_service_refusal(
        &self,
        target: &str,
        refusal: anyhow::Error,
    ) -> anyhow::Result<()>;

    async fn startup_service_command_settled(&self) -> bool;
    async fn service_evidence(&self) -> (ServiceEvidence, u64);
    async fn startup_select_execution_host(&self, host: ExecutionHost) -> anyhow::Result<()>;
    async fn startup_target_identity(&self) -> anyhow::Result<String>;
    async fn recover_committed_startup(
        &self,
        observed: &CoreStatusSnapshot,
    ) -> Option<anyhow::Result<()>>;
    async fn startup_reconcile_application(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()>;
    async fn release_service_owner(&self) -> Result<ServiceEvidence, String>;
    async fn startup_reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
        expected_target: Option<&str>,
    ) -> anyhow::Result<()>;
    fn service_recovery_blocks_handoff(&self) -> bool;
}

pub struct StartupExecutor<'a, H: StartupHost + ?Sized> {
    host: &'a H,
    startup: &'a StartupWorkflowState,
}
impl<'a, H: StartupHost + ?Sized> StartupExecutor<'a, H> {
    pub fn new(host: &'a H, startup: &'a StartupWorkflowState) -> Self {
        Self { host, startup }
    }
    pub async fn run(&self, operation_id: u64, allow_receipt_reuse: bool) -> StartupReport {
        use crate::actor_v2::endpoint::ExecutionHost;

        let lifecycle = self.host;
        let core = lifecycle.core();
        self.startup.mark_owner_unproven();
        let app = lifecycle.startup_application_config();
        let current = match core.status().await {
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

        if lifecycle.core().outcome_uncertain() {
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
        let (service, service_ready_generation) = match ownership::recover_service_error_stop(
            core,
            || lifecycle.service_evidence(),
            service.clone(),
            service_ready_generation,
        )
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
        let current = match core.status().await {
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
                service = match ownership::retire_service_startup_owner(
                    core,
                    || lifecycle.service_evidence(),
                    &service,
                    false,
                )
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
                current = match core.status().await {
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
                        service = match ownership::retire_service_startup_owner(
                            core,
                            || lifecycle.service_evidence(),
                            &service,
                            false,
                        )
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
                current = match core.status().await {
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
                    if let Err(failure) =
                        ownership::confirm_local_owner_stopped(core, &current).await
                    {
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
                    && let Err(reason) =
                        ownership::confirm_local_stopped_baseline(core, &current).await
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
                current = match core.status().await {
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
        let recovery_target =
            if desired == ExecutionHost::Service && lifecycle.canonical_convergence_session() {
                lifecycle.startup_target_identity().await.ok()
            } else {
                None
            };
        let canonical_recovery = if allow_receipt_reuse && current.state_changed_at > 0 {
            lifecycle.recover_committed_startup(&current).await
        } else {
            None
        };

        let mut result = if let Some(result) = canonical_recovery {
            if !lifecycle.core().outcome_uncertain()
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
                match proof::recovery_receipt_matches(
                    core,
                    || lifecycle.startup_target_identity(),
                    &current,
                    desired,
                    allow_receipt_reuse,
                )
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
                    match ownership::retire_service_startup_owner(
                        core,
                        || lifecycle.service_evidence(),
                        &service,
                        true,
                    )
                    .await
                    {
                        Ok(fresh_service) => {
                            service = fresh_service;
                            Ok(())
                        }
                        Err(failure) => Err(failure),
                    }
                } else {
                    ownership::confirm_local_owner_stopped(core, &current).await
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
                && let Err(reason) = ownership::confirm_local_stopped_baseline(core, &current).await
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
            && startup_service_fallback_refused(&result, lifecycle.core().outcome_uncertain())
        {
            let failed_owner = core
                .status()
                .await
                .ok()
                .map(|current| host_for_run_type(current.run_type));
            if failed_owner == Some(ExecutionHost::Service)
                && proof::startup_service_refusal_stopped(core).await
            {
                // A Core submission can succeed and then fail to publish
                // RuntimeProduct. Never release a Service host still running
                // that accepted Core, even if the outer result is Refused.
                if lifecycle.canonical_convergence_session() {
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
                            operation_id,
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
                                recovery_target.as_deref(),
                            )
                            .await;
                        return self.finish_startup_report(
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
        operation_id: u64,
        observation: StartupObservation,
        result: anyhow::Result<()>,
    ) -> StartupReport {
        let lifecycle = self.host;
        let outcome = match result {
            Ok(()) => StartupOutcome::Ready,
            Err(error) if lifecycle.core().outcome_uncertain() => {
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
            && (observation.desired == crate::actor_v2::endpoint::ExecutionHost::Service
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
        operation_id: u64,
        observation: StartupObservation,
        error: anyhow::Error,
    ) -> StartupReport {
        self.startup.mark_owner_unproven();
        let lifecycle = self.host;
        let outcome = if lifecycle.core().outcome_uncertain() {
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

    pub async fn service_host_change_due(&self) -> bool {
        let lifecycle = self.host;
        let core = lifecycle.core();
        if core.outcome_uncertain() {
            return false;
        }
        let app = lifecycle.startup_application_config();
        let Ok(current) = core.status().await else {
            return false;
        };
        // Ref's recovery-priority rule is evaluated by plan(Facts), rather
        // than a separate S2 readiness shortcut.
        let recovering = lifecycle.service_recovery_blocks_handoff();
        // Explicit RunIntent is already enforced by CoreLifecycle's sole
        // mailbox before this method. Here the ref S2 planner decides whether
        // a host transition is owed; ready Service evidence can require
        // retiring another owned Core even when Local is already selected.
        let owner = host_for_run_type(current.run_type);
        let (service, generation) = lifecycle.service_evidence().await;
        // S2 cannot queue a host handoff from a single stale Core/Service
        // pairing. Verify that both independently owned facts stayed fixed
        // across the asynchronous readiness observation, just as S3/S4 do.
        let Ok(resampled_core) = core.status().await else {
            return false;
        };
        let (resampled_service, resampled_generation) = lifecycle.service_evidence().await;
        let Ok(final_core) = core.status().await else {
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
}

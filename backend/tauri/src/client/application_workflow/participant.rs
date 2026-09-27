//! The per-mutation Required participant of a Profile source transaction.
//!
//! The domain actor owns persistence. This participant admits one candidate to
//! Chimera's serialized CoreLifecycle boundary and restores the previous
//! runtime if that source transaction rolls back.

use std::{
    collections::HashMap, marker::PhantomData, panic::AssertUnwindSafe, sync::Arc, time::Duration,
};

use chimera_config::profile::Profiles;
use chimera_core::state::{
    Ack, AckOptions, DecisionHandle, StateAckSubscriber, StateChange, StateDecision, SubscriberName,
};
use chimera_core_manager::{CoreError, CoreErrorKind, OperationId};
use futures_util::FutureExt;
use tokio::sync::{Mutex, Notify};

use super::{
    impact::{ActivationIntent, MutationHints, RuntimeImpact, classify_profiles},
    mutation::{DomainChange, MutationDomain},
    policy::{CommandClass, CommandPolicy, CoreRunIntent, policy_for},
};
use crate::client::{core_lifecycle::CoreLifecycleClient, runtime::RuntimeCommitStatus};

const MUTATION_ACK_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Default)]
struct RuntimeAttempt {
    result: Mutex<Option<Result<(), String>>>,
    completed: Notify,
}

impl RuntimeAttempt {
    async fn complete(&self, result: Result<(), String>) {
        *self.result.lock().await = Some(result);
        self.completed.notify_waiters();
    }

    async fn wait(&self) -> anyhow::Result<()> {
        loop {
            let completed = self.completed.notified();
            tokio::pin!(completed);
            completed.as_mut().enable();
            if let Some(result) = self.result.lock().await.clone() {
                return result.map_err(anyhow::Error::msg);
            }
            completed.await;
        }
    }
}

pub(crate) struct ApplicationMutationParticipant<T: MutationDomain> {
    operation_id: OperationId,
    hints: MutationHints,
    class: CommandClass,
    decision: DecisionHandle,
    core: CoreLifecycleClient,
    outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
    recovery_required: Arc<Mutex<Option<String>>>,
    attempted_runtime: std::sync::atomic::AtomicBool,
    deferred_runtime: std::sync::atomic::AtomicBool,
    runtime_attempt: std::sync::Mutex<Option<Arc<RuntimeAttempt>>>,
    name: String,
    _state: PhantomData<T>,
}

impl<T: MutationDomain> ApplicationMutationParticipant<T> {
    pub(crate) fn new(
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        decision: DecisionHandle,
        core: CoreLifecycleClient,
        outcomes: Arc<Mutex<HashMap<OperationId, RuntimeCommitStatus>>>,
        recovery_required: Arc<Mutex<Option<String>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            name: format!("application-mutation/{operation_id}"),
            operation_id,
            hints,
            class,
            decision,
            core,
            outcomes,
            recovery_required,
            attempted_runtime: std::sync::atomic::AtomicBool::new(false),
            deferred_runtime: std::sync::atomic::AtomicBool::new(false),
            runtime_attempt: std::sync::Mutex::new(None),
            _state: PhantomData,
        })
    }

    async fn reconcile_candidate(&self, candidate: Arc<Profiles>) -> anyhow::Result<()> {
        let attempt = Arc::new(RuntimeAttempt::default());
        *self
            .runtime_attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(attempt.clone());
        self.attempted_runtime
            .store(true, std::sync::atomic::Ordering::Release);

        let wait_attempt = attempt.clone();
        let core = self.core.clone();
        let staged_content = self.hints.staged_content.clone();
        tauri::async_runtime::spawn(async move {
            let result = AssertUnwindSafe(core.reconcile_profiles(candidate, staged_content))
                .catch_unwind()
                .await;
            let result = match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("Profile runtime reconciliation task panicked".into()),
            };
            attempt.complete(result).await;
        });

        wait_attempt.wait().await
    }

    async fn recover_previous(&self, previous: &Profiles) -> anyhow::Result<()> {
        let Some(cause) = self.recovery_required.lock().await.clone() else {
            return Ok(());
        };
        anyhow::ensure!(
            !self.core.status().uncertain,
            "Profile runtime still needs recovery after rollback failure: {cause}"
        );
        let status = self.core.core_status().await?;
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            *self.recovery_required.lock().await = None;
            return Ok(());
        }
        self.core
            .reconcile_profiles(Arc::new(previous.clone()), Default::default())
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "Profile runtime recovery failed after rollback error ({cause}): {error}"
                )
            })?;
        *self.recovery_required.lock().await = None;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<T: MutationDomain> StateAckSubscriber<T> for ApplicationMutationParticipant<T> {
    fn name(&self) -> SubscriberName<'_> {
        SubscriberName(self.name.as_str().into())
    }

    fn ack_options(&self) -> AckOptions {
        AckOptions::required(MUTATION_ACK_TIMEOUT)
    }

    async fn on_prepare(&self, change: StateChange<T>) -> Ack {
        if self.decision.decision() != StateDecision::Undecided {
            return Ack::Rejected(
                "Profile source transaction settled before runtime admission".into(),
            );
        }

        let DomainChange::Profiles {
            previous,
            candidate,
        } = T::domain_change(change);
        let previous = previous
            .map(|state| state.as_ref().clone())
            .unwrap_or_default();
        let candidate = candidate.as_ref().clone();
        if let Err(error) = self.recover_previous(&previous).await {
            return Ack::Failed(error);
        }
        let impact = classify_profiles(&previous, &candidate, &self.hints);
        let explicit_switch = self.class == CommandClass::ExplicitSwitch
            || self.hints.activation != ActivationIntent::None;

        if !explicit_switch && impact == RuntimeImpact::None {
            self.outcomes
                .lock()
                .await
                .insert(self.operation_id.clone(), RuntimeCommitStatus::Unchanged);
            return Ack::Ok;
        }

        if self.core.status().uncertain {
            return Ack::Failed(anyhow::anyhow!(
                "Profile runtime mutation cannot proceed while the core lifecycle outcome is uncertain"
            ));
        }

        let status = match self.core.core_status().await {
            Ok(status) => status,
            Err(error) => return Ack::Failed(error),
        };
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            self.outcomes.lock().await.insert(
                self.operation_id.clone(),
                RuntimeCommitStatus::SavedInactive,
            );
            return Ack::Ok;
        }

        let policy = policy_for(
            self.class,
            impact,
            &Default::default(),
            CoreRunIntent::Running,
        );
        if matches!(
            policy,
            CommandPolicy::SaveOnly | CommandPolicy::SaveThenNotify
        ) {
            self.outcomes
                .lock()
                .await
                .insert(self.operation_id.clone(), RuntimeCommitStatus::Unchanged);
            return Ack::Ok;
        }

        match self.reconcile_candidate(Arc::new(candidate)).await {
            Ok(()) => {
                self.outcomes
                    .lock()
                    .await
                    .insert(self.operation_id.clone(), RuntimeCommitStatus::Applied);
                Ack::Ok
            }
            Err(error) if policy.allows_deferral() && safe_to_defer_runtime_apply(&error) => {
                // QueueFull proves this candidate was not admitted to the core
                // executor. Preserve the running baseline and retry from the
                // committed Profile snapshot after source commit.
                self.attempted_runtime
                    .store(false, std::sync::atomic::Ordering::Release);
                self.deferred_runtime
                    .store(true, std::sync::atomic::Ordering::Release);
                self.outcomes
                    .lock()
                    .await
                    .insert(self.operation_id.clone(), RuntimeCommitStatus::Deferred);
                Ack::Degraded(format!(
                    "Profile was saved with runtime apply deferred: {error}"
                ))
            }
            Err(error) => Ack::Failed(error),
        }
    }

    async fn on_committed(&self, _change: StateChange<T>) -> Ack {
        if self
            .deferred_runtime
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.core.request_runtime_rebuild();
        }
        Ack::Ok
    }

    async fn on_rolled_back(
        &self,
        change: StateChange<T>,
        _reason: chimera_core::state::RollbackReason,
    ) {
        if !self
            .attempted_runtime
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.outcomes.lock().await.remove(&self.operation_id);
            return;
        }

        let attempt = self
            .runtime_attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(attempt) = attempt {
            // A prepare ACK timeout stops the source transaction's wait, not
            // the runtime attempt. Settle it before queuing the baseline restore.
            if let Err(error) = attempt.wait().await {
                tracing::warn!(%error, operation_id = %self.operation_id, "profile runtime attempt ended before rollback");
            }
        }

        let DomainChange::Profiles { previous, .. } = T::domain_change(change);
        let Some(previous) = previous else {
            *self.recovery_required.lock().await =
                Some("no previous Profile snapshot is available for rollback".into());
            return;
        };
        if let Err(error) = self
            .core
            .reconcile_profiles(previous, Default::default())
            .await
        {
            let message = format!("failed to restore the previous Profile runtime: {error}");
            *self.recovery_required.lock().await = Some(message);
        }
        self.outcomes.lock().await.remove(&self.operation_id);
    }
}

fn safe_to_defer_runtime_apply(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::QueueFull) && error.retryable)
    })
}

#[cfg(test)]
mod attempt_tests {
    use super::*;

    #[tokio::test]
    async fn runtime_attempt_survives_a_cancelled_waiter() {
        let attempt = Arc::new(RuntimeAttempt::default());
        let worker_attempt = attempt.clone();
        let (release, wait) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = wait.await;
            worker_attempt.complete(Ok(())).await;
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(1), attempt.wait())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        attempt.wait().await.unwrap();
    }

    #[tokio::test]
    async fn runtime_attempt_preserves_worker_errors_for_rollback() {
        let attempt = RuntimeAttempt::default();
        attempt
            .complete(Err("candidate runtime failed".into()))
            .await;

        assert_eq!(
            attempt.wait().await.unwrap_err().to_string(),
            "candidate runtime failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_deferral_requires_a_retryable_queue_full_error() {
        let queue_full = anyhow::Error::new(CoreError::new(
            CoreErrorKind::QueueFull,
            "executor queue is full",
            true,
        ));
        assert!(safe_to_defer_runtime_apply(&queue_full));

        let non_retryable_queue_full = anyhow::Error::new(CoreError::new(
            CoreErrorKind::QueueFull,
            "queue full without retryability",
            false,
        ));
        assert!(!safe_to_defer_runtime_apply(&non_retryable_queue_full));

        let other_retryable_error = anyhow::Error::new(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "backend unavailable",
            true,
        ));
        assert!(!safe_to_defer_runtime_apply(&other_retryable_error));

        let untyped = anyhow::anyhow!("executor queue is full");
        assert!(!safe_to_defer_runtime_apply(&untyped));
    }
}

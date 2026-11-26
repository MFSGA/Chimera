//! Readiness observation for the existing RuntimeReconciler mailbox.
//! Production watches ref's ServiceActor; the compatibility watch is only
//! used for a port without that actor. No secondary publisher is created.
use super::service_evidence::{
    service_followup_evidence_changed, startup_recovery_evidence_changed,
};
use crate::{
    actor_v2::{
        service_policy::ServicePhase, service_router::ServiceHostStatus as ActorServiceStatus,
        service_status::ServiceHostStatus,
    },
    client::core_lifecycle::ports::{CoreLifecyclePort, ServiceLifecyclePort},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub fn project_actor_service_status_with_ownership(
    actor: &ActorServiceStatus,
    runtime_owned: bool,
) -> ServiceHostStatus {
    let phase = match (actor.phase, runtime_owned) {
        // ref ServiceActor gates on daemon compatibility. Chimera additionally
        // requires that the daemon's runtime directories match this instance.
        (ServicePhase::Ready, false) => ServicePhase::Incompatible,
        (phase, _) => phase,
    };
    ServiceHostStatus {
        name: actor.name.clone(),
        version: actor.version.clone(),
        status: actor.status,
        server: actor.server.clone(),
        phase,
        compat: actor.compat.clone(),
        runtime_owned,
        restart_attempts: actor.restart_attempts,
        settled_ready: actor.settled_ready && runtime_owned,
        ready_generation: actor.ready_generation,
    }
}

pub fn startup_recovery_needed(
    core: &dyn CoreLifecyclePort,
    service: &dyn ServiceLifecyclePort,
    startup_owner_unproven: &AtomicBool,
    service_failed_generation: &AtomicU64,
    legacy_status: &watch::Receiver<ServiceHostStatus>,
) -> bool {
    !core.outcome_uncertain()
        && (startup_owner_unproven.load(Ordering::Acquire) || {
            let failed_generation = service_failed_generation.load(Ordering::Acquire);
            // Compare with the same Actor generation that originally failed.
            let current = service
                .actor_status()
                .unwrap_or_else(|| legacy_status.borrow().clone());
            failed_generation != 0
                && current.ready_generation != failed_generation
                && current.phase.readiness() == Some(true)
        })
}

async fn observe_watch<T: Send + Sync>(
    mut updates: watch::Receiver<T>,
    owner_unproven: &AtomicBool,
    shutdown: &CancellationToken,
    project: impl Fn(&T) -> ServiceHostStatus,
    followup: impl Fn() -> bool,
    recovery: impl Fn(),
) {
    let mut previous = project(&updates.borrow_and_update());
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            changed = updates.changed() => {
                if changed.is_err() { break; }
                let current = project(&updates.borrow_and_update());
                let evidence_changed = startup_recovery_evidence_changed(
                    &previous, &current, owner_unproven.load(Ordering::Acquire),
                );
                let followup_changed = service_followup_evidence_changed(&previous, &current);
                previous = current;
                if followup_changed && !followup() { break; }
                if evidence_changed { recovery(); }
            }
        }
    }
}

pub async fn observe_service(
    actor_updates: Option<watch::Receiver<ActorServiceStatus>>,
    legacy_updates: watch::Receiver<ServiceHostStatus>,
    owner_unproven: Arc<AtomicBool>,
    shutdown: CancellationToken,
    project: impl Fn(&ActorServiceStatus) -> ServiceHostStatus + Send,
    followup: impl Fn() -> bool + Send,
    recovery: impl Fn() + Send,
) {
    if let Some(updates) = actor_updates {
        observe_watch(
            updates,
            &owner_unproven,
            &shutdown,
            project,
            followup,
            recovery,
        )
        .await;
    } else {
        observe_watch(
            legacy_updates,
            &owner_unproven,
            &shutdown,
            Clone::clone,
            followup,
            recovery,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[tokio::test]
    async fn observer_ignores_repeated_probes_and_wakes_for_new_generation() {
        let mut ready = ServiceHostStatus::probing();
        ready.phase = ServicePhase::Ready;
        ready.ready_generation = 5;
        let (tx, rx) = watch::channel(ready.clone());
        let (followup_tx, mut followup_rx) = tokio::sync::mpsc::unbounded_channel();
        let recoveries = Arc::new(AtomicUsize::new(0));
        let count = recoveries.clone();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(observe_service(
            None,
            rx,
            Arc::new(AtomicBool::new(false)),
            shutdown.clone(),
            |_| panic!("legacy watch cannot project a ref actor"),
            move || followup_tx.send(()).is_ok(),
            move || {
                count.fetch_add(1, Ordering::AcqRel);
            },
        ));
        tokio::task::yield_now().await;
        tx.send(ready.clone()).unwrap();
        tokio::task::yield_now().await;
        assert!(followup_rx.try_recv().is_err());
        assert_eq!(recoveries.load(Ordering::Acquire), 0);
        ready.ready_generation += 1;
        tx.send(ready).unwrap();
        tokio::time::timeout(Duration::from_secs(1), followup_rx.recv())
            .await
            .unwrap()
            .unwrap();
        shutdown.cancel();
        task.await.unwrap();
        assert_eq!(recoveries.load(Ordering::Acquire), 1);
    }
}

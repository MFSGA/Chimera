use std::{sync::Arc, time::Duration};

use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    epoch::Epoch,
    health::HealthPolicy,
    probe::{ProbeContext, ProbeHandle, ProbePhase, ProbeResult},
    spec::ResolvedController,
};

#[derive(Debug, Clone)]
pub(crate) struct ProbeObservation {
    pub(crate) run_id: u64,
    pub(crate) pid: u32,
    pub(crate) phase: ProbePhase,
    pub(crate) completed_at: std::time::Instant,
    pub(crate) completed_at_ms: i64,
    pub(crate) result: ProbeResult,
}

enum DriverCommand {
    UseLiveness,
    Reconcile {
        response: tokio::sync::oneshot::Sender<ProbeResult>,
    },
}

pub(crate) struct ProbeDriver {
    command_tx: mpsc::UnboundedSender<DriverCommand>,
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl ProbeDriver {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        epoch: Epoch,
        run_id: u64,
        pid: u32,
        controller: Arc<ResolvedController>,
        readiness: ProbeHandle,
        liveness: Option<ProbeHandle>,
        policy: HealthPolicy,
        observation_tx: mpsc::UnboundedSender<ProbeObservation>,
    ) -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            let mut periodic = Some((readiness.clone(), ProbePhase::Readiness));
            // Match the former `tokio::time::interval` behavior: the first
            // readiness attempt is immediate, then delay after each result.
            let mut next_probe = tokio::time::Instant::now();

            loop {
                tokio::select! {
                    biased;
                    _ = task_cancel.cancelled() => break,
                    command = command_rx.recv() => match command {
                        Some(DriverCommand::UseLiveness) => {
                            periodic = liveness
                                .clone()
                                .map(|probe| (probe, ProbePhase::Liveness));
                            next_probe = tokio::time::Instant::now() + policy.interval();
                        }
                        Some(DriverCommand::Reconcile { response }) => {
                            let probe = liveness.as_ref().unwrap_or(&readiness).clone();
                            if let Some(observation) = run_attempt(
                                &probe,
                                epoch,
                                run_id,
                                pid,
                                ProbePhase::Reconcile,
                                controller.clone(),
                                policy.timeout(),
                                &task_cancel,
                            ).await {
                                let _ = response.send(observation.result.clone());
                                let _ = observation_tx.send(observation);
                            }
                        }
                        None => break,
                    },
                    _ = tokio::time::sleep_until(next_probe), if periodic.is_some() => {
                        let (probe, phase) = periodic.as_ref().expect("guarded");
                        if let Some(observation) = run_attempt(
                            probe,
                            epoch,
                            run_id,
                            pid,
                            *phase,
                            controller.clone(),
                            policy.timeout(),
                            &task_cancel,
                        ).await {
                            let _ = observation_tx.send(observation);
                        }
                        next_probe = tokio::time::Instant::now() + policy.interval();
                    }
                }
            }
        });
        Self {
            command_tx,
            cancel,
            task: Some(task),
        }
    }

    pub(crate) fn use_liveness(&self) {
        let _ = self.command_tx.send(DriverCommand::UseLiveness);
    }

    pub(crate) fn reconcile(&self, response: tokio::sync::oneshot::Sender<ProbeResult>) {
        let _ = self.command_tx.send(DriverCommand::Reconcile { response });
    }

    pub(crate) async fn stop(mut self) {
        self.cancel.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for ProbeDriver {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_attempt(
    probe: &ProbeHandle,
    epoch: Epoch,
    run_id: u64,
    pid: u32,
    phase: ProbePhase,
    controller: Arc<ResolvedController>,
    timeout: Duration,
    driver_cancel: &CancellationToken,
) -> Option<ProbeObservation> {
    let attempt_cancel = driver_cancel.child_token();
    let context = ProbeContext {
        epoch,
        pid,
        phase,
        controller,
        cancel: attempt_cancel.clone(),
    };
    let mut future = probe.check(context);
    let result = tokio::select! {
        biased;
        _ = driver_cancel.cancelled() => {
            attempt_cancel.cancel();
            return None;
        }
        timed = tokio::time::timeout(timeout, &mut future) => match timed {
            Ok(result) => result,
            Err(_) => {
                attempt_cancel.cancel();
                ProbeResult::Unhealthy {
                    detail: Some(format!("probe timed out after {timeout:?}")),
                }
            }
        },
    };
    drop(future);
    Some(ProbeObservation {
        run_id,
        pid,
        phase,
        completed_at: std::time::Instant::now(),
        completed_at_ms: crate::state::now_ms(),
        result,
    })
}

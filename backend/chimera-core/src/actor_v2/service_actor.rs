//! ServiceActor: the daemon as a managed OS resource (integration design §6).
//!
//! Owns install/uninstall/start/stop/update of the privileged daemon —
//! serialized by the mailbox because elevated commands must never interleave —
//! plus health observation, the fail-closed version gate (its only
//! implementation point), endpoint supply, and a bounded auto-restart with an
//! exhaustion latch. It never touches core lifecycle: that belongs to the
//! `CoreControl` running *inside* the daemon.
//!
//! Desired state arrives by facade orchestration, never by config-watch
//! subscription — two owners racing one change was the audited 5d/5e failure
//! shape.
//!
//! The facade's host-transition lock coordinates only this app process. A
//! second app instance or CLI can still race before the daemon is stopped;
//! stopping it inside this mailbox turn is the point after which its control
//! plane cannot admit another reconcile.

use std::sync::Arc;

use chimera_contracts::service::{ParseServiceStatusSnafu, ServiceProcessControl};
use futures_util::future::BoxFuture;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use snafu::ResultExt as _;
use tokio::sync::watch;

use chimera_core_manager::{CoreError, CoreErrorKind};
use chimera_ipc::{
    api::status::CoreStateDetail,
    types::{ServiceStatus, StatusInfo},
};

use super::endpoint::EndpointHandle;
use crate::service::compat::ServiceCompat;
use chimera_contracts::service::{
    ServiceCommandError, StillRunningSnafu, TaskCancelledSnafu, TimedOutSnafu,
};

/// Constructs a control endpoint after the daemon passes the application's version gate.
#[cfg(test)]
#[path = "service_router_tests.rs"]
mod tests;

pub trait ServiceEndpointFactory: Send + Sync + 'static {
    fn endpoint(&self) -> EndpointHandle;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Probing,
    NotInstalled,
    DaemonStopped,
    Installing,
    StartingDaemon,
    Ready,
    /// Version gate failed closed: upgrade required, never downgraded to.
    Incompatible,
    Restarting,
    /// Auto-restart budget spent; waits for `EnsureReady` or a successful explicit start.
    Exhausted,
    Uninstalling,
    /// The probe itself failed or timed out (F5): what the daemon actually
    /// is cannot be determined. Never treated as `DaemonStopped` -- an
    /// unreachable daemon might still be holding a core open, so callers
    /// that gate on "no core held" (the uninstall guard, `RecoverEndpoint`'s
    /// restart) must refuse rather than proceed.
    Unknown,
}

impl ServicePhase {
    /// Whether this phase settles if the daemon can serve: `None` for a
    /// transition still under way and for a probe that could not tell.
    pub fn readiness(self) -> Option<bool> {
        match self {
            Self::Ready => Some(true),
            Self::NotInstalled | Self::DaemonStopped | Self::Incompatible | Self::Exhausted => {
                Some(false)
            }
            Self::Probing
            | Self::Installing
            | Self::StartingDaemon
            | Self::Restarting
            | Self::Uninstalling
            | Self::Unknown => None,
        }
    }
}

/// The watch projection (UI settings page + facade). Daemon state, never a
/// second copy of core state.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServiceHostStatus {
    pub name: std::borrow::Cow<'static, str>,
    pub version: std::borrow::Cow<'static, str>,
    pub status: ServiceStatus,
    pub server: Option<chimera_ipc::api::status::StatusResBody<'static>>,
    pub phase: ServicePhase,
    pub compat: ServiceCompat,
    pub restart_attempts: u8,
    /// The last settled [`ServicePhase::readiness`]: a transition or an
    /// unreadable probe leaves it as it was. Internal, like the generation.
    #[serde(skip)]
    pub settled_ready: bool,
    /// How many times `settled_ready` has turned true. A daemon that
    /// restarted between two reads is a new daemon even when both say
    /// `Ready`; this is what tells them apart. Internal: not part of the UI
    /// projection.
    #[serde(skip)]
    pub ready_generation: u64,
}

/// Which mutating command an elevated call belongs to. It names the helper
/// the actor still owns when that call outlives its bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCommandKind {
    EnsureReady,
    Install,
    Update,
    Uninstall,
    StartDaemon,
    StopDaemon,
    /// Chimera service CLI exposes a single native restart command.
    RestartDaemon,
    RecoverEndpoint,
}

pub enum ServiceActorMessage {
    /// Idempotent convergence to `Ready`: install → start → probe → gate, as
    /// needed. The reply carries the endpoint the CoreActor will route to.
    EnsureReady {
        reply: RpcReplyPort<Result<EndpointHandle, CoreError>>,
    },
    Install {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    Update {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    /// Guarded twice: the facade only reaches this after a handoff away from
    /// Service, and the actor itself refuses while the daemon owns a running
    /// core.
    Uninstall {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    StartDaemon {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    StopDaemon {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    /// Chimera extension: one elevated restart, not a separate stop + start.
    RestartDaemon {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    /// Adopt-only counterpart to `EnsureReady`: one probe, and the endpoint
    /// back only if that same probe says `Ready`. It never installs, starts,
    /// or updates, so a caller that must not raise a UAC prompt -- boot
    /// restoring a persisted host -- can ask for the daemon without ever
    /// converging one. Deciding from a cached phase and then calling
    /// `EnsureReady` is not the same thing: the daemon can stop in between,
    /// and the convergence would run.
    AdoptIfReady {
        reply: RpcReplyPort<Result<EndpointHandle, CoreError>>,
    },
    /// Explicit probe. It reports and never escapes: `Exhausted` is cleared
    /// only by `EnsureReady` or a successful explicit start, matching the
    /// variant's own doc.
    Probe {
        reply: RpcReplyPort<ServiceHostStatus>,
    },
    /// Recover a failed endpoint without installing or upgrading a daemon.
    RecoverEndpoint {
        reply: RpcReplyPort<Result<EndpointHandle, CoreError>>,
    },
    /// Whether every command this actor accepted has finished. The mailbox
    /// answers it only after the commands queued ahead of it, and `false`
    /// while a helper that outlived its bound is still running. A stable
    /// phase is not this evidence, and neither is `Exhausted`.
    CommandSettled { reply: RpcReplyPort<bool> },
    /// Whether the OS reports the daemon not running -- stopped or not
    /// installed. A daemon that is not running cannot be holding a core open,
    /// so this is the proof a handoff away from an unreachable Service host
    /// needs. `false` covers "running" and "cannot tell" alike, and it is
    /// the answer while a command helper may still be changing the daemon.
    DaemonNotRunning { reply: RpcReplyPort<bool> },
    /// Turns the periodic health check on or off. It runs only while service
    /// mode is preferred, so the daemon is not probed for a user who never
    /// asked for it.
    SetHealthCheck { enabled: bool },
    /// One health-check probe, sent by the timer `SetHealthCheck` arms. It
    /// catches a daemon that stopped or came back while nothing else probed:
    /// it never installs, starts or updates one. `round` names the timer that
    /// sent it, so a tick from a check turned off since is dropped.
    HealthCheck { round: u64 },
}

pub struct ServiceActor;

/// Elevated commands are slow (UAC + SCM) but must still be bounded (F6): a
/// wedged daemon must not hang the actor's single, serialized mailbox
/// forever. Production policy; tests inject a short bound instead so a hang
/// test does not wait on this wall clock (see `ServiceClient::spawn_with_bounds`).
const DEFAULT_SERVICE_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(100);

/// How often the health check probes while it is on. A probe is one
/// unelevated `status` query.
const HEALTH_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

pub struct ServiceActorArgs {
    pub process_control: Arc<dyn ServiceProcessControl>,
    pub endpoint_factory: Arc<dyn ServiceEndpointFactory>,
    pub status_tx: watch::Sender<ServiceHostStatus>,
    /// Auto-restart attempts before the exhaustion latch (design OQ-6; the
    /// number is host policy, injected).
    pub restart_budget: u8,
    /// Bound applied to every adapter call the actor makes, `probe` included
    /// (F6).
    pub command_timeout: std::time::Duration,
    pub health_interval: std::time::Duration,
}

pub struct ServiceActorState {
    process_control: Arc<dyn ServiceProcessControl>,
    endpoint_factory: Arc<dyn ServiceEndpointFactory>,
    status_tx: watch::Sender<ServiceHostStatus>,
    restart_budget: u8,
    restart_attempts: u8,
    /// The auto-restart budget is spent. A latch and not a comparison: the
    /// counter alone is cleared by anything that resets it, and a probe that
    /// happens to find the daemon up would then publish `Ready` for a host
    /// nobody has re-armed.
    exhausted: bool,
    command_timeout: std::time::Duration,
    /// A mutating adapter call that outlived `command_timeout`. An elevated
    /// command is not cancelled by dropping its future, so the helper keeps
    /// running and stays owned here until it ends; until then nothing that
    /// hands out the endpoint, and no other mutating command, is served.
    outstanding: Option<(
        ServiceCommandKind,
        tokio::task::JoinHandle<Result<(), ServiceCommandError>>,
    )>,
    health_interval: std::time::Duration,
    /// The round of the health check while it is on. Each probe arms the
    /// next one only after it finished, so a slow probe never piles ticks up
    /// in the mailbox.
    health_round: Option<u64>,
    health_rounds: u64,
    health_timer:
        Option<tokio::task::JoinHandle<Result<(), ractor::MessagingErr<ServiceActorMessage>>>>,
}

impl ServiceActorState {
    /// The settled readiness and its generation that publishing `phase` over
    /// `current` yields. Only a settled not-ready before a settled ready makes
    /// a new generation: a probe that failed in between proves no restart.
    fn readiness(current: &ServiceHostStatus, phase: ServicePhase) -> (bool, u64) {
        match phase.readiness() {
            Some(true) if !current.settled_ready => (true, current.ready_generation + 1),
            Some(ready) => (ready, current.ready_generation),
            None => (current.settled_ready, current.ready_generation),
        }
    }

    fn publish(&self, phase: ServicePhase, compat: ServiceCompat) {
        let current = self.status_tx.borrow().clone();
        let (settled_ready, ready_generation) = Self::readiness(&current, phase);
        self.status_tx.send_replace(ServiceHostStatus {
            settled_ready,
            ready_generation,
            name: current.name.clone(),
            version: current.version.clone(),
            status: current.status,
            server: current.server.clone(),
            phase,
            compat,
            restart_attempts: self.restart_attempts,
        });
    }

    fn publish_probe(
        &self,
        phase: ServicePhase,
        compat: ServiceCompat,
        info: Option<&StatusInfo<'static>>,
    ) {
        let current = self.status_tx.borrow().clone();
        let (settled_ready, ready_generation) = Self::readiness(&current, phase);
        self.status_tx.send_replace(ServiceHostStatus {
            settled_ready,
            ready_generation,
            name: info.map_or_else(|| current.name.clone(), |info| info.name.clone()),
            version: info.map_or_else(|| current.version.clone(), |info| info.version.clone()),
            status: info.map_or(current.status, |info| info.status),
            server: info.map_or_else(|| current.server.clone(), |info| info.server.clone()),
            phase,
            compat,
            restart_attempts: self.restart_attempts,
        });
    }

    fn command_error(what: &str, error: ServiceCommandError) -> CoreError {
        CoreError::new(
            CoreErrorKind::BackendUnavailable,
            format!("service {what} failed: {error}"),
            !matches!(error, ServiceCommandError::AuthorizeServiceCommand { .. }),
        )
    }

    /// Bounds one adapter call (F6). An elapsed bound becomes the same
    /// `Err` shape an adapter-reported failure would, so every existing call
    /// site's error handling covers both alike.
    async fn bounded<T>(
        &self,
        call: BoxFuture<'_, Result<T, ServiceCommandError>>,
    ) -> Result<T, ServiceCommandError> {
        match tokio::time::timeout(self.command_timeout, call).await {
            Ok(result) => result,
            Err(_) => TimedOutSnafu {
                limit_ms: self.command_timeout.as_millis() as u64,
            }
            .fail(),
        }
    }

    /// Bounds one mutating adapter call without abandoning it. The call runs
    /// as a helper task the actor owns; when the bound elapses the helper is
    /// kept in `outstanding` rather than dropped, because an elevated command
    /// whose caller stopped waiting may still be changing the daemon.
    async fn tracked(
        &mut self,
        command: ServiceCommandKind,
        call: impl FnOnce(
            Arc<dyn ServiceProcessControl>,
        ) -> BoxFuture<'static, Result<(), ServiceCommandError>>,
    ) -> Result<(), ServiceCommandError> {
        debug_assert!(
            self.outstanding.is_none(),
            "a mutating call only starts once no helper is outstanding"
        );
        let mut helper = tokio::spawn(call(self.process_control.clone()));
        match tokio::time::timeout(self.command_timeout, &mut helper).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(_) => TaskCancelledSnafu.fail(),
            },
            Err(_) => {
                self.outstanding = Some((command, helper));
                StillRunningSnafu {
                    limit_ms: self.command_timeout.as_millis() as u64,
                }
                .fail()
            }
        }
    }

    /// Whether no command helper is still running. A finished one is reaped
    /// here without a probe of its own: every gated message probes as part of
    /// its own legs, and its caller budget has no room for another (F6).
    async fn reap_outstanding(&mut self) -> bool {
        match &self.outstanding {
            None => return true,
            Some((_, helper)) if !helper.is_finished() => return false,
            Some(_) => {}
        }
        let (command, helper) = self.outstanding.take().expect("checked above");
        match helper.await {
            Ok(Ok(())) => {
                tracing::info!("the service {command:?} command finished after its bound")
            }
            Ok(Err(error)) => {
                tracing::warn!("the service {command:?} command failed after its bound: {error}")
            }
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => {
                    tracing::warn!("the service {command:?} command's task was cancelled: {error}")
                }
            },
        }
        true
    }

    /// `CommandSettled`: whether every accepted command has ended. Reaping a
    /// finished helper probes once, since the phase published when its bound
    /// elapsed says nothing about what it went on to do.
    async fn settle_outstanding(&mut self) -> bool {
        let reaping = self.outstanding.is_some();
        if !self.reap_outstanding().await {
            return false;
        }
        if reaping {
            let _ = self.probe_and_publish().await;
        }
        true
    }

    /// The refusal every endpoint-handing or mutating message gets while a
    /// helper is still running. A probe that reports `Ready` meanwhile does
    /// not lift it: the helper can still be installing or updating the very
    /// daemon that probe answered for.
    async fn refuse_while_running(&mut self) -> Option<CoreError> {
        if self.reap_outstanding().await {
            return None;
        }
        let (command, _) = self
            .outstanding
            .as_ref()
            .expect("an unsettled helper is outstanding");
        Some(CoreError::new(
            CoreErrorKind::OperationConflict,
            format!("a service {command:?} command started earlier is still running"),
            true,
        ))
    }

    /// Turns one probe answer into the compat fact and the phase it implies.
    /// `Err` -- an adapter failure, or (via `bounded`) an elapsed timeout --
    /// is `ServicePhase::Unknown`: it must never be read as `DaemonStopped`,
    /// which is what let a hung probe wave the uninstall guard through (F5).
    fn classify_probe(
        result: &Result<StatusInfo<'static>, ServiceCommandError>,
    ) -> (ServiceCompat, ServicePhase) {
        match result {
            Ok(info) => {
                let compat = ServiceCompat::classify(info);
                let phase = match info.status {
                    ServiceStatus::Running if compat.allows_service_backend() => {
                        ServicePhase::Ready
                    }
                    ServiceStatus::Running => ServicePhase::Incompatible,
                    ServiceStatus::Stopped => ServicePhase::DaemonStopped,
                    ServiceStatus::NotInstalled => ServicePhase::NotInstalled,
                };
                (compat, phase)
            }
            Err(error) => {
                tracing::warn!("service probe failed or timed out: {error}");
                (ServiceCompat::Unknown, ServicePhase::Unknown)
            }
        }
    }

    /// Parses inside the bound: unparseable output is as unknown as no answer.
    async fn probe(&self) -> Result<StatusInfo<'static>, ServiceCommandError> {
        self.bounded(Box::pin(async {
            let status = self.process_control.probe().await?;
            serde_json::from_str(&status).context(ParseServiceStatusSnafu)
        }))
        .await
    }

    /// One probe, classified. The classification is what the daemon actually
    /// is; the latch decides what gets published.
    async fn probe_and_publish(
        &self,
    ) -> (
        Result<StatusInfo<'static>, ServiceCommandError>,
        ServiceCompat,
        ServicePhase,
    ) {
        let result = self.probe().await;
        let (compat, phase) = Self::classify_probe(&result);
        // A latched host stays latched no matter what a probe finds. `compat`
        // still refreshes: the version fact is true either way.
        let published = if self.exhausted {
            ServicePhase::Exhausted
        } else {
            phase
        };
        self.publish_probe(published, compat.clone(), result.as_ref().ok());
        (result, compat, phase)
    }

    /// One health-check probe. Unlike `probe_and_publish` it publishes only
    /// when what the daemon is has changed, so a steady daemon does not wake
    /// every subscriber on each tick.
    async fn health_check(&mut self) {
        if !self.reap_outstanding().await {
            return;
        }
        let result = self.probe().await;
        let (compat, phase) = Self::classify_probe(&result);
        let published = if self.exhausted {
            ServicePhase::Exhausted
        } else {
            phase
        };
        // A failed probe keeps the rest of the status as it was, so only the
        // phase and the compat fact can change then.
        let unchanged = {
            let current = self.status_tx.borrow();
            current.phase == published
                && current.compat == compat
                && result.as_ref().ok().is_none_or(|info| {
                    info.status == current.status && info.version == current.version
                })
        };
        if !unchanged {
            self.publish_probe(published, compat, result.as_ref().ok());
        }
    }

    /// Arms the next health-check tick of the current round, if it is on.
    fn arm_health_check(&mut self, myself: &ActorRef<ServiceActorMessage>) {
        self.health_timer = self.health_round.map(|round| {
            myself.send_after(self.health_interval, move || {
                ServiceActorMessage::HealthCheck { round }
            })
        });
    }

    async fn recover_endpoint(&mut self) -> Result<EndpointHandle, CoreError> {
        if self.exhausted {
            return Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "daemon restart budget exhausted; explicitly reconnect the service host",
                false,
            ));
        }
        let (_, mut compat, mut phase) = self.probe_and_publish().await;
        // An unreachable probe is not proof of death. Only the OS-confirmed
        // stopped state permits a start; a live daemon needs a fresh endpoint.
        if phase == ServicePhase::DaemonStopped {
            if self.restart_attempts >= self.restart_budget {
                self.exhausted = true;
                self.publish(ServicePhase::Exhausted, ServiceCompat::Unknown);
                return Err(CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    "daemon restart budget exhausted; explicitly reconnect the service host",
                    false,
                ));
            }
            self.restart_attempts += 1;
            self.publish(ServicePhase::Restarting, ServiceCompat::Unknown);
            let started = self
                .tracked(ServiceCommandKind::RecoverEndpoint, |adapter| {
                    Box::pin(async move { adapter.start_daemon().await })
                })
                .await;
            let still_running = self.outstanding.is_some();
            let observed = self.probe_and_publish().await;
            compat = observed.1;
            phase = observed.2;
            // A returned OS error is a completed attempt. A timeout leaves
            // the elevated command running in its helper.
            if still_running {
                return Err(CoreError::new(
                    CoreErrorKind::Internal,
                    "daemon recovery start timed out; the elevated command may still be running",
                    false,
                ));
            }
            started.map_err(|error| Self::command_error("recovery start", error))?;
        }
        if phase == ServicePhase::Ready {
            Ok(self.endpoint_factory.endpoint())
        } else {
            Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("daemon endpoint recovery did not reach Ready: {phase:?}"),
                matches!(phase, ServicePhase::Unknown | ServicePhase::DaemonStopped)
                    || (phase == ServicePhase::Incompatible && compat == ServiceCompat::Unknown),
            ))
        }
    }

    /// The idempotent convergence: at most one install and one start per
    /// call, then the gate decides. Explicit convergence clears the exhaustion
    /// latch; a successful explicit daemon start also re-arms it.
    async fn ensure_ready(&mut self) -> Result<EndpointHandle, CoreError> {
        self.exhausted = false;
        self.restart_attempts = 0;
        let result = self.converge().await;
        if result.is_err() {
            // The last thing published is a transitional phase --
            // `Installing` or `StartingDaemon` -- which reads as "still
            // working" forever. One more probe replaces it with what the
            // daemon actually is; the command's own error is what returns.
            let _ = self.probe_and_publish().await;
        }
        result
    }

    async fn converge(&mut self) -> Result<EndpointHandle, CoreError> {
        let (_, _, phase) = self.probe_and_publish().await;
        if phase == ServicePhase::NotInstalled {
            self.publish(ServicePhase::Installing, ServiceCompat::Unknown);
            self.tracked(ServiceCommandKind::EnsureReady, |adapter| {
                Box::pin(async move { adapter.install().await })
            })
            .await
            .map_err(|error| Self::command_error("install", error))?;
        }
        let (_, _, phase) = self.probe_and_publish().await;
        if phase == ServicePhase::DaemonStopped {
            self.publish(ServicePhase::StartingDaemon, ServiceCompat::Unknown);
            self.tracked(ServiceCommandKind::EnsureReady, |adapter| {
                Box::pin(async move { adapter.start_daemon().await })
            })
            .await
            .map_err(|error| Self::command_error("start", error))?;
        }
        let (_, compat, phase) = self.probe_and_publish().await;
        match phase {
            ServicePhase::Ready => Ok(self.endpoint_factory.endpoint()),
            ServicePhase::Incompatible => Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("daemon version gate failed closed: {compat:?}; upgrade required"),
                false,
            )),
            // `Unknown` lands here too (F5): a probe we cannot trust is a
            // retry candidate, exactly like any other not-yet-Ready phase.
            other => Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("daemon did not reach Ready: {other:?}"),
                true,
            )),
        }
    }
}

impl Actor for ServiceActor {
    type Msg = ServiceActorMessage;
    type State = ServiceActorState;
    type Arguments = ServiceActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let mut state = ServiceActorState {
            process_control: args.process_control,
            endpoint_factory: args.endpoint_factory,
            status_tx: args.status_tx,
            restart_budget: args.restart_budget,
            restart_attempts: 0,
            exhausted: false,
            command_timeout: args.command_timeout,
            outstanding: None,
            health_interval: args.health_interval,
            health_round: None,
            health_rounds: 0,
            health_timer: None,
        };
        // Startup version reconcile: an outdated-but-running daemon is
        // upgraded once, preserving the product's auto `update_service`
        // (UAC prompt) semantic. Fail-closed either way — a still-incompatible
        // daemon never passes the gate.
        let (_, compat, phase) = state.probe_and_publish().await;
        if phase == ServicePhase::Incompatible {
            tracing::info!("daemon incompatible at startup ({compat:?}); attempting one update");
            // Tracked like every other mutating call: this runs before any
            // facade exists, so an update that outlives its bound is known
            // only to `outstanding`, and nothing can adopt the daemon until
            // it ends.
            if let Err(error) = state
                .tracked(ServiceCommandKind::Update, |adapter| {
                    Box::pin(async move { adapter.update().await })
                })
                .await
            {
                tracing::warn!("startup daemon update failed: {error}");
            }
            let _ = state.probe_and_publish().await;
        }
        Ok(state)
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(timer) = state.health_timer.take() {
            timer.abort();
            if let Err(error) = timer.await
                && let Ok(panic) = error.try_into_panic()
            {
                std::panic::resume_unwind(panic);
            }
        }
        // Ref's tracked Service commands keep ownership beyond the caller's
        // timeout. Actor termination must not detach an installer/updater or
        // daemon stop that can still mutate the real service. We deliberately
        // do not abort the helper: an elevated command may already be running.
        if let Some((command, helper)) = state.outstanding.take() {
            match helper.await {
                Ok(Ok(())) => {
                    tracing::info!(?command, "outstanding service command settled during stop")
                }
                Ok(Err(error)) => {
                    tracing::warn!(?command, %error, "outstanding service command failed during stop")
                }
                Err(error) => match error.try_into_panic() {
                    Ok(panic) => std::panic::resume_unwind(panic),
                    Err(error) => {
                        tracing::warn!(?command, %error, "outstanding service command cancelled during stop")
                    }
                },
            }
        }
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ServiceActorMessage::EnsureReady { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let _ = reply.send(state.ensure_ready().await);
            }
            ServiceActorMessage::AdoptIfReady { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let (_, compat, phase) = state.probe_and_publish().await;
                let result = if phase == ServicePhase::Ready {
                    Ok(state.endpoint_factory.endpoint())
                } else {
                    Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        format!(
                            "the daemon is {phase:?} ({compat:?}); adopting it would have to install or start it"
                        ),
                        true,
                    ))
                };
                let _ = reply.send(result);
            }
            ServiceActorMessage::Install { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                state.publish(ServicePhase::Installing, ServiceCompat::Unknown);
                let result = state
                    .tracked(ServiceCommandKind::Install, |adapter| {
                        Box::pin(async move { adapter.install().await })
                    })
                    .await
                    .map_err(|error| ServiceActorState::command_error("install", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Update { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let result = state
                    .tracked(ServiceCommandKind::Update, |adapter| {
                        Box::pin(async move { adapter.update().await })
                    })
                    .await
                    .map_err(|error| ServiceActorState::command_error("update", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Uninstall { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let result = async {
                    // Self-check half of the double guard: never uninstall
                    // under a daemon that may still own a core. Fail-closed --
                    // the refusal covers "we know it does" and "we cannot tell
                    // that it does not" alike, because uninstalling out from
                    // under a live core orphans it either way.
                    //
                    // The coarse `CoreState` takes no part in this: it
                    // collapses Starting and Restarting into a Stopped shape,
                    // and Switching and Stopping into a Running one. Neither
                    // shape proves a terminal state.
                    let probe_result = state.probe().await;
                    // F5: the probe call itself can fail outright -- not
                    // merely quiet, unreachable. That is strictly less known
                    // than the in-band "running but blind" case below, so it
                    // is refused the same way, and must never be read as the
                    // "not running" case that follows.
                    let info = match &probe_result {
                        Ok(info) => info,
                        Err(_) => {
                            return Err(CoreError {
                                kind: None,
                                message: "the daemon's status probe failed, so whether it owns a core cannot be determined; stop the daemon first".to_owned(),
                                retryable: false,
                                operation_id: None,
                            });
                        }
                    };
                    //
                    // The kind says what is known, not merely that the answer
                    // was no: `AlreadyRunning` asserts a running core exists,
                    // which is a claim only the first case can make. The other
                    // two know nothing, and callers branch on the kind rather
                    // than on the message.
                    let running = matches!(info.status, ServiceStatus::Running);
                    let refusal = match (&info.status, info.server.as_ref()) {
                        // A daemon that is not running cannot be holding a
                        // core process open.
                        (ServiceStatus::NotInstalled | ServiceStatus::Stopped, _) => None,
                        (ServiceStatus::Running, None) => Some((
                            None,
                            "the daemon is running but did not answer its status probe, so whether it owns a core cannot be determined; stop the daemon first",
                        )),
                        (ServiceStatus::Running, Some(server)) => {
                            match &server.core_infos.detail {
                                Some(CoreStateDetail::Stopped { .. }) => None,
                                Some(_) => Some((
                                    Some(CoreErrorKind::AlreadyRunning),
                                    "the daemon still owns a running core; hand off to Local first",
                                )),
                                None => Some((
                                    None,
                                    "the daemon published no core state detail, so whether it owns a core cannot be determined; stop the daemon first",
                                )),
                            }
                        }
                    };
                    if let Some((kind, message)) = refusal {
                        return Err(CoreError {
                            kind,
                            message: message.to_owned(),
                            retryable: false,
                            operation_id: None,
                        });
                    }
                    state.publish(ServicePhase::Uninstalling, ServiceCompat::Unknown);
                    // Only a running daemon is stopped first: `stop` fails on
                    // one that is already stopped, and that failure would
                    // refuse an uninstall nothing stands in the way of.
                    if running {
                        state
                            .tracked(ServiceCommandKind::Uninstall, |adapter| {
                                Box::pin(async move { adapter.stop_daemon().await })
                            })
                            .await
                            .map_err(|error| ServiceActorState::command_error("stop", error))?;
                        let stopped = state.probe().await;
                        if !matches!(
                            stopped,
                            Ok(StatusInfo {
                                status: ServiceStatus::Stopped | ServiceStatus::NotInstalled,
                                ..
                            })
                        ) {
                            return Err(CoreError::new(
                                CoreErrorKind::AlreadyRunning,
                                "the daemon did not prove it stopped; uninstall was refused",
                                false,
                            ));
                        }
                    }
                    state
                        .tracked(ServiceCommandKind::Uninstall, |adapter| {
                            Box::pin(async move { adapter.uninstall().await })
                        })
                        .await
                        .map_err(|error| ServiceActorState::command_error("uninstall", error))
                }
                .await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::StartDaemon { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let result = state
                    .tracked(ServiceCommandKind::StartDaemon, |adapter| {
                        Box::pin(async move { adapter.start_daemon().await })
                    })
                    .await
                    .map_err(|error| ServiceActorState::command_error("start", error));
                if result.is_ok() {
                    state.exhausted = false;
                    state.restart_attempts = 0;
                }
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::StopDaemon { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let result = state
                    .tracked(ServiceCommandKind::StopDaemon, |adapter| {
                        Box::pin(async move { adapter.stop_daemon().await })
                    })
                    .await
                    .map_err(|error| ServiceActorState::command_error("stop", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::RestartDaemon { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                state.publish(ServicePhase::Restarting, ServiceCompat::Unknown);
                let result = state
                    .tracked(ServiceCommandKind::RestartDaemon, |adapter| {
                        Box::pin(async move { adapter.restart_daemon().await })
                    })
                    .await
                    .map_err(|error| ServiceActorState::command_error("restart", error));
                if result.is_ok() {
                    state.exhausted = false;
                    state.restart_attempts = 0;
                }
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Probe { reply } => {
                let _ = state.probe_and_publish().await;
                let _ = reply.send(state.status_tx.borrow().clone());
            }
            ServiceActorMessage::RecoverEndpoint { reply } => {
                if let Some(refusal) = state.refuse_while_running().await {
                    let _ = reply.send(Err(refusal));
                    return Ok(());
                }
                let _ = reply.send(state.recover_endpoint().await);
            }
            ServiceActorMessage::CommandSettled { reply } => {
                let _ = reply.send(state.settle_outstanding().await);
            }
            ServiceActorMessage::DaemonNotRunning { reply } => {
                let not_running = state.refuse_while_running().await.is_none() && {
                    let (result, _, _) = state.probe_and_publish().await;
                    matches!(
                        result,
                        Ok(StatusInfo {
                            status: ServiceStatus::Stopped | ServiceStatus::NotInstalled,
                            ..
                        })
                    )
                };
                let _ = reply.send(not_running);
            }
            ServiceActorMessage::SetHealthCheck { enabled } => {
                if enabled == state.health_round.is_some() {
                    return Ok(());
                }
                if let Some(timer) = state.health_timer.take() {
                    timer.abort();
                }
                state.health_round = enabled.then(|| {
                    state.health_rounds += 1;
                    state.health_rounds
                });
                state.arm_health_check(&myself);
            }
            ServiceActorMessage::HealthCheck { round } => {
                // A tick from a check turned off since is dropped.
                if state.health_round == Some(round) {
                    state.health_check().await;
                    state.arm_health_check(&myself);
                }
            }
        }
        Ok(())
    }
}

/// Scheduling and mailbox-hop slack layered on top of the worst-case leg sum
/// for every caller-facing budget below. Mirrors `CoreClient`'s
/// `CALL_BUDGET_SLACK` in `mod.rs` (F3): not a place to hide another retry,
/// it only has to cover the time between an internal path finishing
/// honestly and the reply crossing back to the caller.
const CALL_BUDGET_SLACK: std::time::Duration = std::time::Duration::from_secs(10);

/// Caller bound for [`ServiceClient::probe`]. One leg: `probe_and_publish`'s
/// single bounded `adapter.probe()` call -- the `Probe` handler does nothing
/// else.
fn probe_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout + CALL_BUDGET_SLACK
}

/// Caller bound for [`ServiceClient::install`], [`ServiceClient::update`],
/// [`ServiceClient::start_daemon`], and [`ServiceClient::stop_daemon`]. Each
/// of those four handlers is exactly two bounded legs: the command itself,
/// then the unconditional follow-up `probe_and_publish` that replaces the
/// transitional phase it published with what the daemon actually is.
fn command_and_probe_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout * 2 + CALL_BUDGET_SLACK
}

/// Caller bound for [`ServiceClient::uninstall`]. Five legs: the guard probe,
/// `stop_daemon`, the structural stopped-state proof, `uninstall`, and the
/// handler's final `probe_and_publish`.
fn uninstall_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout * 5 + CALL_BUDGET_SLACK
}

/// Caller bound for [`ServiceClient::ensure_ready`]. Six legs, worst case:
/// `converge` always runs three unconditional `probe_and_publish` calls (the
/// entry read, the post-install read, and the final gate read) and, between
/// them, up to two conditional adapter calls -- `install` when the entry
/// read is `NotInstalled`, `start_daemon` when the post-install read is
/// `DaemonStopped` -- both of which fire on the same path when installing a
/// daemon that does not auto-start itself. If the final gate read still is
/// not `Ready` (e.g. `Incompatible`), `converge` returns `Err` and
/// `ensure_ready` runs one more recovery `probe_and_publish` so the last
/// published phase is not left transitional. 3 probes + install + start + 1
/// recovery probe = 6.
fn ensure_ready_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout * 6 + CALL_BUDGET_SLACK
}

// Honesty note: the mailbox is strictly serialized, so any caller can also
// sit queued behind whatever message is already being handled before its
// own turn even starts. The budgets above bound a message's own legs once
// the actor is handling it; queue residence ahead of that is a separate,
// real exposure this file does not attempt to bound.

/// Typed wrapper; callers never hold the raw `ActorRef`.
#[derive(Clone)]
pub struct ServiceClient {
    actor: ActorRef<ServiceActorMessage>,
    status_rx: watch::Receiver<ServiceHostStatus>,
    /// Per-message caller bounds derived from the injected `command_timeout`
    /// at spawn time -- this actor's twin of `CoreClient`'s F3 fix in
    /// `mod.rs`. Each must exceed the worst-case sum of bounded legs its
    /// message can take internally (see the `*_budget` functions above), or
    /// the client gives up on a call the actor is still legitimately
    /// working through.
    probe_budget: std::time::Duration,
    command_and_probe_budget: std::time::Duration,
    uninstall_budget: std::time::Duration,
    ensure_ready_budget: std::time::Duration,
}

impl ServiceClient {
    /// Registers cleanup immediately after construction, including partial startup.
    pub fn drain_on_shutdown(
        &self,
        tasks: &tokio_util::task::TaskTracker,
        shutdown: tokio_util::sync::CancellationToken,
    ) {
        let cell = self.actor.get_cell();
        tasks.spawn(async move {
            shutdown.cancelled().await;
            let _ = cell.drain_and_wait(None).await;
        });
    }

    pub async fn spawn(
        process_control: Arc<dyn ServiceProcessControl>,
        endpoint_factory: Arc<dyn ServiceEndpointFactory>,
        restart_budget: u8,
    ) -> Result<Self, ractor::SpawnErr> {
        Self::spawn_with_bounds(
            process_control,
            endpoint_factory,
            restart_budget,
            DEFAULT_SERVICE_COMMAND_TIMEOUT,
        )
        .await
    }

    /// Turns the periodic health check on or off. Best effort: an actor that
    /// is gone has nothing left to check.
    pub fn set_health_check(&self, enabled: bool) {
        let _ = self
            .actor
            .cast(ServiceActorMessage::SetHealthCheck { enabled });
    }

    /// Whether the OS reports the daemon not running, which proves it holds
    /// no core. See [`ServiceActorMessage::DaemonNotRunning`].
    ///
    /// It waits for the real answer: the probe inside is bounded at the
    /// adapter, and a proof that arrives late is still the proof.
    pub async fn daemon_not_running(&self) -> Result<bool, CoreError> {
        match self
            .actor
            .call(
                |reply| ServiceActorMessage::DaemonNotRunning { reply },
                None,
            )
            .await
        {
            Ok(ractor::rpc::CallResult::Success(not_running)) => Ok(not_running),
            _ => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the service actor is gone",
                false,
            )),
        }
    }

    /// Test seam (F6): lets a test inject a short `command_timeout` so a
    /// hang test is bounded by the actor's own timeout instead of the
    /// production wall clock. Not `pub` -- production code always goes
    /// through `spawn`, which pins the policy constant.
    async fn spawn_with_bounds(
        process_control: Arc<dyn ServiceProcessControl>,
        endpoint_factory: Arc<dyn ServiceEndpointFactory>,
        restart_budget: u8,
        command_timeout: std::time::Duration,
    ) -> Result<Self, ractor::SpawnErr> {
        let (status_tx, status_rx) = watch::channel(ServiceHostStatus {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed(""),
            status: ServiceStatus::NotInstalled,
            server: None,
            phase: ServicePhase::Probing,
            compat: ServiceCompat::Unknown,
            restart_attempts: 0,
            settled_ready: false,
            ready_generation: 0,
        });
        let (actor, _handle) = Actor::spawn(
            None,
            ServiceActor,
            ServiceActorArgs {
                process_control,
                endpoint_factory,
                status_tx,
                restart_budget,
                command_timeout,
                health_interval: HEALTH_CHECK_INTERVAL,
            },
        )
        .await?;
        Ok(Self {
            actor,
            status_rx,
            probe_budget: probe_budget(command_timeout),
            command_and_probe_budget: command_and_probe_budget(command_timeout),
            uninstall_budget: uninstall_budget(command_timeout),
            ensure_ready_budget: ensure_ready_budget(command_timeout),
        })
    }

    /// The same seam for a workflow test that needs an elevated command to
    /// outlive a short bound.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn spawn_bounded(
        process_control: Arc<dyn ServiceProcessControl>,
        endpoint_factory: Arc<dyn ServiceEndpointFactory>,
        restart_budget: u8,
        command_timeout: std::time::Duration,
    ) -> Result<Self, ractor::SpawnErr> {
        Self::spawn_with_bounds(
            process_control,
            endpoint_factory,
            restart_budget,
            command_timeout,
        )
        .await
    }

    pub fn status(&self) -> ServiceHostStatus {
        self.status_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ServiceHostStatus> {
        self.status_rx.clone()
    }

    pub async fn ensure_ready(&self) -> Result<EndpointHandle, CoreError> {
        self.call(
            |reply| ServiceActorMessage::EnsureReady { reply },
            self.ensure_ready_budget,
        )
        .await?
    }

    /// The endpoint of a daemon that is already `Ready`, or an error. Unlike
    /// [`Self::ensure_ready`] this never converges, so its worst case is a
    /// single probe leg and it borrows `probe`'s budget.
    pub async fn adopt_if_ready(&self) -> Result<EndpointHandle, CoreError> {
        self.call(
            |reply| ServiceActorMessage::AdoptIfReady { reply },
            self.probe_budget,
        )
        .await?
    }

    pub async fn install(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::Install { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn update(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::Update { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn uninstall(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::Uninstall { reply },
            self.uninstall_budget,
        )
        .await?
    }

    pub async fn start_daemon(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::StartDaemon { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn stop_daemon(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::StopDaemon { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    /// Chimera's native restart is a single bound Service Actor mailbox
    /// command and an unconditional status probe, never a stop/start pair.
    pub async fn restart_daemon(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::RestartDaemon { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn probe(&self) -> Result<ServiceHostStatus, CoreError> {
        self.call(
            |reply| ServiceActorMessage::Probe { reply },
            self.probe_budget,
        )
        .await
    }

    pub async fn recover_endpoint(&self) -> Result<EndpointHandle, CoreError> {
        self.call(
            |reply| ServiceActorMessage::RecoverEndpoint { reply },
            // Initial probe, optional start, and the post-start probe.
            self.probe_budget + self.command_and_probe_budget,
        )
        .await?
    }

    /// Positive evidence that every command this actor accepted has ended,
    /// including a helper that outlived its bound. Reaping a finished helper
    /// probes once, so this borrows `probe`'s budget.
    pub async fn command_settled(&self) -> Result<bool, CoreError> {
        self.call(
            |reply| ServiceActorMessage::CommandSettled { reply },
            self.probe_budget,
        )
        .await
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> ServiceActorMessage,
        timeout: std::time::Duration,
    ) -> Result<T, CoreError> {
        // `timeout` is one of the per-message `*_budget`s above, each
        // derived to exceed that message's own worst-case leg sum (F6); this
        // gives up only after the actor could no longer still be honestly
        // working through it.
        match self.actor.call(message, Some(timeout)).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            // Past the budget the remaining explanation is mailbox residence
            // behind another message, which means the elevated command may be
            // running right now. Saying so is the only honest answer: it is
            // not retryable, because a blind retry would queue a second
            // elevated command behind the first.
            Ok(ractor::rpc::CallResult::Timeout) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the service actor did not answer within its bound; the command may still be running",
                false,
            )),
            Ok(ractor::rpc::CallResult::SenderError) | Err(_) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the service actor is gone",
                false,
            )),
        }
    }
}

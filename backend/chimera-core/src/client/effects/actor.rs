//! Owns peripheral desired state and independent, coalesced execution groups.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectFields, ApplicationEffectInputs, ApplicationEffectPlan,
        ClashEffectFields, EffectKind, TrayRefresh,
    },
    ports::{ApplicationEffectsPort, CommitNotifications},
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::convergence::{ConvergenceHealth, RetryBudget};
use chimera_config::runtime::executor::ResolvedPortBindings;

#[derive(Clone, Debug, Default)]
pub struct EffectsSnapshot {
    pub event_seq: u64,
    #[cfg(any(test, feature = "test-support"))]
    pub revision: u64,
    pub effects: Vec<EffectProgress>,
}

/// One effect kind: what its owner last reported, and how convergence is going.
#[derive(Clone, Debug)]
pub struct EffectProgress {
    pub status: EffectStatus,
    pub health: ConvergenceHealth,
    pub attempts: u32,
    pub automatic_remaining: u8,
}
struct Entry {
    status: EffectStatus,
    health: ConvergenceHealth,
    budget: RetryBudget,
    next: Option<tokio::time::Instant>,
    automatic: bool,
}
impl Entry {
    fn new(kind: EffectKind) -> Self {
        Self {
            status: EffectStatus {
                kind,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Pending,
            },
            health: ConvergenceHealth::Pending,
            budget: RetryBudget::default(),
            next: None,
            automatic: false,
        }
    }
}

#[derive(Clone)]
pub struct EffectsClient {
    actor: ActorRef<Message>,
    status: watch::Receiver<EffectsSnapshot>,
    shutdown: CancellationToken,
}

pub struct EffectsArgs {
    pub port: Arc<dyn ApplicationEffectsPort>,
    pub initial: ApplicationEffectInputs,
}

struct EffectsActor;
struct Args {
    dependencies: EffectsArgs,
    shutdown: CancellationToken,
    status: watch::Sender<EffectsSnapshot>,
}
struct State {
    port: Arc<dyn ApplicationEffectsPort>,
    desired: ApplicationEffectInputs,
    revision: u64,
    pending: BTreeMap<EffectKind, ApplicationEffect>,
    entries: BTreeMap<EffectKind, Entry>,
    waiters: BTreeMap<(EffectKind, EffectRevision), Vec<RpcReplyPort<EffectStatus>>>,
    active: [Option<tokio::task::JoinHandle<()>>; 4],
    status: watch::Sender<EffectsSnapshot>,
    closed: bool,
    shutdown: CancellationToken,
    retry_timer: Option<(
        tokio::time::Instant,
        tokio::task::JoinHandle<Result<(), ractor::MessagingErr<Message>>>,
    )>,
}

enum Message {
    Publish {
        slice: Slice,
        refresh: bool,
        full: bool,
        requested: Vec<EffectKind>,
    },
    ReconcileHotkeys {
        inputs: Box<ApplicationEffectInputs>,
        reply: RpcReplyPort<EffectStatus>,
    },
    ReconcileEffect {
        kind: EffectKind,
        reply: RpcReplyPort<EffectStatus>,
    },
    Completed {
        group: usize,
        revision: EffectRevision,
        kinds: Vec<EffectKind>,
        statuses: Vec<EffectStatus>,
    },
    Tick(tokio::time::Instant),
    RetryNow(EffectKind),
    Shutdown(RpcReplyPort<Vec<EffectStatus>>),
    #[cfg(any(test, feature = "test-support"))]
    Barrier(RpcReplyPort<()>),
    #[cfg(test)]
    RetryDeadline(RpcReplyPort<Option<(tokio::time::Instant, tokio::task::Id)>>),
}

/// One owner sends only its latest slice. The actor merges slices before it
/// diffs effects, so an update cannot overwrite another domain's newer state.
enum Slice {
    Application(Box<ApplicationEffectFields>),
    Clash(ClashEffectFields),
    Ports(Option<ResolvedPortBindings>),
    Profiles,
    Icons,
    Full(Box<ApplicationEffectInputs>),
}

fn merge_slice(mut inputs: ApplicationEffectInputs, slice: Slice) -> ApplicationEffectInputs {
    match slice {
        Slice::Application(app) => inputs.app = *app,
        Slice::Clash(clash) => inputs.clash = clash,
        Slice::Ports(ports) => inputs.ports = ports,
        Slice::Profiles => {}
        Slice::Icons => inputs.icon_revision += 1,
        Slice::Full(complete) => {
            let icon_revision = inputs.icon_revision;
            inputs = *complete;
            inputs.icon_revision = icon_revision;
        }
    }
    inputs
}

fn group(kind: EffectKind) -> usize {
    match kind {
        // One owner: the system proxy actor also applies auto-launch.
        EffectKind::SystemProxy | EffectKind::ProxyGuard | EffectKind::AutoLaunch => 0,
        EffectKind::Hotkeys => 1,
        EffectKind::Locale
        | EffectKind::Logger
        | EffectKind::Widget
        | EffectKind::WidgetLogo
        | EffectKind::Tray => 2,
        EffectKind::CoreLogStorage | EffectKind::TransparentProxy => 3,
    }
}

impl State {
    fn arm_retry(&mut self, actor: &ActorRef<Message>) {
        let due = if self.closed || self.shutdown.is_cancelled() {
            None
        } else {
            self.entries.values().filter_map(|entry| entry.next).min()
        };
        if self.retry_timer.as_ref().map(|(at, _)| *at) == due {
            return;
        }
        if let Some((_, timer)) = self.retry_timer.take() {
            timer.abort();
        }
        self.retry_timer = due.map(|at| {
            let wait = at.saturating_duration_since(tokio::time::Instant::now());
            (at, actor.send_after(wait, move || Message::Tick(at)))
        });
    }

    fn publish(&self) {
        let event_seq = self.status.borrow().event_seq + 1;
        self.status.send_replace(EffectsSnapshot {
            event_seq,
            #[cfg(any(test, feature = "test-support"))]
            revision: self.revision,
            effects: self
                .entries
                .values()
                .map(|entry| EffectProgress {
                    status: entry.status.clone(),
                    health: entry.health,
                    attempts: entry.budget.attempts,
                    automatic_remaining: entry.budget.remaining,
                })
                .collect(),
        });
    }

    fn enqueue(&mut self, inputs: ApplicationEffectInputs, refresh: bool, full: bool) {
        let changes = ApplicationEffectPlan::diff(&self.desired, &inputs);
        let changed: std::collections::BTreeSet<_> = changes
            .effects()
            .iter()
            .map(ApplicationEffect::kind)
            .collect();
        let plan = if full {
            ApplicationEffectPlan::full(&inputs)
        } else {
            ApplicationEffectPlan::diff(&self.desired, &inputs)
        };
        let binding_ready = self.desired.ports != inputs.ports && inputs.ports.is_some();
        self.desired = inputs;
        let mut effects = plan.effects().to_vec();
        if refresh
            && !effects
                .iter()
                .any(|effect| effect.kind() == EffectKind::Tray)
        {
            effects.push(ApplicationEffect::Tray(
                TrayRefresh::Part,
                self.desired.tray_view(),
            ));
        }
        if effects.is_empty() {
            return;
        }
        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        let superseded: Vec<_> = self
            .waiters
            .keys()
            .filter(|(kind, previous)| changed.contains(kind) && *previous < revision)
            .copied()
            .collect();
        for key in superseded {
            if let Some(replies) = self.waiters.remove(&key) {
                let applied_revision = self
                    .entries
                    .get(&key.0)
                    .map(|entry| entry.status.applied_revision)
                    .unwrap_or_default();
                let status = EffectStatus {
                    kind: key.0,
                    desired_revision: key.1,
                    applied_revision,
                    health: EffectHealth::Superseded,
                };
                for reply in replies {
                    let _ = reply.send(status.clone());
                }
            }
        }
        for mut effect in effects {
            let kind = effect.kind();
            if let ApplicationEffect::Tray(refresh, _) = &mut effect
                && matches!(
                    self.pending.get(&kind),
                    Some(ApplicationEffect::Tray(TrayRefresh::Full, _))
                )
            {
                *refresh = TrayRefresh::Full;
            }
            let entry = self.entries.entry(kind).or_insert_with(|| Entry::new(kind));
            if changed.contains(&kind) {
                entry.budget = RetryBudget::default();
                entry.automatic = false;
            }
            entry.next = None;
            entry.health = ConvergenceHealth::Pending;
            entry.status.desired_revision = revision;
            entry.status.health = EffectHealth::Pending;
            self.pending.insert(kind, effect);
        }
        if binding_ready {
            self.retry(EffectKind::ProxyGuard, false);
        }
    }

    fn enqueue_hotkeys(&mut self, inputs: ApplicationEffectInputs) -> EffectRevision {
        self.desired.app.hotkeys = inputs.app.hotkeys;
        let effect = ApplicationEffectPlan::full(&self.desired)
            .effects()
            .iter()
            .find(|effect| effect.kind() == EffectKind::Hotkeys)
            .cloned()
            .expect("a full application effect plan contains hotkeys");

        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        let superseded: Vec<_> = self
            .waiters
            .keys()
            .filter(|(kind, previous)| *kind == EffectKind::Hotkeys && *previous < revision)
            .copied()
            .collect();
        let applied_revision = self
            .entries
            .get(&EffectKind::Hotkeys)
            .map(|entry| entry.status.applied_revision)
            .unwrap_or_default();
        for key in superseded {
            if let Some(replies) = self.waiters.remove(&key) {
                let status = EffectStatus {
                    kind: EffectKind::Hotkeys,
                    desired_revision: key.1,
                    applied_revision,
                    health: EffectHealth::Superseded,
                };
                for reply in replies {
                    let _ = reply.send(status.clone());
                }
            }
        }

        let entry = self
            .entries
            .entry(EffectKind::Hotkeys)
            .or_insert_with(|| Entry::new(EffectKind::Hotkeys));
        entry.budget = RetryBudget::default();
        entry.automatic = false;
        entry.next = None;
        entry.health = ConvergenceHealth::Pending;
        entry.status.desired_revision = revision;
        entry.status.health = EffectHealth::Pending;
        self.pending.insert(EffectKind::Hotkeys, effect);
        revision
    }

    fn retry(&mut self, kind: EffectKind, automatic: bool) {
        let Some(entry) = self.entries.get_mut(&kind) else {
            return;
        };
        if entry.health == ConvergenceHealth::Pending || entry.health == ConvergenceHealth::Healthy
        {
            return;
        }
        if automatic && entry.health == ConvergenceHealth::Blocked {
            return;
        }
        let Some(effect) = ApplicationEffectPlan::full(&self.desired)
            .effects()
            .iter()
            .find(|e| e.kind() == kind)
            .cloned()
        else {
            return;
        };
        entry.automatic = automatic && entry.health == ConvergenceHealth::RetryScheduled;
        entry.next = None;
        entry.health = ConvergenceHealth::Pending;
        self.revision += 1;
        entry.status.desired_revision = EffectRevision::new(self.revision);
        entry.status.health = EffectHealth::Pending;
        self.pending.insert(kind, effect);
    }

    /// A fresh core apply changes the expected revision even when the listener
    /// port is stable, so resend the current target to the Service owner.
    fn reapply(&mut self, kind: EffectKind) {
        let Some(effect) = ApplicationEffectPlan::full(&self.desired)
            .effects()
            .iter()
            .find(|effect| effect.kind() == kind)
            .cloned()
        else {
            return;
        };
        let entry = self.entries.entry(kind).or_insert_with(|| Entry::new(kind));
        entry.automatic = false;
        entry.next = None;
        entry.health = ConvergenceHealth::Pending;
        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        entry.status.desired_revision = revision;
        entry.status.health = EffectHealth::Pending;
        self.pending.insert(kind, effect);
    }

    /// Ref EffectsActor drains active owners rather than aborting a command
    /// halfway through a platform mutation. All new work is already gated by
    /// `closed`; draining also prevents `port.shutdown()` from racing a late
    /// SystemProxy/TransparentProxy/Hotkeys apply.
    async fn drain_active(&mut self) {
        let mut first_panic = None;
        for task in self.active.iter_mut().filter_map(Option::take) {
            if let Err(error) = task.await {
                match error.try_into_panic() {
                    Ok(panic) if first_panic.is_none() => first_panic = Some(panic),
                    Ok(_) => tracing::error!("additional effects owner task panicked during drain"),
                    Err(error) => {
                        tracing::warn!(%error, "effects owner task was cancelled during drain");
                    }
                }
            }
        }
        // Drain every independent group before resuming a fatal panic. An
        // early resume would detach other OS mutations and lose ownership.
        if let Some(panic) = first_panic {
            std::panic::resume_unwind(panic);
        }
    }

    fn drive(&mut self, myself: &ActorRef<Message>) {
        for index in 0..4 {
            if self.closed || self.shutdown.is_cancelled() {
                break;
            }
            if self.active[index].is_some() {
                continue;
            }
            let kinds: Vec<_> = self
                .pending
                .keys()
                .copied()
                .filter(|kind| group(*kind) == index)
                .collect();
            if kinds.is_empty() {
                continue;
            }
            let effects: Vec<_> = kinds
                .iter()
                .map(|kind| self.pending.remove(kind).unwrap())
                .collect();
            let revision = EffectRevision::new(self.revision);
            for kind in &kinds {
                let entry = self.entries.get_mut(kind).unwrap();
                entry.status.desired_revision = revision;
                entry.budget.attempts += 1;
                if entry.automatic {
                    entry.budget.remaining = entry.budget.remaining.saturating_sub(1);
                }
                entry.health = ConvergenceHealth::Pending;
            }
            let port = self.port.clone();
            let actor = myself.clone();
            self.active[index] = Some(tokio::spawn(async move {
                let work = async {
                    port.apply(revision, ApplicationEffectPlan::from_effects(effects))
                        .await
                };
                // The effect actor retains its recovery/retry state rather
                // than allowing a panicking effect to tear down the group.
                // Only the active poll is marked recoverable: no TLS guard
                // may remain active while this task is suspended.
                let statuses = match chimera_panics::catch_recoverable_future(work).await {
                    Ok(statuses) => statuses,
                    Err(error) => {
                        tracing::error!(%error, ?kinds, "application effect group panicked");
                        Vec::new()
                    }
                };
                let _ = actor.cast(Message::Completed {
                    group: index,
                    revision,
                    kinds,
                    statuses,
                });
            }));
        }
        self.publish();
    }
}

impl Actor for EffectsActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _myself: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            port: args.dependencies.port,
            desired: args.dependencies.initial,
            revision: 0,
            pending: BTreeMap::new(),
            entries: BTreeMap::new(),
            waiters: BTreeMap::new(),
            active: [None, None, None, None],
            status: args.status,
            closed: false,
            shutdown: args.shutdown,
            retry_timer: None,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Publish {
                slice,
                refresh,
                full,
                requested,
            } if !state.closed && !state.shutdown.is_cancelled() => {
                let runtime_notification = matches!(&slice, Slice::Ports(_));
                let inputs = merge_slice(state.desired.clone(), slice);
                let changed: Vec<_> = ApplicationEffectPlan::diff(&state.desired, &inputs)
                    .effects()
                    .iter()
                    .map(ApplicationEffect::kind)
                    .collect();
                state.enqueue(inputs, refresh, full);
                if runtime_notification && !changed.contains(&EffectKind::TransparentProxy) {
                    state.reapply(EffectKind::TransparentProxy);
                }
                for kind in requested {
                    if !changed.contains(&kind) {
                        state.retry(kind, false);
                    }
                }
                state.drive(&myself);
            }
            Message::Publish { .. } => {}
            Message::ReconcileHotkeys { inputs, reply }
                if !state.closed && !state.shutdown.is_cancelled() =>
            {
                let revision = state.enqueue_hotkeys(*inputs);
                state
                    .waiters
                    .entry((EffectKind::Hotkeys, revision))
                    .or_default()
                    .push(reply);
                state.drive(&myself);
            }
            Message::ReconcileHotkeys { reply, .. } => {
                let _ = reply.send(EffectStatus {
                    kind: EffectKind::Hotkeys,
                    desired_revision: EffectRevision::default(),
                    applied_revision: EffectRevision::default(),
                    health: EffectHealth::Degraded {
                        code: "effects_shut_down",
                        message: "the application effect actor is shutting down".into(),
                        retryable: false,
                    },
                });
            }
            Message::ReconcileEffect { kind, reply }
                if !state.closed && !state.shutdown.is_cancelled() =>
            {
                if !state.entries.contains_key(&kind) {
                    let _ = reply.send(EffectStatus {
                        kind,
                        desired_revision: EffectRevision::default(),
                        applied_revision: EffectRevision::default(),
                        health: EffectHealth::Degraded {
                            code: "effect_not_initialized",
                            message: format!("the {kind:?} effect has not been initialized"),
                            retryable: true,
                        },
                    });
                } else {
                    let settled = state
                        .entries
                        .get(&kind)
                        .is_some_and(|entry| entry.status.health != EffectHealth::Pending);
                    if settled && state.entries[&kind].status.health != EffectHealth::Healthy {
                        state.retry(kind, false);
                    }
                    let entry = &state.entries[&kind];
                    let revision = entry.status.desired_revision;
                    if entry.status.health != EffectHealth::Pending {
                        let _ = reply.send(entry.status.clone());
                    } else {
                        state
                            .waiters
                            .entry((kind, revision))
                            .or_default()
                            .push(reply);
                        state.drive(&myself);
                    }
                }
            }
            Message::ReconcileEffect { kind, reply } => {
                let _ = reply.send(EffectStatus {
                    kind,
                    desired_revision: EffectRevision::default(),
                    applied_revision: EffectRevision::default(),
                    health: EffectHealth::Degraded {
                        code: "effects_shut_down",
                        message: "the application effect actor is shutting down".into(),
                        retryable: false,
                    },
                });
            }
            Message::Completed {
                group: completed_group,
                revision,
                kinds,
                statuses,
            } if !state.closed && !state.shutdown.is_cancelled() => {
                // A completion message is sent before the spawned task
                // returns. Reap the owned JoinHandle before admitting more
                // work for this group; never silently detach a fatal panic.
                if let Some(task) = state.active[completed_group].take()
                    && let Err(error) = task.await
                    && let Ok(panic) = error.try_into_panic()
                {
                    std::panic::resume_unwind(panic);
                }
                for kind in kinds {
                    let Some(entry) = state.entries.get_mut(&kind) else {
                        continue;
                    };
                    // A newer desired revision was queued meanwhile; this result is stale.
                    if entry.status.desired_revision != revision {
                        continue;
                    }
                    match statuses.iter().find(|s| s.kind == kind) {
                        Some(status) if status.health == EffectHealth::Healthy => {
                            entry.status.applied_revision = revision;
                            entry.status.health = EffectHealth::Healthy;
                        }
                        Some(status) => entry.status.health = status.health.clone(),
                        None => {
                            entry.status.health = EffectHealth::Degraded {
                                code: "effect_owner_silent",
                                message: format!("{kind:?} returned no result"),
                                retryable: false,
                            }
                        }
                    }
                    entry.health = match &entry.status.health {
                        EffectHealth::Healthy => ConvergenceHealth::Healthy,
                        EffectHealth::Degraded {
                            code:
                                "widget_unavailable"
                                | "system_proxy_port_unresolved"
                                | "proxy_guard_waiting_dependency",
                            ..
                        } => {
                            entry.budget.attempts = entry.budget.attempts.saturating_sub(1);
                            if entry.automatic {
                                entry.budget.remaining += 1;
                            }
                            entry.next =
                                Some(tokio::time::Instant::now() + Duration::from_secs(30));
                            ConvergenceHealth::WaitingDependency
                        }
                        EffectHealth::Degraded {
                            retryable: true, ..
                        } if entry.budget.remaining > 0 => {
                            entry.next = entry
                                .budget
                                .next_delay()
                                .map(|delay| tokio::time::Instant::now() + delay);
                            ConvergenceHealth::RetryScheduled
                        }
                        _ => ConvergenceHealth::Blocked,
                    };
                    entry.automatic = false;
                    let status = entry.status.clone();
                    if let Some(replies) = state.waiters.remove(&(kind, revision)) {
                        for reply in replies {
                            let _ = reply.send(status.clone());
                        }
                    }
                }
                if completed_group == 0
                    && state
                        .entries
                        .get(&EffectKind::SystemProxy)
                        .is_some_and(|r| r.health == ConvergenceHealth::Healthy)
                    && state
                        .entries
                        .get(&EffectKind::ProxyGuard)
                        .is_some_and(|r| r.health == ConvergenceHealth::WaitingDependency)
                {
                    state.retry(EffectKind::ProxyGuard, false);
                }
                state.drive(&myself);
            }
            Message::Completed { .. } => {}
            Message::Tick(at)
                if !state.closed
                    && !state.shutdown.is_cancelled()
                    && state
                        .retry_timer
                        .as_ref()
                        .is_some_and(|(due, _)| *due == at) =>
            {
                state.retry_timer = None;
                let ready: Vec<_> = state
                    .entries
                    .iter()
                    .filter(|(_, r)| r.next.is_some_and(|at| at <= tokio::time::Instant::now()))
                    .map(|(kind, _)| *kind)
                    .collect();
                if !ready.is_empty() {
                    for kind in ready {
                        state.retry(kind, true);
                    }
                    state.drive(&myself);
                }
            }
            Message::RetryNow(kind) if !state.closed && !state.shutdown.is_cancelled() => {
                state.retry(kind, false);
                state.drive(&myself);
            }
            Message::Tick(_) | Message::RetryNow(_) => {}
            Message::Shutdown(reply) => {
                state.closed = true;
                if let Some((_, timer)) = state.retry_timer.take() {
                    timer.abort();
                }
                state.pending.clear();
                // A platform effect can already be changing OS state. Abort
                // would allow a late apply to race the following shutdown.
                // The ref owner instead waits for in-flight groups to settle.
                state.drain_active().await;
                for ((kind, revision), replies) in std::mem::take(&mut state.waiters) {
                    let status = EffectStatus {
                        kind,
                        desired_revision: revision,
                        applied_revision: EffectRevision::default(),
                        health: EffectHealth::Degraded {
                            code: "effects_shut_down",
                            message:
                                "the application effect actor shut down before the effect settled"
                                    .into(),
                            retryable: false,
                        },
                    };
                    for reply in replies {
                        let _ = reply.send(status.clone());
                    }
                }
                state.publish();
                let _ = reply.send(state.port.shutdown().await);
            }
            #[cfg(test)]
            Message::RetryDeadline(reply) => {
                let _ = reply.send(
                    state
                        .retry_timer
                        .as_ref()
                        .map(|(at, timer)| (*at, timer.id())),
                );
            }
            #[cfg(any(test, feature = "test-support"))]
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
        state.arm_retry(&myself);
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        let release_port = !state.closed;
        state.closed = true;
        state.pending.clear();
        if let Some((_, timer)) = state.retry_timer.take() {
            timer.abort();
        }
        // Unexpected actor termination must not abandon in-flight OS effects.
        // Preserve upstream JoinHandle panic propagation after draining.
        state.drain_active().await;
        // Chimera's host adapter owns shortcut registration. Keep its release
        // inside the tracked effects owner rather than calling a stopped actor
        // from the Tauri exit boundary.
        if release_port {
            for status in state.port.shutdown().await {
                if let EffectHealth::Degraded { code, message, .. } = status.health {
                    tracing::warn!(%code, %message, "failed to release application effects at shutdown");
                }
            }
        }
        Ok(())
    }
}

impl EffectsClient {
    pub async fn spawn(args: EffectsArgs) -> anyhow::Result<Self> {
        Self::spawn_inner(args, CancellationToken::new()).await
    }

    /// Register the effects owner with the same root lifecycle as ref.
    pub async fn spawn_tracked(
        args: EffectsArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let client = Self::spawn_inner(args, shutdown.clone()).await?;
        chimera_owner_support::shutdown::drain_on_shutdown(
            tasks,
            shutdown,
            client.actor.get_cell(),
        );
        Ok(client)
    }

    async fn spawn_inner(args: EffectsArgs, shutdown: CancellationToken) -> anyhow::Result<Self> {
        let (status, receiver) = watch::channel(EffectsSnapshot::default());
        let (actor, _) = Actor::spawn(
            None,
            EffectsActor,
            Args {
                dependencies: args,
                shutdown: shutdown.clone(),
                status,
            },
        )
        .await?;
        Ok(Self {
            actor,
            status: receiver,
            shutdown,
        })
    }

    pub fn retry_now(&self, kind: EffectKind) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.shutdown.is_cancelled(),
            "effects owner is shutting down"
        );
        self.actor
            .cast(Message::RetryNow(kind))
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    /// A tray icon file changed: the tray redraws, and the widget logo follows
    /// in case it shows the tray's icons.
    pub fn request_tray_refresh(&self) -> anyhow::Result<()> {
        self.publish_icons(true)
    }

    /// A widget icon file changed.
    pub fn request_widget_logo_refresh(&self) -> anyhow::Result<()> {
        self.publish_icons(false)
    }

    fn publish_icons(&self, refresh_tray: bool) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.shutdown.is_cancelled(),
            "effects owner is shutting down"
        );
        self.actor
            .cast(Message::Publish {
                slice: Slice::Icons,
                refresh: refresh_tray,
                full: false,
                requested: Vec::new(),
            })
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    pub async fn reconcile_hotkeys(&self, inputs: ApplicationEffectInputs) -> EffectStatus {
        match self
            .actor
            .call(
                |reply| Message::ReconcileHotkeys {
                    inputs: Box::new(inputs),
                    reply,
                },
                Some(Duration::from_secs(20)),
            )
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => EffectStatus {
                kind: EffectKind::Hotkeys,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "hotkey_effect_wait_failed",
                    message: format!("the shared effect actor did not settle hotkeys: {other:?}"),
                    retryable: true,
                },
            },
        }
    }

    /// Reconciles one already-published effect and waits for its owner. This
    /// is used by confirmed agent actions that must verify host state before
    /// reporting completion; ordinary UI mutations remain asynchronous.
    pub async fn reconcile_effect(&self, kind: EffectKind) -> EffectStatus {
        match self
            .actor
            .call(
                |reply| Message::ReconcileEffect { kind, reply },
                Some(Duration::from_secs(20)),
            )
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => EffectStatus {
                kind,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "effect_wait_failed",
                    message: format!("the shared effect owner did not settle: {other:?}"),
                    retryable: true,
                },
            },
        }
    }
    pub fn snapshot(&self) -> EffectsSnapshot {
        self.status.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<EffectsSnapshot> {
        self.status.clone()
    }
    pub fn reconcile(&self, inputs: ApplicationEffectInputs) {
        if self.shutdown.is_cancelled() {
            return;
        }
        if let Err(error) = self.actor.cast(Message::Publish {
            slice: Slice::Full(Box::new(inputs)),
            refresh: true,
            full: true,
            requested: Vec::new(),
        }) {
            tracing::warn!(%error, "effects reconcile could not be queued");
        }
    }
    pub async fn shutdown(&self) -> Vec<EffectStatus> {
        match self
            .actor
            .call(Message::Shutdown, Some(Duration::from_secs(10)))
            .await
        {
            Ok(CallResult::Success(statuses)) => statuses,
            result => vec![EffectStatus {
                kind: EffectKind::SystemProxy,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "effects_shutdown_unresolved",
                    message: format!("{result:?}"),
                    retryable: false,
                },
            }],
        }
    }
    #[cfg(test)]
    async fn retry_deadline(&self) -> Option<(tokio::time::Instant, tokio::task::Id)> {
        match self.actor.call(Message::RetryDeadline, None).await.unwrap() {
            CallResult::Success(timer) => timer,
            _ => panic!("retry deadline reply dropped"),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn barrier(&self) {
        assert!(matches!(
            self.actor
                .call(Message::Barrier, Some(Duration::from_secs(5)))
                .await,
            Ok(CallResult::Success(()))
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        application::ChimeraAppConfig, clash::config::ClashConfig,
        runtime::executor::ResolvedPortBindings,
    };
    use tokio::sync::mpsc;

    struct RecordingEffectsPort {
        plans: mpsc::UnboundedSender<ApplicationEffectPlan>,
    }

    #[async_trait::async_trait]
    impl ApplicationEffectsPort for RecordingEffectsPort {
        async fn apply(
            &self,
            revision: EffectRevision,
            plan: ApplicationEffectPlan,
        ) -> Vec<EffectStatus> {
            let _ = self.plans.send(plan.clone());
            plan.effects()
                .iter()
                .map(|effect| EffectStatus {
                    kind: effect.kind(),
                    desired_revision: revision,
                    applied_revision: revision,
                    health: EffectHealth::Healthy,
                })
                .collect()
        }

        async fn shutdown(&self) -> Vec<EffectStatus> {
            Vec::new()
        }
    }

    /// A side-effect already admitted by the owner is not safe to abort:
    /// it may be changing system proxy settings when shutdown begins.
    struct BlockingEffectsPort {
        started: mpsc::UnboundedSender<()>,
        release: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        completed: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl ApplicationEffectsPort for BlockingEffectsPort {
        async fn apply(
            &self,
            revision: EffectRevision,
            plan: ApplicationEffectPlan,
        ) -> Vec<EffectStatus> {
            let done = self.release.lock().await.take().expect("one gated effect");
            let _ = self.started.send(());
            let _ = done.await;
            self.completed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            plan.effects()
                .iter()
                .map(|effect| EffectStatus {
                    kind: effect.kind(),
                    desired_revision: revision,
                    applied_revision: revision,
                    health: EffectHealth::Healthy,
                })
                .collect()
        }

        async fn shutdown(&self) -> Vec<EffectStatus> {
            assert!(
                self.completed.load(std::sync::atomic::Ordering::SeqCst),
                "effect must settle before its owner calls shutdown"
            );
            Vec::new()
        }
    }

    #[tokio::test]
    async fn shutdown_drains_inflight_platform_effect_without_aborting_it() {
        let (started, mut receiver) = mpsc::unbounded_channel();
        let (release, gate) = tokio::sync::oneshot::channel();
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(BlockingEffectsPort {
                started,
                release: tokio::sync::Mutex::new(Some(gate)),
                completed: completed.clone(),
            }),
            initial: inputs(),
        })
        .await
        .expect("effects actor");

        CommitNotifications::profiles_committed(&effects, Vec::new());
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("effect began")
            .expect("effect owner is active");
        let stopping = tokio::spawn(async move { effects.shutdown().await });
        tokio::task::yield_now().await;
        assert!(!completed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            !stopping.is_finished(),
            "shutdown cannot abort an active effect"
        );
        release.send(()).expect("worker was not cancelled");
        let reported = tokio::time::timeout(Duration::from_secs(5), stopping)
            .await
            .expect("shutdown settled")
            .expect("shutdown task returned");
        assert!(reported.is_empty(), "effect port shutdown completed");
        assert!(completed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn root_shutdown_drains_effects_and_refuses_new_work() {
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let (started, mut receiver) = mpsc::unbounded_channel();
        let (release, gate) = tokio::sync::oneshot::channel();
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let effects = EffectsClient::spawn_tracked(
            EffectsArgs {
                port: Arc::new(BlockingEffectsPort {
                    started,
                    release: tokio::sync::Mutex::new(Some(gate)),
                    completed: completed.clone(),
                }),
                initial: inputs(),
            },
            shutdown.child_token(),
            &tasks,
        )
        .await
        .expect("tracked effects actor");

        CommitNotifications::profiles_committed(&effects, Vec::new());
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("effect began")
            .expect("effect owner is active");
        shutdown.cancel();
        tasks.close();
        CommitNotifications::profiles_committed(&effects, Vec::new());
        assert!(effects.retry_now(EffectKind::Tray).is_err());
        assert!(!completed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!tasks.is_empty(), "the in-flight group must remain tracked");
        release.send(()).expect("worker was not cancelled");
        tokio::time::timeout(Duration::from_secs(5), tasks.wait())
            .await
            .expect("tracked effects owner must drain");
        assert!(completed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            receiver.try_recv().is_err(),
            "no new work may start after cancel"
        );
    }

    struct RetryLocalePort;
    #[async_trait::async_trait]
    impl ApplicationEffectsPort for RetryLocalePort {
        async fn apply(
            &self,
            revision: EffectRevision,
            plan: ApplicationEffectPlan,
        ) -> Vec<EffectStatus> {
            plan.effects()
                .iter()
                .map(|effect| {
                    if effect.kind() == EffectKind::Locale {
                        super::super::status::degraded(
                            effect.kind(),
                            revision,
                            "locale_failed",
                            "temporary failure".into(),
                            true,
                        )
                    } else {
                        super::super::status::healthy(effect.kind(), revision)
                    }
                })
                .collect()
        }
        async fn shutdown(&self) -> Vec<EffectStatus> {
            Vec::new()
        }
    }

    // Adapt the upstream stale retry callback regression to the local explicit
    // shutdown contract and preserve the timer identity across stale ticks.
    #[tokio::test(start_paused = true)]
    async fn stale_retry_callback_keeps_replacement_timer_and_shutdown_cancels_it() {
        let client = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RetryLocalePort),
            initial: inputs(),
        })
        .await
        .unwrap();
        let mut desired = inputs();
        desired.app.language = chimera_config::application::I18nLanguage::Korean;
        client.application_committed(desired.app, Vec::new());
        let mut snapshots = client.subscribe();
        snapshots
            .wait_for(|s| {
                s.effects.iter().any(|e| {
                    e.status.kind == EffectKind::Locale
                        && e.health == ConvergenceHealth::RetryScheduled
                })
            })
            .await
            .unwrap();
        client.barrier().await;
        let first = client.retry_deadline().await.unwrap().0;
        tokio::time::advance(Duration::from_millis(500)).await;
        client.retry_now(EffectKind::Locale).unwrap();
        snapshots
            .wait_for(|s| {
                s.effects.iter().any(|e| {
                    e.status.kind == EffectKind::Locale
                        && e.attempts == 2
                        && e.health == ConvergenceHealth::RetryScheduled
                })
            })
            .await
            .unwrap();
        client.barrier().await;
        let replacement = client.retry_deadline().await.unwrap();
        assert!(replacement.0 > first);
        client.actor.cast(Message::Tick(first)).unwrap();
        client.barrier().await;
        assert_eq!(client.retry_deadline().await, Some(replacement));
        client.shutdown().await;
        assert_eq!(client.retry_deadline().await, None);
    }

    fn inputs() -> ApplicationEffectInputs {
        ApplicationEffectInputs::project(
            &ChimeraAppConfig::default(),
            &ClashConfig::default(),
            None,
        )
    }

    #[test]
    fn publishing_one_domain_preserves_the_other_domain_and_runtime_ports() {
        let initial = inputs();
        let mut app = initial.app.clone();
        app.enable_tray_text = !app.enable_tray_text;
        let after_app = merge_slice(initial.clone(), Slice::Application(Box::new(app.clone())));

        assert_eq!(after_app.app, app);
        assert_eq!(after_app.clash, initial.clash);
        assert_eq!(after_app.ports, initial.ports);

        let mut clash = initial.clash.clone();
        clash.enable_tun_mode = !clash.enable_tun_mode;
        let after_clash = merge_slice(after_app, Slice::Clash(clash.clone()));

        assert_eq!(after_clash.app, app);
        assert_eq!(after_clash.clash, clash);
        assert_eq!(after_clash.ports, initial.ports);

        assert_eq!(
            merge_slice(after_clash.clone(), Slice::Profiles),
            after_clash
        );
    }

    #[tokio::test]
    async fn profile_commit_notifies_the_tray_without_replaying_other_effects() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let initial = inputs();
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RecordingEffectsPort { plans: sender }),
            initial: initial.clone(),
        })
        .await
        .expect("effects actor");

        let mut latest = initial.clone();
        latest.app.enable_tray_text = true;
        CommitNotifications::application_committed(&effects, latest.app.clone(), Vec::new());
        let config_plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("application commit effect plan")
            .expect("recorded effect plan");
        assert_eq!(
            config_plan.effects(),
            &[ApplicationEffect::Tray(
                TrayRefresh::Part,
                latest.tray_view(),
            )]
        );

        CommitNotifications::profiles_committed(&effects, Vec::new());
        let profile_plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("profile commit effect plan")
            .expect("recorded effect plan");
        assert_eq!(
            profile_plan.effects(),
            &[ApplicationEffect::Tray(
                TrayRefresh::Part,
                latest.tray_view(),
            )]
        );
        let _ = effects.shutdown().await;
    }

    #[tokio::test]
    async fn tray_icon_refresh_uses_latest_inputs_without_replaying_other_effects() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let initial = inputs();
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RecordingEffectsPort { plans: sender }),
            initial: initial.clone(),
        })
        .await
        .unwrap();
        let mut latest = initial;
        latest.clash.enable_tun_mode = true;
        CommitNotifications::clash_committed(&effects, latest.clash.clone());
        let _ = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        effects.request_tray_refresh().unwrap();
        let plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            plan.effects(),
            &[
                ApplicationEffect::WidgetLogo(super::super::plan::WidgetLogoDesired {
                    source: chimera_config::application::NetworkStatisticWidgetLogoSource::Tray,
                    state: super::super::plan::WidgetLogoState::Normal,
                    icon_revision: 1,
                }),
                ApplicationEffect::Tray(TrayRefresh::Part, latest.tray_view())
            ]
        );
        effects.request_widget_logo_refresh().unwrap();
        let plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            plan.effects(),
            &[ApplicationEffect::WidgetLogo(
                super::super::plan::WidgetLogoDesired {
                    source: chimera_config::application::NetworkStatisticWidgetLogoSource::Tray,
                    state: super::super::plan::WidgetLogoState::Normal,
                    icon_revision: 2,
                }
            )]
        );
        let _ = effects.shutdown().await;
    }

    #[tokio::test]
    async fn runtime_port_binding_gives_system_proxy_the_effective_mixed_port() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut initial = inputs();
        initial.app.enable_system_proxy = true;
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RecordingEffectsPort { plans: sender }),
            initial,
        })
        .await
        .expect("effects actor");

        CommitNotifications::runtime_bound(
            &effects,
            Some(ResolvedPortBindings {
                mixed_port: 17890,
                ..Default::default()
            }),
            false,
        );
        let plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("system proxy effect plan")
            .expect("recorded effect plan");

        assert!(matches!(
            plan.effects(),
            [ApplicationEffect::SystemProxy(desired)] if desired.port == Some(17890)
        ));
        let _ = effects.shutdown().await;
    }

    #[tokio::test]
    async fn a_new_core_runtime_reapplies_transparent_proxy_when_its_port_is_unchanged() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let ports = ResolvedPortBindings {
            mixed_port: 17890,
            ..Default::default()
        };
        let mut initial = inputs();
        initial.ports = Some(ports.clone());
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RecordingEffectsPort { plans: sender }),
            initial,
        })
        .await
        .expect("effects actor");

        CommitNotifications::runtime_bound(&effects, Some(ports), false);
        let plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("transparent proxy effect plan")
            .expect("recorded effect plan");

        assert!(matches!(
            plan.effects(),
            [ApplicationEffect::TransparentProxy(_)]
        ));
        let _ = effects.shutdown().await;
    }

    #[tokio::test]
    async fn confirmed_action_can_wait_for_the_shared_system_proxy_owner() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let mut desired = inputs();
        desired.app.enable_system_proxy = true;
        desired.ports = Some(ResolvedPortBindings {
            mixed_port: 17890,
            ..Default::default()
        });
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RecordingEffectsPort { plans: sender }),
            initial: inputs(),
        })
        .await
        .expect("effects actor");

        effects.reconcile(desired);
        let status = effects.reconcile_effect(EffectKind::SystemProxy).await;

        assert_eq!(status.health, EffectHealth::Healthy);
        assert!(status.applied_revision >= status.desired_revision);
        let _ = effects.shutdown().await;
    }
}

impl CommitNotifications for EffectsClient {
    fn application_committed(&self, fields: ApplicationEffectFields, requested: Vec<EffectKind>) {
        self.publish(
            Slice::Application(Box::new(fields)),
            false,
            false,
            requested,
        );
    }

    fn clash_committed(&self, fields: ClashEffectFields) {
        self.publish(Slice::Clash(fields), false, false, Vec::new());
    }

    fn profiles_committed(&self, _: Vec<chimera_config::profile::ProfileId>) {
        self.publish(Slice::Profiles, true, false, Vec::new());
    }

    fn runtime_bound(&self, ports: Option<ResolvedPortBindings>, refresh: bool) {
        self.publish(Slice::Ports(ports), refresh, false, Vec::new());
    }

    fn publish_full(&self, ports: Option<ResolvedPortBindings>) {
        self.publish(Slice::Ports(ports), true, true, Vec::new());
    }
}

impl EffectsClient {
    fn publish(&self, slice: Slice, refresh: bool, full: bool, requested: Vec<EffectKind>) {
        if self.shutdown.is_cancelled() {
            return;
        }
        if let Err(error) = self.actor.cast(Message::Publish {
            slice,
            refresh,
            full,
            requested,
        }) {
            tracing::warn!(%error, "committed effects could not be queued");
        }
    }
}

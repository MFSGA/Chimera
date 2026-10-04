//! Owns peripheral desired state and independent, coalesced execution groups.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::watch;

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectFields, ApplicationEffectInputs, ApplicationEffectPlan,
        ClashEffectFields, EffectKind, TrayRefresh,
    },
    ports::{ApplicationEffectsPort, CommitNotifications},
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::{
    UiEventSink,
    convergence::{ConvergenceHealth, RetryBudget},
};
use chimera_config::runtime::executor::ResolvedPortBindings;

#[derive(Clone, Debug, Default)]
pub struct EffectsSnapshot {
    pub event_seq: u64,
    #[cfg(test)]
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
pub(crate) struct EffectsClient {
    actor: ActorRef<Message>,
    status: watch::Receiver<EffectsSnapshot>,
}

pub(crate) struct EffectsArgs {
    pub port: Arc<dyn ApplicationEffectsPort>,
    pub ui: Arc<dyn UiEventSink>,
    pub initial: ApplicationEffectInputs,
}

struct EffectsActor;
struct Args {
    dependencies: EffectsArgs,
    status: watch::Sender<EffectsSnapshot>,
}
struct State {
    port: Arc<dyn ApplicationEffectsPort>,
    ui: Arc<dyn UiEventSink>,
    desired: ApplicationEffectInputs,
    revision: u64,
    pending: BTreeMap<EffectKind, ApplicationEffect>,
    entries: BTreeMap<EffectKind, Entry>,
    waiters: BTreeMap<(EffectKind, EffectRevision), Vec<RpcReplyPort<EffectStatus>>>,
    active: [Option<tokio::task::JoinHandle<()>>; 3],
    status: watch::Sender<EffectsSnapshot>,
    closed: bool,
    timer: tokio::task::JoinHandle<()>,
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
    Completed {
        group: usize,
        revision: EffectRevision,
        kinds: Vec<EffectKind>,
        statuses: Vec<EffectStatus>,
    },
    Tick,
    RetryNow(EffectKind),
    Shutdown(RpcReplyPort<Vec<EffectStatus>>),
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
}

/// One owner sends only its latest slice. The actor merges slices before it
/// diffs effects, so an update cannot overwrite another domain's newer state.
enum Slice {
    Application(Box<ApplicationEffectFields>),
    Clash(ClashEffectFields),
    Ports(Option<ResolvedPortBindings>),
    Profiles,
    Full(Box<ApplicationEffectInputs>),
}

fn merge_slice(mut inputs: ApplicationEffectInputs, slice: Slice) -> ApplicationEffectInputs {
    match slice {
        Slice::Application(app) => inputs.app = *app,
        Slice::Clash(clash) => inputs.clash = clash,
        Slice::Ports(ports) => inputs.ports = ports,
        Slice::Profiles => {}
        Slice::Full(complete) => inputs = *complete,
    }
    inputs
}

fn group(kind: EffectKind) -> usize {
    match kind {
        // One owner: the system proxy actor also applies auto-launch.
        EffectKind::SystemProxy | EffectKind::ProxyGuard | EffectKind::AutoLaunch => 0,
        EffectKind::Hotkeys => 1,
        EffectKind::Locale | EffectKind::Logger | EffectKind::Widget | EffectKind::Tray => 2,
    }
}

impl State {
    fn publish(&self) {
        let event_seq = self.status.borrow().event_seq + 1;
        self.status.send_replace(EffectsSnapshot {
            event_seq,
            #[cfg(test)]
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
            effects.push(ApplicationEffect::Tray(TrayRefresh::Part));
        }
        if effects.is_empty() {
            return;
        }
        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        for mut effect in effects {
            let kind = effect.kind();
            if matches!(
                self.pending.get(&kind),
                Some(ApplicationEffect::Tray(TrayRefresh::Full))
            ) {
                effect = ApplicationEffect::Tray(TrayRefresh::Full);
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

    fn drive(&mut self, myself: &ActorRef<Message>) {
        for index in 0..3 {
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
            let ui = self.ui.clone();
            let actor = myself.clone();
            self.active[index] = Some(tokio::spawn(async move {
                let work = async {
                    if index == 2 {
                        ui.refresh_clash();
                    }
                    port.apply(revision, ApplicationEffectPlan::from_effects(effects))
                        .await
                };
                let statuses = std::panic::AssertUnwindSafe(work)
                    .catch_unwind()
                    .await
                    .unwrap_or_default();
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
        myself: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            port: args.dependencies.port,
            ui: args.dependencies.ui,
            desired: args.dependencies.initial,
            revision: 0,
            pending: BTreeMap::new(),
            entries: BTreeMap::new(),
            waiters: BTreeMap::new(),
            active: [None, None, None],
            status: args.status,
            closed: false,
            timer: myself.send_interval(Duration::from_millis(250), || Message::Tick),
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
            } if !state.closed => {
                let inputs = merge_slice(state.desired.clone(), slice);
                let changed: Vec<_> = ApplicationEffectPlan::diff(&state.desired, &inputs)
                    .effects()
                    .iter()
                    .map(ApplicationEffect::kind)
                    .collect();
                state.enqueue(inputs, refresh, full);
                for kind in requested {
                    if !changed.contains(&kind) {
                        state.retry(kind, false);
                    }
                }
                state.drive(&myself);
            }
            Message::Publish { .. } => {}
            Message::ReconcileHotkeys { inputs, reply } if !state.closed => {
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
            Message::Completed {
                group: completed_group,
                revision,
                kinds,
                statuses,
            } if !state.closed => {
                state.active[completed_group] = None;
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
            Message::Tick if !state.closed => {
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
            Message::RetryNow(kind) if !state.closed => {
                state.retry(kind, false);
                state.drive(&myself);
            }
            Message::Tick | Message::RetryNow(_) => {}
            Message::Shutdown(reply) => {
                state.closed = true;
                state.timer.abort();
                state.pending.clear();
                for task in &mut state.active {
                    if let Some(task) = task.take() {
                        task.abort();
                        let _ = task.await;
                    }
                }
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
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.timer.abort();
        for task in &mut state.active {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
        Ok(())
    }
}

impl EffectsClient {
    pub async fn spawn(args: EffectsArgs) -> anyhow::Result<Self> {
        let (status, receiver) = watch::channel(EffectsSnapshot::default());
        let (actor, _) = Actor::spawn(
            None,
            EffectsActor,
            Args {
                dependencies: args,
                status,
            },
        )
        .await?;
        Ok(Self {
            actor,
            status: receiver,
        })
    }

    pub fn retry_now(&self, kind: EffectKind) -> anyhow::Result<()> {
        self.actor
            .cast(Message::RetryNow(kind))
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
    pub fn snapshot(&self) -> EffectsSnapshot {
        self.status.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<EffectsSnapshot> {
        self.status.clone()
    }
    pub fn reconcile(&self, inputs: ApplicationEffectInputs) {
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
    use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
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
        let effects = EffectsClient::spawn(EffectsArgs {
            port: Arc::new(RecordingEffectsPort { plans: sender }),
            ui: Arc::new(crate::client::NoopUiEventSink),
            initial: inputs(),
        })
        .await
        .expect("effects actor");

        CommitNotifications::profiles_committed(&effects);
        let plan = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("profile commit effect plan")
            .expect("recorded effect plan");

        assert_eq!(
            plan.effects(),
            &[ApplicationEffect::Tray(TrayRefresh::Part)]
        );
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

    fn profiles_committed(&self) {
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

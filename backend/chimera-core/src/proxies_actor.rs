//! Actor-owned proxy cache shared by IPC and tray adapters.
use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode;
use clash_api::{IndexMap, ProviderName, ProxyName};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::{sync::watch, time::Instant};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    actor_v2::api::ApiClient,
    clash::proxies::{NativeProxies as Proxies, ProxyProviderSummary},
    connections::{ConnectionScope, interrupt_connections},
};

/// Host adapter for obtaining the current instance-bound controller lease.
/// The actor and its cache remain independent of Tauri and Service IPC.
#[async_trait::async_trait]
pub trait ProxyApiClientSource: Send + Sync {
    async fn api_client(&self) -> Result<ApiClient>;
}

/// Small authority handle copied from ref's CoreClient role. It never owns
/// endpoint lifecycle; it only asks the host adapter for the current lease.
#[derive(Clone)]
pub struct CoreClient(Arc<dyn ProxyApiClientSource>);

impl CoreClient {
    pub fn new(source: Arc<dyn ProxyApiClientSource>) -> Self {
        Self(source)
    }

    #[cfg(test)]
    fn from_source(source: Arc<dyn ProxyApiClientSource>) -> Self {
        Self(source)
    }

    async fn api_client(&self) -> Result<ApiClient> {
        self.0.api_client().await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyDegradationPhase {
    SystemEffect,
    UiEffect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyDegradation {
    pub phase: ProxyDegradationPhase,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyMutationOutcome {
    pub degradations: Vec<ProxyDegradation>,
}

impl ProxyMutationOutcome {
    pub fn degradations(&self) -> &[ProxyDegradation] {
        &self.degradations
    }
}

struct Snapshot {
    api: ApiClient,
    proxies: Proxies,
    providers: IndexMap<ProviderName, ProxyProviderSummary>,
    fetched: Instant,
    fingerprint: u64,
}

enum Message {
    Read {
        force: bool,
        reply: RpcReplyPort<Result<Arc<Snapshot>>>,
    },
    Select {
        group: String,
        name: String,
        strategy: ProxyChangeBreakMode,
        reply: RpcReplyPort<Result<ProxyMutationOutcome>>,
    },
    ClearFixed {
        group: String,
        strategy: ProxyChangeBreakMode,
        reply: RpcReplyPort<Result<ProxyMutationOutcome>>,
    },
    Provider {
        name: String,
        action: ProviderAction,
        reply: RpcReplyPort<Result<()>>,
    },
    Refresh,
    /// The sidebar arrangement of the groups changed; the snapshot did not.
    Rearranged,
    Invalidated(u64),
    Invalidate,
}

/// A provider-wide request whose result changes the cached nodes.
enum ProviderAction {
    Update,
    Healthcheck,
}

/// A user's change to which member carries a group's traffic.
enum RouteChange {
    Select { name: String },
    ClearFixed,
}

impl RouteChange {
    /// What a degradation message reports as done.
    fn done(&self) -> &'static str {
        match self {
            Self::Select { .. } => "proxy selected",
            Self::ClearFixed => "pinned selection cleared",
        }
    }
}

struct ProxiesActor;
struct Args {
    core: CoreClient,
    snapshots: watch::Sender<Option<Arc<Snapshot>>>,
    changes: watch::Sender<()>,
    refresh_queued: Arc<AtomicBool>,
    shutdown: CancellationToken,
}
struct State {
    core: CoreClient,
    snapshots: watch::Sender<Option<Arc<Snapshot>>>,
    changes: watch::Sender<()>,
    cache: Option<Arc<Snapshot>>,
    generation: u64,
    monitor: Option<tokio::task::JoinHandle<()>>,
    timer: Option<tokio::task::JoinHandle<()>>,
    /// Set while a refresh request waits in the mailbox, so a burst of
    /// requests (a delay test per node) rereads once rather than once each.
    refresh_queued: Arc<AtomicBool>,
    shutdown: CancellationToken,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = self.monitor.take() {
            task.abort();
        }
        if let Some(task) = self.timer.take() {
            task.abort();
        }
    }
}
impl State {
    fn clear(&mut self) {
        if let Some(task) = self.monitor.take() {
            task.abort();
        }
        if self.cache.take().is_some() {
            self.snapshots.send_replace(None);
            self.changes.send_replace(());
        }
    }

    async fn read(&mut self, actor: &ActorRef<Message>, force: bool) -> Result<Arc<Snapshot>> {
        let api = match self.core.api_client().await {
            Ok(api) => api,
            Err(error) => {
                self.clear();
                return Err(error);
            }
        };
        if let Some(cache) = &self.cache {
            if cache.api.same_instance(&api) {
                if !force && cache.fetched.elapsed() < Duration::from_secs(3) {
                    return Ok(cache.clone());
                }
            } else {
                self.clear();
            }
        }
        self.refresh(actor, api).await
    }

    async fn refresh(
        &mut self,
        actor: &ActorRef<Message>,
        api: ApiClient,
    ) -> Result<Arc<Snapshot>> {
        let result = async {
            let response = api.proxy_snapshot().await?;
            let proxies =
                Proxies::from_responses(response.proxies, &response.providers, response.groups)?;
            // Provider nodes are needed above, but only their metadata stays in the cache.
            let providers = response
                .providers
                .into_iter()
                .map(|(name, provider)| (name, ProxyProviderSummary::from(provider)))
                .collect();
            // Hash the application view without retaining its serialized bytes.
            let mut fingerprint = DefaultHasher::new();
            fingerprint.write(&serde_json::to_vec(&(&proxies, &providers))?);
            anyhow::ensure!(
                !api.is_revoked(),
                "core instance retired during proxy assembly"
            );
            anyhow::ensure!(
                !self.shutdown.is_cancelled(),
                "proxy owner shut down during snapshot assembly"
            );
            Ok::<_, anyhow::Error>(Arc::new(Snapshot {
                api: api.clone(),
                proxies,
                providers,
                fetched: Instant::now(),
                fingerprint: fingerprint.finish(),
            }))
        }
        .await;
        match result {
            Ok(snapshot) => {
                anyhow::ensure!(
                    !self.shutdown.is_cancelled(),
                    "proxy owner shut down before snapshot publication"
                );
                let changed = self
                    .cache
                    .as_ref()
                    .is_none_or(|old| old.fingerprint != snapshot.fingerprint);
                self.cache = Some(snapshot.clone());
                self.snapshots.send_replace(Some(snapshot.clone()));
                if changed {
                    self.changes.send_replace(());
                }
                if let Some(task) = self.monitor.take() {
                    task.abort();
                }
                self.generation += 1;
                let generation = self.generation;
                let actor = actor.clone();
                self.monitor = Some(tokio::spawn(async move {
                    api.cancelled().await;
                    let _ = actor.cast(Message::Invalidated(generation));
                }));
                Ok(snapshot)
            }
            Err(error) => {
                self.clear();
                Err(error)
            }
        }
    }

    /// The core may have applied a mutation it reported as failed, so re-read it and publish the
    /// truth. The mutation's own error is returned unchanged.
    async fn reread_after_rejection(
        &mut self,
        actor: &ActorRef<Message>,
        api: ApiClient,
        mutation: Result<()>,
    ) -> Result<()> {
        if mutation.is_err()
            && let Err(error) = self.refresh(actor, api).await
        {
            tracing::debug!(%error, "proxy cache refresh after a rejected mutation failed");
        }
        mutation
    }

    async fn change_route(
        &mut self,
        actor: &ActorRef<Message>,
        group: String,
        change: RouteChange,
        strategy: ProxyChangeBreakMode,
    ) -> Result<ProxyMutationOutcome> {
        // The published snapshot stays until the core answers: clearing it first would hand every
        // subscriber an empty proxy list, and a rejected change would leave it empty.
        let api = match self.core.api_client().await {
            Ok(api) => api,
            Err(error) => {
                self.clear();
                return Err(error);
            }
        };
        let group_name = ProxyName::from(group.clone());
        let applied = match &change {
            RouteChange::Select { name } => {
                api.select_proxy(&group_name, &name.clone().into()).await
            }
            RouteChange::ClearFixed => api.clear_proxy_selection(&group_name).await,
        }
        .map_err(anyhow::Error::from);
        self.reread_after_rejection(actor, api.clone(), applied)
            .await?;
        // Keep every follow-up on the change's revocable source capability.
        let interruption = match strategy {
            ProxyChangeBreakMode::Off => Ok(()),
            ProxyChangeBreakMode::ProxyGroup => {
                interrupt_connections(
                    &api,
                    &ConnectionScope::ProxyGroup {
                        name: group.clone(),
                    },
                )
                .await
            }
            ProxyChangeBreakMode::All => interrupt_connections(&api, &ConnectionScope::All).await,
        };
        let mut degradations = Vec::new();
        if let Err(error) = interruption {
            degradations.push(ProxyDegradation {
                phase: ProxyDegradationPhase::SystemEffect,
                code: "proxy_interruption_failed".into(),
                message: format!(
                    "{}, but source-instance connection interruption failed: {error}",
                    change.done()
                ),
                retryable: false,
            });
        }
        if let Err(error) = self.refresh(actor, api).await {
            degradations.push(ProxyDegradation {
                phase: ProxyDegradationPhase::UiEffect,
                code: "proxy_cache_refresh_failed".into(),
                message: format!("{}, but cache refresh failed: {error}", change.done()),
                retryable: true,
            });
        }
        Ok(ProxyMutationOutcome { degradations })
    }
}

impl Actor for ProxiesActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;
    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            core: args.core,
            snapshots: args.snapshots,
            changes: args.changes,
            cache: None,
            generation: 0,
            monitor: None,
            timer: None,
            refresh_queued: args.refresh_queued,
            shutdown: args.shutdown,
        })
    }
    async fn post_start(
        &self,
        actor: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.timer = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if actor
                    .call(|reply| Message::Read { force: true, reply }, None)
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(())
    }
    async fn handle(
        &self,
        actor: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if state.shutdown.is_cancelled() {
            match message {
                Message::Read { reply, .. } => {
                    let _ = reply.send(Err(anyhow::anyhow!("proxy owner is shutting down")));
                }
                Message::Select { reply, .. } => {
                    let _ = reply.send(Err(anyhow::anyhow!("proxy owner is shutting down")));
                }
                Message::ClearFixed { reply, .. } => {
                    let _ = reply.send(Err(anyhow::anyhow!("proxy owner is shutting down")));
                }
                Message::Provider { reply, .. } => {
                    let _ = reply.send(Err(anyhow::anyhow!("proxy owner is shutting down")));
                }
                Message::Refresh
                | Message::Rearranged
                | Message::Invalidated(_)
                | Message::Invalidate => {}
            }
            return Ok(());
        }
        match message {
            Message::Read { force, reply } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let _ = reply.send(state.read(&actor, force).await);
            }
            Message::Select {
                group,
                name,
                strategy,
                reply,
            } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let _ = reply.send(
                    state
                        .change_route(&actor, group, RouteChange::Select { name }, strategy)
                        .await,
                );
            }
            Message::ClearFixed {
                group,
                strategy,
                reply,
            } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let _ = reply.send(
                    state
                        .change_route(&actor, group, RouteChange::ClearFixed, strategy)
                        .await,
                );
            }
            Message::Provider {
                name,
                action,
                reply,
            } => {
                if reply.is_closed() {
                    return Ok(());
                }
                let result = async {
                    let api = match state.core.api_client().await {
                        Ok(api) => api,
                        Err(error) => {
                            state.clear();
                            return Err(error);
                        }
                    };
                    let name = name.into();
                    let requested = match action {
                        ProviderAction::Update => api.update_proxy_provider(&name).await,
                        ProviderAction::Healthcheck => api.healthcheck_proxy_provider(&name).await,
                    }
                    .map_err(anyhow::Error::from);
                    state
                        .reread_after_rejection(&actor, api.clone(), requested)
                        .await?;
                    state.refresh(&actor, api).await.context(match action {
                        ProviderAction::Update => {
                            "provider update succeeded but cache refresh failed"
                        }
                        ProviderAction::Healthcheck => {
                            "provider health check succeeded but cache refresh failed"
                        }
                    })?;
                    Ok(())
                }
                .await;
                let _ = reply.send(result);
            }
            Message::Refresh => {
                state.refresh_queued.store(false, Ordering::Release);
                if let Err(error) = state.read(&actor, true).await {
                    tracing::debug!(%error, "proxy cache refresh failed");
                }
            }
            Message::Rearranged => {
                state.changes.send_replace(());
            }
            Message::Invalidated(generation) if generation == state.generation => state.clear(),
            Message::Invalidated(_) => {}
            Message::Invalidate => state.clear(),
        }
        Ok(())
    }
    async fn post_stop(
        &self,
        _actor: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(timer) = state.timer.take() {
            timer.abort();
        }
        Ok(())
    }
}

struct ClientInner {
    actor: ActorRef<Message>,
    snapshots: watch::Receiver<Option<Arc<Snapshot>>>,
    changes: watch::Receiver<()>,
    refresh_queued: Arc<AtomicBool>,
    shutdown: CancellationToken,
}
impl Drop for ClientInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.actor.stop(None);
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn proxy actor")]
    SpawnActor {
        #[source]
        source: ractor::SpawnErr,
    },
}

#[derive(Clone)]
pub struct ProxiesClient(Arc<ClientInner>);
impl ProxiesClient {
    #[cfg(test)]
    pub(crate) async fn stop_for_test(&self) -> Result<()> {
        self.0.actor.stop_and_wait(None, None).await?;
        Ok(())
    }

    pub async fn spawn(
        core: CoreClient,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> std::result::Result<Self, Error> {
        let (snapshots, snapshot_rx) = watch::channel(None);
        let (changes, changes_rx) = watch::channel(());
        let refresh_queued = Arc::new(AtomicBool::new(false));
        let (actor, _) = Actor::spawn(
            None,
            ProxiesActor,
            Args {
                core,
                shutdown: shutdown.clone(),
                snapshots,
                changes,
                refresh_queued: refresh_queued.clone(),
            },
        )
        .await
        .map_err(|source| Error::SpawnActor { source })?;
        let shutdown_actor = actor.clone();
        let shutdown_token = shutdown.clone();
        tasks.spawn(async move {
            shutdown_token.cancelled().await;
            let _ = shutdown_actor.stop_and_wait(None, None).await;
        });
        Ok(Self(Arc::new(ClientInner {
            actor,
            snapshots: snapshot_rx,
            changes: changes_rx,
            refresh_queued,
            shutdown,
        })))
    }
    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<Result<T>>) -> Message,
    ) -> Result<T> {
        match self.0.actor.call(message, None).await {
            Ok(ractor::rpc::CallResult::Success(result)) => result,
            _ => anyhow::bail!("proxy actor is unavailable"),
        }
    }
    pub async fn get(&self, force: bool) -> Result<Proxies> {
        let snapshot = self.call(|reply| Message::Read { force, reply }).await?;
        anyhow::ensure!(
            !snapshot.api.is_revoked(),
            "proxy snapshot belongs to a retired instance"
        );
        Ok(snapshot.proxies.clone())
    }
    pub async fn providers(&self) -> Result<IndexMap<ProviderName, ProxyProviderSummary>> {
        let snapshot = self
            .call(|reply| Message::Read {
                force: false,
                reply,
            })
            .await?;
        anyhow::ensure!(
            !snapshot.api.is_revoked(),
            "provider snapshot belongs to a retired instance"
        );
        Ok(snapshot.providers.clone())
    }
    pub async fn select(
        &self,
        group: String,
        name: String,
        strategy: ProxyChangeBreakMode,
    ) -> Result<ProxyMutationOutcome> {
        self.call(|reply| Message::Select {
            group,
            name,
            strategy,
            reply,
        })
        .await
    }
    pub async fn clear_fixed(
        &self,
        group: String,
        strategy: ProxyChangeBreakMode,
    ) -> Result<ProxyMutationOutcome> {
        self.call(|reply| Message::ClearFixed {
            group,
            strategy,
            reply,
        })
        .await
    }
    pub async fn update_provider(&self, name: String) -> Result<()> {
        self.call(|reply| Message::Provider {
            name,
            action: ProviderAction::Update,
            reply,
        })
        .await
    }
    pub async fn healthcheck_provider(&self, name: String) -> Result<()> {
        self.call(|reply| Message::Provider {
            name,
            action: ProviderAction::Healthcheck,
            reply,
        })
        .await
    }
    pub fn invalidate(&self) {
        let _ = self.0.actor.cast(Message::Invalidate);
    }
    pub fn request_refresh(&self) {
        if !self.0.refresh_queued.swap(true, Ordering::AcqRel) {
            let _ = self.0.actor.cast(Message::Refresh);
        }
    }
    /// Wakes the subscribers because the order they show the groups in changed.
    pub fn notify_rearranged(&self) {
        let _ = self.0.actor.cast(Message::Rearranged);
    }
    pub fn snapshot(&self) -> Proxies {
        self.0
            .snapshots
            .borrow()
            .as_ref()
            .filter(|snapshot| !snapshot.api.is_revoked())
            .map(|snapshot| snapshot.proxies.clone())
            .unwrap_or_default()
    }
    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.0.changes.clone()
    }
}

#[cfg(test)]
#[path = "proxies_actor_tests.rs"]
mod tests;

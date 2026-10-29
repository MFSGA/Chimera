//! Application configuration client boundary.
//!
//! Application state is actor-owned and persisted as typed `application.yaml`.
//! The legacy combined verge model remains a compatibility mirror while callers
//! are migrated to the ref-style typed facade.

use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result, bail};
use camino::Utf8PathBuf;
use chimera_config::{
    application::{ChimeraAppConfig, ChimeraAppConfigPatch},
    clash::config::ClashConfig,
    state::PersistentState,
};
use chimera_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use struct_patch::Patch;

use super::ChimeraClient;
use crate::{
    bridge::{split_legacy_verge_patch, verge::application_from_legacy},
    config::{chimera::IVerge, core::Config},
    core::handle,
    state::{
        ConditionalReplaceResult,
        application::{
            ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
        },
        mirror::{PreparedTypedReplace, VergeLegacyBridge},
    },
};

const APPLICATION_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct ApplicationClient {
    inner: Arc<ApplicationClientInner>,
}

enum ApplicationClientInner {
    Actor {
        actor_ref: ActorRef<ApplicationActorMessage>,
        snapshot: StateSnapshot<ChimeraAppConfig>,
    },
    #[cfg(test)]
    Static {
        state: parking_lot::RwLock<ChimeraAppConfig>,
    },
}

impl ApplicationClient {
    #[cfg(test)]
    pub(crate) fn legacy() -> anyhow::Result<Self> {
        Ok(Self::static_state(ChimeraAppConfig::default()))
    }

    #[cfg(test)]
    pub(crate) fn static_state(state: ChimeraAppConfig) -> Self {
        Self {
            inner: Arc::new(ApplicationClientInner::Static {
                state: parking_lot::RwLock::new(state),
            }),
        }
    }

    pub(super) async fn new(
        config_path: Utf8PathBuf,
        seed: ChimeraAppConfig,
        bridge: Arc<dyn VergeLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<ChimeraAppConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load application persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize application persistent state manager")?
        };
        let snapshot = manager.snapshot_handle();

        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs {
                manager,
                bridge: bridge.clone(),
            },
        )
        .await
        .context("failed to spawn application actor")?;

        let client = Self {
            inner: Arc::new(ApplicationClientInner::Actor {
                actor_ref,
                snapshot,
            }),
        };

        let loaded = client.get().await?.state;
        bridge
            .prepare(&loaded)
            .context("failed to prepare loaded application legacy mirror")?
            .apply();

        Ok(client)
    }

    pub(crate) fn get_legacy(&self) -> IVerge {
        Config::verge().latest().clone()
    }

    pub(crate) fn get_typed(&self) -> ChimeraAppConfig {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(test)]
            ApplicationClientInner::Static { state } => state.read().clone(),
        }
    }

    pub(crate) async fn patch_core(
        &self,
        core: chimera_config::application::ClashCore,
    ) -> anyhow::Result<()> {
        self.patch_typed(ChimeraAppConfigPatch {
            core: Some(core),
            ..Default::default()
        })
        .await?;
        Config::verge().data().save_file()?;
        handle::Handle::refresh_verge();
        Ok(())
    }

    pub(super) async fn get(&self) -> anyhow::Result<ApplicationSnapshot> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { .. } => {
                self.call(ApplicationActorMessage::Get, Some(APPLICATION_READ_TIMEOUT))
                    .await
            }
            #[cfg(test)]
            ApplicationClientInner::Static { state } => Ok(ApplicationSnapshot {
                state: state.read().clone(),
                version: 0,
            }),
        }
    }

    pub(super) async fn patch_typed(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<ApplicationSnapshot> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { .. } => {
                self.call(
                    |reply| ApplicationActorMessage::Patch { patch, reply },
                    None,
                )
                .await
            }
            #[cfg(test)]
            ApplicationClientInner::Static { state } => {
                let mut state = state.write();
                state.apply(patch);
                Ok(ApplicationSnapshot {
                    state: state.clone(),
                    version: 0,
                })
            }
        }
    }

    pub(super) async fn prepare_replace(
        &self,
        state: ChimeraAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ChimeraAppConfig>> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ApplicationActorMessage::PrepareReplace { state, reply },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("application actor call timed out"),
                }
            }
            #[cfg(test)]
            ApplicationClientInner::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub(super) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<ChimeraAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ApplicationActorMessage::ReplacePreparedIfVersion {
                            expected_version,
                            prepared,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("application actor call timed out"),
                }
            }
            #[cfg(test)]
            ApplicationClientInner::Static { state } => {
                let (next, mirror) = prepared.into_parts();
                *state.write() = next.clone();
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(ApplicationSnapshot {
                    state: next,
                    version: expected_version + 1,
                }))
            }
        }
    }

    async fn patch_legacy(&self, owner: &ChimeraClient, patch: IVerge) -> anyhow::Result<()> {
        patch_legacy_uncoordinated(owner, patch).await
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ApplicationSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ApplicationSnapshot>>) -> ApplicationActorMessage,
    {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { actor_ref, .. } => {
                match actor_ref.call(make, timeout).await? {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("application actor call timed out"),
                }
            }
            #[cfg(test)]
            ApplicationClientInner::Static { .. } => {
                anyhow::bail!("application actor is unavailable in the static test backend")
            }
        }
    }
}

impl Drop for ApplicationClientInner {
    fn drop(&mut self) {
        match self {
            ApplicationClientInner::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(test)]
            ApplicationClientInner::Static { .. } => {}
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct SystemProxyMutation<'a> {
    _guard: tokio::sync::MutexGuard<'a, ()>,
}

impl ChimeraClient {
    pub(crate) fn get_app_config(&self) -> anyhow::Result<ChimeraAppConfig> {
        Ok(self.inner.application.get_typed())
    }

    pub(crate) fn application_config(&self) -> IVerge {
        self.inner.application.get_legacy()
    }

    pub(crate) async fn expected_system_proxy_port(&self) -> anyhow::Result<Option<u16>> {
        let clash = self.inner.clash_config.get()?;
        let receipt = match self.inner.core.observe_runtime_baseline().await? {
            super::application_workflow::mutation::KnownRuntimeState::Applied(receipt) => {
                Some(receipt)
            }
            super::application_workflow::mutation::KnownRuntimeState::Stopped => {
                self.inner.core.last_confirmed_runtime_receipt()
            }
            super::application_workflow::mutation::KnownRuntimeState::NeverApplied => None,
        };
        Ok(super::ports::effective_mixed_port(
            receipt.as_deref(),
            &clash,
        ))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn active_system_proxy_port(&self) -> anyhow::Result<Option<u16>> {
        let receipt = match self.inner.core.observe_runtime_baseline().await? {
            super::application_workflow::mutation::KnownRuntimeState::Applied(receipt) => receipt,
            super::application_workflow::mutation::KnownRuntimeState::Stopped
            | super::application_workflow::mutation::KnownRuntimeState::NeverApplied => {
                return Ok(None);
            }
        };
        Ok(super::ports::confirmed_mixed_port(Some(&receipt)))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn reconcile_system_proxy_effect(
        &self,
    ) -> super::effects::status::EffectStatus {
        self.inner
            .effects
            .reconcile_effect(super::effects::plan::EffectKind::SystemProxy)
            .await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn lock_system_proxy_mutation(&self) -> SystemProxyMutation<'_> {
        SystemProxyMutation {
            _guard: self.inner.system_proxy_mutation.lock().await,
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn patch_app_config_under_system_proxy_mutation(
        &self,
        _mutation: &SystemProxyMutation<'_>,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<super::MutationOutcome<()>> {
        self.patch_app_config_inner(patch).await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn reconcile_system_proxy_effect_under_mutation(
        &self,
        _mutation: &SystemProxyMutation<'_>,
    ) -> super::effects::status::EffectStatus {
        self.reconcile_system_proxy_effect().await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn disable_stale_system_proxy(
        &self,
        expected_core_state_changed_at: i64,
        expected_port: u16,
        desired_before: bool,
        original: super::system_proxy::ports::OsProxyConfig,
    ) -> Result<(), super::system_proxy::SystemProxyActionError> {
        let _mutation = self.inner.system_proxy_mutation.lock().await;
        let Some(system_proxy) = &self.inner.system_proxy else {
            return Err(super::system_proxy::SystemProxyActionError::Stopped);
        };
        let notifications: Arc<dyn super::effects::ports::CommitNotifications> = Arc::new((
            self.inner.effects.clone(),
            super::event_sink::UiCommitNotifications(self.inner.ui_sink.clone()),
        ));
        let persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync> =
            Arc::new(persist_legacy_application_mirror);
        self.inner
            .core_lifecycle
            .disable_stale_system_proxy(
                system_proxy.clone(),
                expected_port,
                expected_core_state_changed_at,
                desired_before,
                original,
                notifications,
                persist_legacy_mirror,
            )
            .await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn restore_system_proxy_after_failed_action(
        &self,
        _mutation: &SystemProxyMutation<'_>,
        original: super::system_proxy::ports::OsProxyConfig,
    ) -> Result<(), super::system_proxy::SystemProxyActionError> {
        let Some(system_proxy) = &self.inner.system_proxy else {
            return Err(super::system_proxy::SystemProxyActionError::Stopped);
        };
        system_proxy.restore_after_failed_action(original).await
    }

    pub(crate) async fn patch_verge(&self, patch: IVerge) -> anyhow::Result<()> {
        self.inner.application.patch_legacy(self, patch).await
    }

    pub(crate) async fn patch_app_config(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<super::MutationOutcome<()>> {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let _system_proxy_mutation = if application_patch_touches_system_proxy(&patch) {
            Some(self.lock_system_proxy_mutation().await)
        } else {
            None
        };

        self.patch_app_config_inner(patch).await
    }

    async fn patch_app_config_inner(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<super::MutationOutcome<()>> {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let reconcile_hotkeys = patch.hotkeys.is_some();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if let Some(hotkeys) = patch.hotkeys.as_deref() {
            super::hotkey::validate_bindings(hotkeys, self.inner.accelerators.as_ref())?;
        }

        #[cfg(windows)]
        validate_windows_service_mode_tun_transition(
            patch.enable_service_mode,
            self.get_clash_config()?.enable_tun_mode,
            None,
        )?;

        let service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
        if super::application_workflow::impact::application_patch_requires_reconcile(
            &patch,
            service_connected,
        ) {
            self.inner
                .core_lifecycle
                .patch_application_config(patch)
                .await?;
        } else {
            self.inner.application.patch_typed(patch).await?;
        }
        let snapshot = self.inner.application.get().await?;
        let mut degradations = Vec::new();

        // Keep the legacy Verge file current while existing service and agent
        // entry points still read it. The typed application.yaml is already
        // committed, so a mirror write failure is reported as degraded.
        if let Err(error) = persist_legacy_application_mirror() {
            degradations.push(super::Degradation {
                phase: super::runtime::DegradationPhase::LegacyMirror,
                code: "legacy_application_mirror_write_failed".into(),
                message: format!("failed to persist legacy application mirror: {error:#}"),
                retryable: true,
            });
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let clash = self.get_clash_config()?;
            let inputs = super::effects::plan::ApplicationEffectInputs::project(
                &snapshot.state,
                &clash,
                None,
            );
            self.publish_runtime_port_binding().await;
            super::effects::ports::CommitNotifications::application_committed(
                &self.inner.effects,
                inputs.app,
                Vec::new(),
            );

            if reconcile_hotkeys {
                let status = self.reconcile_hotkeys().await;
                if let Some(degradation) = super::effects::status::degradation_of(&status) {
                    degradations.push(degradation);
                }
            }
        }

        self.inner.ui_sink.refresh_verge();
        Ok(
            super::MutationOutcome::from_parts((), degradations).with_commit(
                super::runtime::CommitReceipt {
                    operation_id: None,
                    domain: "application".into(),
                    source_version: snapshot.version,
                    runtime: super::runtime::RuntimeCommitStatus::Pending,
                },
            ),
        )
    }

    pub(crate) async fn patch_clash_config(
        &self,
        patch: chimera_config::clash::config::ClashConfigPatch,
    ) -> anyhow::Result<super::MutationOutcome<()>> {
        if patch.enable_tun_mode == Some(true) {
            #[cfg(target_os = "macos")]
            {
                let app = self.get_app_config()?;
                let service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
                if !(app.enable_service_mode && service_connected) {
                    let core: chimera_utils::core::CoreType = (&app.core).into();
                    tokio::task::spawn_blocking(move || {
                        crate::core::manager::grant_permission(&core)
                    })
                    .await
                    .context("macOS TUN authorization task failed")?
                    .context("failed to grant selected core the macOS TUN permission")?;
                }
            }

            #[cfg(target_os = "windows")]
            crate::core::service::ensure_tun_host_ready(self)
                .await
                .context("Windows TUN service preflight failed")?;
        }

        // Complete permission prompts before taking the proxy transaction lock;
        // the lock only needs to cover the settings write and its effect publish.
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let _system_proxy_mutation =
            if patch.mixed_port.kind.is_some() || patch.mixed_port.start_port.is_some() {
                Some(self.lock_system_proxy_mutation().await)
            } else {
                None
            };

        let snapshot = self
            .inner
            .clash_config
            .patch_typed_config(self, patch)
            .await?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let app = self.get_app_config()?;
            let clash = self.get_clash_config()?;
            let inputs = super::effects::plan::ApplicationEffectInputs::project(&app, &clash, None);
            self.publish_runtime_port_binding().await;
            super::effects::ports::CommitNotifications::clash_committed(
                &self.inner.effects,
                inputs.clash,
            );
        }
        self.inner.ui_sink.refresh_clash();

        Ok(
            super::MutationOutcome::from_parts((), Vec::new()).with_commit(
                super::runtime::CommitReceipt {
                    operation_id: None,
                    domain: "clash".into(),
                    source_version: snapshot.version,
                    runtime: super::runtime::RuntimeCommitStatus::Pending,
                },
            ),
        )
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn publish_runtime_port_binding(&self) {
        let ports = self.confirmed_effect_port_binding().await;
        super::effects::ports::CommitNotifications::runtime_bound(
            &self.inner.effects,
            ports,
            false,
        );
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn publish_full_effects(&self) {
        let ports = self.confirmed_effect_port_binding().await;
        super::effects::ports::CommitNotifications::publish_full(&self.inner.effects, ports);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn confirmed_effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        let receipt = match self.inner.core.active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::debug!(%error, "no active runtime receipt is available for application effects");
                return None;
            }
        };
        match resolved_effect_port_binding(receipt.as_deref()) {
            Some(binding) => Some(binding),
            None => {
                tracing::warn!(
                    "the active core has no confirmed mixed port for application effects"
                );
                None
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(super) fn resolved_effect_port_binding(
    receipt: Option<&super::runtime::RuntimeApplyReceipt>,
) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
    receipt
        .map(|receipt| receipt.ports.bindings().mixed_port)
        .filter(|port| *port != 0)
        .map(
            |mixed_port| chimera_config::runtime::executor::ResolvedPortBindings {
                mixed_port,
                ..Default::default()
            },
        )
}

enum PreparedLegacyDomain {
    Application {
        expected_version: u64,
        forward: PreparedTypedReplace<ChimeraAppConfig>,
        rollback: PreparedTypedReplace<ChimeraAppConfig>,
    },
    Session {
        expected_version: u64,
        forward: PreparedTypedReplace<PersistentState>,
        rollback: PreparedTypedReplace<PersistentState>,
    },
    Clash {
        expected_version: u64,
        forward: PreparedTypedReplace<ClashConfig>,
        rollback: PreparedTypedReplace<ClashConfig>,
    },
}

enum CommittedLegacyDomain {
    Application {
        committed_version: u64,
        rollback: PreparedTypedReplace<ChimeraAppConfig>,
    },
    Session {
        committed_version: u64,
        rollback: PreparedTypedReplace<PersistentState>,
    },
    Clash {
        committed_version: u64,
        rollback: PreparedTypedReplace<ClashConfig>,
    },
}

struct VergePatchPlan {
    service_mode: Option<bool>,
    auto_launch_changed: bool,
    system_proxy_changed: bool,
    proxy_bypass_changed: bool,
    proxy_guard_changed: bool,
    log_level_changed: bool,
    log_max_files_changed: bool,
    language_changed: bool,
    refresh_systray: bool,
    refresh_systray_part: bool,
}

fn plan_verge_patch(
    patch: &IVerge,
    clash_patch: Option<&chimera_config::clash::config::ClashConfigPatch>,
) -> Result<VergePatchPlan> {
    if let Some(ref theme_color) = patch.theme_color
        && !theme_color.is_empty()
        && !crate::config::chimera::is_hex_color(theme_color)
    {
        bail!("Invalid theme color: {}", theme_color);
    }

    Ok(VergePatchPlan {
        service_mode: patch.enable_service_mode,
        auto_launch_changed: patch.enable_auto_launch.is_some(),
        system_proxy_changed: patch.enable_system_proxy.is_some(),
        proxy_bypass_changed: patch.system_proxy_bypass.is_some(),
        proxy_guard_changed: patch.enable_proxy_guard.is_some(),
        log_level_changed: patch.app_log_level.is_some(),
        log_max_files_changed: patch.max_log_files.is_some(),
        language_changed: patch.language.is_some(),
        refresh_systray: patch.language.is_some() || patch.clash_tray_selector.is_some(),
        refresh_systray_part: patch.enable_system_proxy.is_some()
            || clash_patch.is_some_and(|patch| patch.enable_tun_mode.is_some()),
    })
}

#[cfg(any(target_os = "windows", test))]
fn validate_windows_service_mode_tun_transition(
    service_mode: Option<bool>,
    current_tun: bool,
    target_tun: Option<bool>,
) -> Result<()> {
    if service_mode == Some(false) && target_tun.unwrap_or(current_tun) {
        anyhow::bail!("disable TUN before disabling Service Mode on Windows");
    }
    Ok(())
}

async fn apply_verge_runtime_change(client: &ChimeraClient, plan: &VergePatchPlan) -> Result<()> {
    let ipc_state = crate::core::service::ipc::get_ipc_state();

    if let Some(service_mode) = plan.service_mode
        && ipc_state.is_connected()
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
        client.rebuild_running_config().await?;
    }

    Ok(())
}

fn requested_effects(plan: &VergePatchPlan) -> Vec<super::effects::plan::EffectKind> {
    use super::effects::plan::EffectKind;

    let mut requested = Vec::new();
    if plan.auto_launch_changed {
        requested.push(EffectKind::AutoLaunch);
    }
    if plan.system_proxy_changed || plan.proxy_bypass_changed {
        requested.push(EffectKind::SystemProxy);
    }
    if plan.proxy_guard_changed {
        requested.push(EffectKind::ProxyGuard);
    }
    if plan.log_level_changed || plan.log_max_files_changed {
        requested.push(EffectKind::Logger);
    }
    if plan.language_changed {
        requested.push(EffectKind::Locale);
    }
    if plan.refresh_systray || plan.refresh_systray_part {
        requested.push(EffectKind::Tray);
    }
    requested.sort_unstable();
    requested.dedup();
    requested
}

fn legacy_finalize_error(error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "legacy mutation may have non-reversible runtime or OS side effects and requires reconciliation: {error:#}"
    )
}

async fn rollback_legacy_domains(
    client: &ChimeraClient,
    mut committed: Vec<CommittedLegacyDomain>,
    primary: anyhow::Error,
) -> anyhow::Error {
    let mut failures = Vec::new();

    while let Some(domain) = committed.pop() {
        match domain {
            CommittedLegacyDomain::Application {
                committed_version,
                rollback,
            } => match client
                .inner
                .application
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {}
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => failures.push(
                    format!(
                        "application rollback conflict: expected {committed_version}, actual {actual_version}"
                    ),
                ),
                Err(error) => failures.push(format!("application rollback failed: {error:#}")),
            },
            CommittedLegacyDomain::Session {
                committed_version,
                rollback,
            } => match client
                .inner
                .session_state
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {}
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => failures.push(
                    format!(
                        "session rollback conflict: expected {committed_version}, actual {actual_version}"
                    ),
                ),
                Err(error) => failures.push(format!("session rollback failed: {error:#}")),
            },
            CommittedLegacyDomain::Clash {
                committed_version,
                rollback,
            } => match client
                .inner
                .clash_config
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {}
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => failures.push(
                    format!(
                        "clash rollback conflict: expected {committed_version}, actual {actual_version}"
                    ),
                ),
                Err(error) => failures.push(format!("clash rollback failed: {error:#}")),
            },
        }
    }

    if let Err(error) = Config::verge().data().save_file() {
        failures.push(format!(
            "legacy verge rollback persistence failed: {error:#}"
        ));
    }
    if let Err(error) = Config::clash().data().save_config() {
        failures.push(format!(
            "legacy clash rollback persistence failed: {error:#}"
        ));
    }
    handle::Handle::refresh_verge();

    if failures.is_empty() {
        primary
    } else {
        anyhow::anyhow!(
            "{primary:#}; compensation failures: {}",
            failures.join("; ")
        )
    }
}

async fn patch_legacy_uncoordinated(client: &ChimeraClient, patch: IVerge) -> Result<()> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let _system_proxy_mutation = if legacy_patch_touches_system_proxy(&patch) {
        Some(client.inner.system_proxy_mutation.lock().await)
    } else {
        None
    };

    let base = Config::verge().latest().clone();
    let legacy_clash = Config::clash().latest().clone();
    let mut split = split_legacy_verge_patch(&base, &patch, &legacy_clash)?;
    let application_patch = split.application.clone().unwrap_or_default();
    let plan = plan_verge_patch(&application_patch, split.clash_config.as_ref())?;

    #[cfg(target_os = "macos")]
    if macos_tun_authorization_required(
        split
            .clash_config
            .as_ref()
            .and_then(|patch| patch.enable_tun_mode),
        application_patch
            .enable_service_mode
            .or(base.enable_service_mode)
            .unwrap_or(false),
        crate::core::service::ipc::get_ipc_state().is_connected(),
    ) {
        let core = application_patch
            .clash_core
            .as_ref()
            .or(base.clash_core.as_ref())
            .cloned()
            .unwrap_or_default();
        let core: chimera_utils::core::CoreType = (&core).into();
        tokio::task::spawn_blocking(move || crate::core::manager::grant_permission(&core))
            .await
            .context("macOS TUN authorization task failed")?
            .context("failed to grant selected core the macOS TUN permission")?;
    }

    #[cfg(target_os = "windows")]
    validate_windows_service_mode_tun_transition(
        plan.service_mode,
        client.get_clash_config()?.enable_tun_mode,
        split
            .clash_config
            .as_ref()
            .and_then(|patch| patch.enable_tun_mode),
    )?;

    if split
        .clash_config
        .as_ref()
        .and_then(|patch| patch.enable_tun_mode)
        == Some(true)
    {
        crate::core::service::ensure_tun_host_ready(client)
            .await
            .context("Windows TUN service preflight failed")?;
    }

    let application_pair = if let Some(app_patch) = split.application.as_ref() {
        let snapshot = client.inner.application.get().await?;
        let mut projected = base.clone();
        projected.patch_config(app_patch.clone());
        let next = application_from_legacy(&projected)?;
        Some((snapshot, next))
    } else {
        None
    };

    let session_pair = if let Some(session_patch) = split.session_state.take() {
        let snapshot = client.inner.session_state.get().await?;
        let mut next = snapshot.state.clone();
        next.apply(session_patch);
        Some((snapshot, next))
    } else {
        None
    };

    let clash_pair = if let Some(clash_patch) = split.clash_config.as_ref() {
        let snapshot = client.inner.clash_config.get_snapshot().await?;
        let mut next = snapshot.state.clone();
        next.apply(clash_patch.clone());
        Some((snapshot, next))
    } else {
        None
    };

    let mut prepared = Vec::new();
    if let Some((snapshot, next)) = application_pair {
        prepared.push(PreparedLegacyDomain::Application {
            expected_version: snapshot.version,
            forward: client.inner.application.prepare_replace(next).await?,
            rollback: client
                .inner
                .application
                .prepare_replace(snapshot.state.clone())
                .await?,
        });
    }
    if let Some((snapshot, next)) = session_pair {
        prepared.push(PreparedLegacyDomain::Session {
            expected_version: snapshot.version,
            forward: client.inner.session_state.prepare_replace(next).await?,
            rollback: client
                .inner
                .session_state
                .prepare_replace(snapshot.state.clone())
                .await?,
        });
    }
    if let Some((snapshot, next)) = clash_pair {
        prepared.push(PreparedLegacyDomain::Clash {
            expected_version: snapshot.version,
            forward: client.inner.clash_config.prepare_replace(next).await?,
            rollback: client
                .inner
                .clash_config
                .prepare_replace(snapshot.state.clone())
                .await?,
        });
    }

    let mut committed = Vec::new();
    for domain in prepared {
        let commit_error = match domain {
            PreparedLegacyDomain::Application {
                expected_version,
                forward,
                rollback,
            } => match client
                .inner
                .application
                .replace_prepared_if_version(expected_version, forward)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                    committed.push(CommittedLegacyDomain::Application {
                        committed_version: snapshot.version,
                        rollback,
                    });
                    continue;
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => anyhow::anyhow!(
                    "application config version conflict: expected {expected_version}, actual {actual_version}"
                ),
                Err(error) => error.context("failed to commit application typed config"),
            },
            PreparedLegacyDomain::Session {
                expected_version,
                forward,
                rollback,
            } => match client
                .inner
                .session_state
                .replace_prepared_if_version(expected_version, forward)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                    committed.push(CommittedLegacyDomain::Session {
                        committed_version: snapshot.version,
                        rollback,
                    });
                    continue;
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => anyhow::anyhow!(
                    "session config version conflict: expected {expected_version}, actual {actual_version}"
                ),
                Err(error) => error.context("failed to commit session typed state"),
            },
            PreparedLegacyDomain::Clash {
                expected_version,
                forward,
                rollback,
            } => match client
                .inner
                .clash_config
                .replace_prepared_if_version(expected_version, forward)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                    committed.push(CommittedLegacyDomain::Clash {
                        committed_version: snapshot.version,
                        rollback,
                    });
                    continue;
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => anyhow::anyhow!(
                    "clash config version conflict: expected {expected_version}, actual {actual_version}"
                ),
                Err(error) => error.context("failed to commit clash typed config"),
            },
        };

        return Err(rollback_legacy_domains(client, committed, commit_error).await);
    }

    let finalize = async {
        apply_verge_runtime_change(client, &plan).await?;
        if let Some(clash_patch) = split.clash_config.as_ref() {
            client
                .inner
                .clash_config
                .apply_legacy_patch_runtime(client, clash_patch)
                .await?;
        }
        Config::verge().data().save_file()?;
        if split.clash_config.is_some() {
            Config::clash().data().save_config()?;
        }
        handle::Handle::refresh_verge();
        Ok::<_, anyhow::Error>(())
    }
    .await;

    if let Err(error) = finalize {
        return Err(rollback_legacy_domains(
            client,
            committed,
            legacy_finalize_error(error.context("failed to finalize legacy verge patch")),
        )
        .await);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if split.application.is_some() {
            let app = client.get_app_config()?;
            let clash = client.get_clash_config()?;
            let inputs = super::effects::plan::ApplicationEffectInputs::project(&app, &clash, None);
            super::effects::ports::CommitNotifications::application_committed(
                &client.inner.effects,
                inputs.app,
                requested_effects(&plan),
            );
        }
        if split.clash_config.is_some() {
            client.publish_runtime_port_binding().await;
            let app = client.get_app_config()?;
            let clash = client.get_clash_config()?;
            let inputs = super::effects::plan::ApplicationEffectInputs::project(&app, &clash, None);
            super::effects::ports::CommitNotifications::clash_committed(
                &client.inner.effects,
                inputs.clash,
            );
        }
    }

    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn application_patch_touches_system_proxy(patch: &ChimeraAppConfigPatch) -> bool {
    patch.enable_system_proxy.is_some()
        || patch.enable_proxy_guard.is_some()
        || patch.system_proxy_bypass.is_some()
        || patch.proxy_guard_interval.is_some()
        || patch.pac_url.is_some()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn legacy_patch_touches_system_proxy(patch: &IVerge) -> bool {
    patch.enable_system_proxy.is_some()
        || patch.enable_proxy_guard.is_some()
        || patch.system_proxy_bypass.is_some()
        || patch.proxy_guard_interval.is_some()
        || patch.enable_random_port.is_some()
        || patch.verge_mixed_port.is_some()
}

pub(crate) fn persist_legacy_application_mirror() -> anyhow::Result<()> {
    Config::verge()
        .data()
        .save_file()
        .context("failed to persist legacy application mirror")
}

#[cfg(target_os = "macos")]
fn macos_tun_authorization_required(
    target_tun: Option<bool>,
    service_mode: bool,
    service_connected: bool,
) -> bool {
    target_tun == Some(true) && !(service_mode && service_connected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::PreparedLegacyMirror;
    use std::sync::{Arc, Mutex};

    struct RecordingPreparedMirror {
        value: bool,
        mirrored: Arc<Mutex<Option<bool>>>,
    }

    impl PreparedLegacyMirror for RecordingPreparedMirror {
        fn apply(self: Box<Self>) {
            *self.mirrored.lock().unwrap() = Some(self.value);
        }
    }

    struct RecordingBridge {
        legacy: ChimeraAppConfig,
        mirrored: Arc<Mutex<Option<bool>>>,
    }

    impl VergeLegacyBridge for RecordingBridge {
        fn prepare(
            &self,
            snap: &ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(RecordingPreparedMirror {
                value: snap.enable_auto_check_update,
                mirrored: self.mirrored.clone(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ChimeraAppConfig> {
            Ok(self.legacy.clone())
        }
    }

    #[test]
    fn legacy_finalize_error_reports_runtime_reconciliation_requirement() {
        let error = legacy_finalize_error(anyhow::anyhow!("tray refresh failed"));
        let message = format!("{error:#}");
        assert!(message.contains("non-reversible runtime or OS side effects"));
        assert!(message.contains("requires reconciliation"));
        assert!(message.contains("tray refresh failed"));
    }

    #[test]
    fn windows_service_mode_cannot_be_disabled_while_tun_remains_enabled() {
        assert!(validate_windows_service_mode_tun_transition(Some(false), true, None).is_err());
        assert!(
            validate_windows_service_mode_tun_transition(Some(false), false, Some(true)).is_err()
        );
        assert!(
            validate_windows_service_mode_tun_transition(Some(false), true, Some(false)).is_ok()
        );
        assert!(validate_windows_service_mode_tun_transition(Some(true), true, None).is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_tun_authorization_required_only_for_local_enable() {
        assert!(macos_tun_authorization_required(Some(true), false, false));
        assert!(macos_tun_authorization_required(Some(true), true, false));
        assert!(!macos_tun_authorization_required(Some(true), true, true));
        assert!(macos_tun_authorization_required(Some(true), false, true));
        assert!(!macos_tun_authorization_required(Some(false), false, false));
        assert!(!macos_tun_authorization_required(None, false, false));
    }

    #[test]
    fn verge_patch_plan_rebuilds_full_tray_for_language_or_selector_changes() {
        let language = plan_verge_patch(
            &IVerge {
                language: Some("zh-cn".into()),
                ..IVerge::default()
            },
            None,
        )
        .unwrap();
        assert!(language.language_changed);
        assert!(language.refresh_systray);
        assert!(!language.refresh_systray_part);

        let selector = plan_verge_patch(
            &IVerge {
                clash_tray_selector: Some(crate::config::chimera::ProxiesSelectorMode::Hidden),
                ..IVerge::default()
            },
            None,
        )
        .unwrap();
        assert!(!selector.language_changed);
        assert!(selector.refresh_systray);
        assert!(!selector.refresh_systray_part);
    }

    #[test]
    fn legacy_application_changes_request_shared_effects_after_commit() {
        use super::super::effects::plan::EffectKind;

        let plan = plan_verge_patch(
            &IVerge {
                enable_auto_launch: Some(true),
                enable_system_proxy: Some(true),
                enable_proxy_guard: Some(true),
                language: Some("ja-JP".into()),
                ..IVerge::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(
            requested_effects(&plan),
            [
                EffectKind::Locale,
                EffectKind::AutoLaunch,
                EffectKind::SystemProxy,
                EffectKind::ProxyGuard,
                EffectKind::Tray,
            ]
        );
    }

    #[test]
    fn legacy_patch_without_effect_fields_does_not_request_effect_retries() {
        assert!(requested_effects(&plan_verge_patch(&IVerge::default(), None).unwrap()).is_empty());
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn effects_receive_the_confirmed_mixed_port_not_the_controller_port() {
        use std::net::TcpListener;

        use crate::client::ports::{
            CandidatePortBindings, SessionPortResolver, test_runtime_receipt,
        };

        assert!(resolved_effect_port_binding(None).is_none());
        let empty_receipt = test_runtime_receipt(CandidatePortBindings::default());
        assert!(resolved_effect_port_binding(Some(&empty_receipt)).is_none());
        let mixed_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let mixed_port = mixed_probe.local_addr().unwrap().port();
        let controller_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let controller_port = controller_probe.local_addr().unwrap().port();
        drop((mixed_probe, controller_probe));

        let mut clash = ClashConfig::default();
        clash.mixed_port = chimera_config::clash::config::clash_strategy::PortStrategy {
            kind: chimera_config::clash::config::clash_strategy::PortStrategyKind::Fixed,
            start_port: mixed_port,
        };
        clash.external_controller.port =
            chimera_config::clash::config::clash_strategy::PortStrategy {
                kind: chimera_config::clash::config::clash_strategy::PortStrategyKind::Fixed,
                start_port: controller_port,
            };
        let candidate = SessionPortResolver::default()
            .resolve_candidate(&clash)
            .unwrap();
        let receipt = test_runtime_receipt(candidate);
        assert_eq!(
            receipt
                .ports
                .bindings()
                .external_controller
                .as_deref()
                .unwrap()
                .rsplit(':')
                .next()
                .unwrap()
                .parse::<u16>()
                .unwrap(),
            controller_port
        );
        assert_eq!(
            resolved_effect_port_binding(Some(&receipt))
                .expect("confirmed runtime port should be published")
                .mixed_port,
            mixed_port
        );
    }

    #[tokio::test]
    async fn existing_typed_file_wins_over_legacy_seed_on_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("application.yaml");

        let persisted = ChimeraAppConfig {
            enable_auto_check_update: false,
            ..ChimeraAppConfig::default()
        };
        std::fs::write(&path, serde_yaml::to_string(&persisted).unwrap()).unwrap();

        let legacy = ChimeraAppConfig {
            enable_auto_check_update: true,
            ..ChimeraAppConfig::default()
        };
        let mirrored = Arc::new(Mutex::new(None));
        let bridge: Arc<dyn VergeLegacyBridge> = Arc::new(RecordingBridge {
            legacy,
            mirrored: mirrored.clone(),
        });

        let path = Utf8PathBuf::from_path_buf(path).unwrap();
        let client = ApplicationClient::new(path, bridge.snapshot_legacy().unwrap(), bridge)
            .await
            .unwrap();

        assert!(!client.get_typed().enable_auto_check_update);
        assert_eq!(*mirrored.lock().unwrap(), Some(false));
    }
}

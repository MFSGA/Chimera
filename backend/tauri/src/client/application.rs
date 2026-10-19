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
    core::{handle, sysopt},
    state::{
        ConditionalReplaceResult,
        application::{
            ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
        },
        mirror::{PreparedTypedReplace, VergeLegacyBridge},
    },
    utils,
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

impl ChimeraClient {
    pub(crate) fn get_app_config(&self) -> anyhow::Result<ChimeraAppConfig> {
        Ok(self.inner.application.get_typed())
    }

    pub(crate) fn application_config(&self) -> IVerge {
        self.inner.application.get_legacy()
    }

    pub(crate) async fn patch_verge(&self, patch: IVerge) -> anyhow::Result<()> {
        self.inner.application.patch_legacy(self, patch).await
    }

    pub(crate) async fn patch_app_config(
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
        if let Err(error) = Config::verge().data().save_file() {
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
            self.publish_runtime_port_binding();
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
            self.publish_runtime_port_binding();
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
    fn publish_runtime_port_binding(&self) {
        let ports = chimera_config::runtime::executor::ResolvedPortBindings {
            mixed_port: self.clash_info().port,
            ..Default::default()
        };
        super::effects::ports::CommitNotifications::runtime_bound(
            &self.inner.effects,
            Some(ports),
            false,
        );
    }
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
    enable_proxy_guard: bool,
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
        enable_proxy_guard: patch.enable_proxy_guard == Some(true),
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

fn run_verge_patch_side_effects(plan: &VergePatchPlan, patch: &IVerge) -> Result<()> {
    if plan.auto_launch_changed {
        sysopt::Sysopt::global().update_launch()?;
    }

    if plan.system_proxy_changed || plan.proxy_bypass_changed {
        sysopt::Sysopt::global().update_sysproxy()?;
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.enable_proxy_guard {
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.log_level_changed || plan.log_max_files_changed {
        utils::init::refresh_logger((patch.app_log_level.clone(), patch.max_log_files))?;
    }

    if plan.refresh_systray {
        if !plan.language_changed {
            handle::Handle::update_systray()?;
        }
    } else if plan.refresh_systray_part {
        handle::Handle::update_systray_part()?;
    }

    log::debug!("todo: handle other fields");
    Ok(())
}

fn legacy_finalize_error(error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "legacy mutation may have non-reversible runtime or OS side effects and requires reconciliation: {error:#}"
    )
}

fn persist_then_apply_legacy_language(
    language: Option<&str>,
    persist: impl FnOnce() -> Result<()>,
    apply_locale: impl FnOnce(&str) -> Result<()>,
) -> Result<()> {
    // Keep the process-global locale and tray unchanged when a legacy write fails.
    persist()?;

    if let Some(language) = language {
        let locale = crate::bridge::verge::locale_key_from_legacy(language)
            .map(str::to_owned)
            .unwrap_or_else(|| language.to_ascii_lowercase());
        apply_locale(&locale)?;
    }

    Ok(())
}

fn apply_legacy_locale(
    locale: &str,
    previous_locale: &str,
    mut set_locale: impl FnMut(&str),
    mut refresh_tray: impl FnMut() -> Result<()>,
) -> Result<()> {
    set_locale(locale);
    if let Err(error) = refresh_tray() {
        set_locale(previous_locale);
        return match refresh_tray() {
            Ok(()) => Err(error
                .context("failed to refresh the language tray; the previous locale was restored")),
            Err(restore_error) => Err(anyhow::anyhow!(
                "failed to refresh the language tray: {error:#}; the previous locale was restored, but refreshing its tray also failed: {restore_error:#}"
            )),
        };
    }
    Ok(())
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
    let base = Config::verge().latest().clone();
    let previous_language = client.get_app_config()?.language;
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
        run_verge_patch_side_effects(&plan, &application_patch)?;
        persist_then_apply_legacy_language(
            plan.language_changed
                .then(|| application_patch.language.as_deref())
                .flatten(),
            || {
                Config::verge().data().save_file()?;
                if split.clash_config.is_some() {
                    Config::clash().data().save_config()?;
                }
                Ok(())
            },
            |locale| {
                apply_legacy_locale(
                    locale,
                    previous_language.as_str(),
                    rust_i18n::set_locale,
                    handle::Handle::update_systray,
                )
            },
        )?;
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

    Ok(())
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
    fn legacy_language_is_not_applied_when_persistence_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let persist_events = events.clone();
        let locale_events = events.clone();

        let result = persist_then_apply_legacy_language(
            Some("ja-JP"),
            move || {
                persist_events.lock().unwrap().push("persist".to_owned());
                Err(anyhow::anyhow!("legacy save failed"))
            },
            move |locale| {
                locale_events.lock().unwrap().push(locale.to_owned());
                Ok(())
            },
        );

        assert!(result.is_err());
        assert_eq!(*events.lock().unwrap(), vec!["persist"]);
    }

    #[test]
    fn legacy_language_applies_the_canonical_locale_after_persistence() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let persist_events = events.clone();
        let locale_events = events.clone();

        persist_then_apply_legacy_language(
            Some("ja-JP"),
            move || {
                persist_events.lock().unwrap().push("persist".to_owned());
                Ok(())
            },
            move |locale| {
                locale_events
                    .lock()
                    .unwrap()
                    .push(format!("locale:{locale}"));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            *events.lock().unwrap(),
            vec!["persist".to_owned(), "locale:ja".to_owned()]
        );
    }

    #[test]
    fn legacy_locale_restores_previous_value_when_tray_refresh_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let locale_events = events.clone();
        let refresh_events = events.clone();
        let refresh_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let refresh_attempts = refresh_count.clone();

        let result = apply_legacy_locale(
            "ja",
            "en",
            move |locale| {
                locale_events
                    .lock()
                    .unwrap()
                    .push(format!("locale:{locale}"));
            },
            move || {
                refresh_events.lock().unwrap().push("refresh".to_owned());
                if refresh_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Err(anyhow::anyhow!("tray refresh failed"))
                } else {
                    Ok(())
                }
            },
        );

        assert!(result.is_err());
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "locale:ja".to_owned(),
                "refresh".to_owned(),
                "locale:en".to_owned(),
                "refresh".to_owned(),
            ]
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

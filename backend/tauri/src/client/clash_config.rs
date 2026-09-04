//! Clash desired-configuration client boundary.
//!
//! REF owns Clash desired state through a typed config client. Chimera still
//! persists the legacy Clash guard mapping plus typed runtime overrides, so
//! this transitional client keeps those storage semantics while centralizing
//! reads, validation, mutation, and running-core coordination here.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context as _, Result, bail};
use camino::Utf8PathBuf;
use chimera_config::clash::config::{
    ClashConfig, ClashConfigPatch,
    clash_strategy::{PortStrategy, PortStrategyKind},
    overrides::ClashGuardOverridesPatch,
};
use chimera_core::state::{PersistentStateManagerSetup, StateSnapshot};
use chimera_ipc::api::status::CoreState;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use serde_yaml::{Mapping, Value};
use struct_patch::Patch;

use crate::{
    config::{clash::ClashInfo, core::Config, runtime::ClashConfigOverrides},
    core::{
        clash::transaction::{RuntimePatchCoordinator, TransactionOutcome},
        handle, sysopt,
    },
    log_err,
    state::{
        ConditionalReplaceResult,
        clash_config::{
            ClashConfigActor, ClashConfigActorArgs, ClashConfigActorMessage, ClashConfigSnapshot,
        },
        mirror::{ClashLegacyBridge, PreparedTypedReplace},
    },
};

use super::{ChimeraClient, core_bridge::RunningConfigPort};

#[cfg(test)]
use super::core_bridge::LegacyRunningConfigBridge;

const CLASH_CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ClashConfigClient {
    state: Arc<ClashConfigStateBackend>,
    runtime_patch: RuntimePatchCoordinator,
    running_config: Arc<dyn RunningConfigPort>,
}

enum ClashConfigStateBackend {
    Actor {
        actor_ref: ActorRef<ClashConfigActorMessage>,
        snapshot: StateSnapshot<ClashConfig>,
    },
    #[cfg(test)]
    Static {
        state: parking_lot::RwLock<ClashConfig>,
    },
}

struct ClashPatchPlan {
    mixed_port: Option<u16>,
    mixed_port_changed: bool,
    external_controller: Option<String>,
    external_controller_changed: bool,
    mode_changed: bool,
    break_on_mode_change: bool,
    requires_restart: bool,
}

impl ClashConfigClient {
    #[cfg(test)]
    pub(crate) fn legacy() -> anyhow::Result<Self> {
        Ok(Self::with_state_and_running_config(
            ClashConfigStateBackend::Static {
                state: parking_lot::RwLock::new(ClashConfig::default()),
            },
            Arc::new(LegacyRunningConfigBridge),
        ))
    }

    pub(super) async fn new(
        config_path: Utf8PathBuf,
        seed: ClashConfig,
        bridge: Arc<dyn ClashLegacyBridge>,
        running_config: Arc<dyn RunningConfigPort>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load clash persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize clash persistent state manager")?
        };
        let snapshot = manager.snapshot_handle();
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs {
                manager,
                bridge: bridge.clone(),
            },
        )
        .await
        .context("failed to spawn clash config actor")?;

        let client = Self::with_state_and_running_config(
            ClashConfigStateBackend::Actor {
                actor_ref,
                snapshot,
            },
            running_config,
        );
        bridge
            .prepare(&client.get_typed())
            .context("failed to prepare loaded clash legacy mirror")?
            .apply();
        Ok(client)
    }

    fn with_state_and_running_config(
        state: ClashConfigStateBackend,
        running_config: Arc<dyn RunningConfigPort>,
    ) -> Self {
        Self {
            state: Arc::new(state),
            runtime_patch: RuntimePatchCoordinator::default(),
            running_config,
        }
    }

    fn get_typed(&self) -> ClashConfig {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => state.read().clone(),
        }
    }

    pub(crate) fn get(&self) -> Result<ClashConfig> {
        Ok(self.get_typed())
    }

    pub(super) async fn get_snapshot(&self) -> anyhow::Result<ClashConfigSnapshot> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { .. } => {
                self.call(
                    ClashConfigActorMessage::Get,
                    Some(CLASH_CONFIG_READ_TIMEOUT),
                )
                .await
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => Ok(ClashConfigSnapshot {
                state: state.read().clone(),
                version: 0,
            }),
        }
    }

    pub(super) async fn prepare_replace(
        &self,
        state: ClashConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ClashConfig>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::PrepareReplace { state, reply },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub(super) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::ReplacePreparedIfVersion {
                            expected_version,
                            prepared,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => {
                let (next, mirror) = prepared.into_parts();
                *state.write() = next.clone();
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(ClashConfigSnapshot {
                    state: next,
                    version: expected_version + 1,
                }))
            }
        }
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ClashConfigSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>) -> ClashConfigActorMessage,
    {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref.call(make, timeout).await? {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => {
                anyhow::bail!("clash config actor is unavailable in the static test backend")
            }
        }
    }

    pub(crate) fn get_info(&self) -> ClashInfo {
        Config::clash().latest().get_client_info()
    }

    pub(super) async fn apply_legacy_patch_runtime(
        &self,
        owner: &ChimeraClient,
        patch: &ClashConfigPatch,
    ) -> Result<()> {
        if patch.enable_tun_mode.is_some() {
            log::debug!(target: "app", "toggle tun mode");
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                use crate::utils::dirs::check_core_permission;
                let current_core = Config::verge().data().clash_core.unwrap_or_default();
                let current_core: chimera_utils::core::CoreType = (&current_core).into();
                let service_state = crate::core::service::ipc::get_ipc_state();
                if !service_state.is_connected()
                    && check_core_permission(&current_core)
                        .inspect_err(|e| {
                            log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                        })
                        .is_ok_and(|v| !v)
                {
                    log::debug!(target: "app", "clash core permission is missing, tun toggle will restart core and may still fail");
                };
            }

            update_core_config(owner).await?;
        }

        Ok(())
    }

    async fn patch(&self, owner: &ChimeraClient, patch: Mapping) -> Result<()> {
        let overrides = ClashConfigOverrides::from_mapping(&patch)?;
        self.patch_with_overrides(owner, patch, overrides).await
    }

    async fn patch_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        let patch = overrides.to_mapping();
        self.patch_with_overrides(owner, patch, overrides).await
    }

    async fn patch_running_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        let mapping = overrides.to_mapping();
        let persist_overrides = overrides.clone();
        let client = owner.clone();

        let read_port = self.running_config.clone();
        let patch_port = self.running_config.clone();

        self.runtime_patch
            .apply(
                mapping,
                move || {
                    let port = read_port.clone();
                    async move { port.read().await }
                },
                move |patch| {
                    let port = patch_port.clone();
                    async move { port.patch(&patch).await }
                },
                move |_patch| {
                    let overrides = persist_overrides.clone();
                    let client = client.clone();
                    async move { client.patch_clash_overrides(overrides).await }
                },
            )
            .await
    }

    async fn patch_with_overrides(
        &self,
        owner: &ChimeraClient,
        patch: Mapping,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        let snapshot = self.get_snapshot().await?;
        let current = snapshot.state.clone();
        let next = apply_mapping_to_typed_clash(&current, &patch)?;
        let plan = plan_clash_patch(&patch, &current)?;
        validate_mixed_port_change(&plan, &current)?;
        validate_external_controller_change(owner, &plan, &current).await?;

        let forward = self.prepare_replace(next).await?;
        let rollback = self.prepare_replace(current).await?;
        let committed = match self
            .replace_prepared_if_version(snapshot.version, forward)
            .await?
        {
            ConditionalReplaceResult::Replaced(snapshot) => snapshot,
            ConditionalReplaceResult::Conflict { actual_version } => {
                bail!(
                    "clash config version conflict: expected {}, actual {}",
                    snapshot.version,
                    actual_version
                );
            }
        };

        let finalize = async {
            apply_clash_runtime_change(owner, &plan).await?;
            run_clash_patch_side_effects(&plan);
            Config::runtime().draft().patch_config(&overrides);
            Config::runtime().apply();
            Config::clash().data().save_config()?;
            Config::verge().data().save_file()?;
            Ok::<_, anyhow::Error>(())
        }
        .await;

        if let Err(primary) = finalize {
            Config::runtime().discard();
            let rollback_result = self
                .replace_prepared_if_version(committed.version, rollback)
                .await;
            let mut failures = Vec::new();
            match rollback_result {
                Ok(ConditionalReplaceResult::Replaced(_)) => {}
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failures.push(format!(
                        "clash rollback conflict: expected {}, actual {}",
                        committed.version, actual_version
                    ))
                }
                Err(error) => failures.push(format!("clash rollback failed: {error:#}")),
            }
            if let Err(error) = Config::clash().data().save_config() {
                failures.push(format!(
                    "legacy clash rollback persistence failed: {error:#}"
                ));
            }
            if let Err(error) = Config::verge().data().save_file() {
                failures.push(format!(
                    "legacy verge rollback persistence failed: {error:#}"
                ));
            }
            if failures.is_empty() {
                return Err(primary);
            }
            bail!(
                "{primary:#}; compensation failures: {}",
                failures.join("; ")
            );
        }

        if plan.mode_changed {
            log_err!(
                crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change(
                    plan.break_on_mode_change,
                )
                .await,
                "failed to interrupt connections after mode change"
            );
        }
        Ok(())
    }
}

impl Drop for ClashConfigStateBackend {
    fn drop(&mut self) {
        match self {
            ClashConfigStateBackend::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => {}
        }
    }
}

impl ChimeraClient {
    pub(crate) fn get_clash_config(&self) -> Result<ClashConfig> {
        self.inner.clash_config.get()
    }

    pub(crate) fn clash_info(&self) -> ClashInfo {
        self.inner.clash_config.get_info()
    }

    pub(crate) async fn patch_clash(&self, patch: Mapping) -> Result<()> {
        self.inner.clash_config.patch(self, patch).await
    }

    pub(crate) async fn patch_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        self.inner
            .clash_config
            .patch_overrides(self, overrides)
            .await
    }

    pub(crate) async fn patch_running_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        self.inner
            .clash_config
            .patch_running_overrides(self, overrides)
            .await
    }
}

fn apply_mapping_to_typed_clash(current: &ClashConfig, patch: &Mapping) -> Result<ClashConfig> {
    let mut next = current.clone();

    let mut override_mapping = Mapping::new();
    for key in [
        "allow-lan",
        "ipv6",
        "log-level",
        "mode",
        "secret",
        "unified-delay",
        "tcp-concurrent",
    ] {
        let key_value = Value::String(key.to_string());
        if let Some(value) = patch.get(&key_value) {
            override_mapping.insert(key_value, value.clone());
        }
    }
    let override_patch: ClashGuardOverridesPatch =
        serde_yaml::from_value(Value::Mapping(override_mapping))
            .context("invalid typed Clash override patch")?;
    next.overrides.apply(override_patch);

    if let Some(value) = get_non_null_patch_value(patch, "mixed-port") {
        let port = value
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("mixed-port must be an integer"))?;
        next.mixed_port.start_port =
            u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port"))?;
    }

    if let Some(value) = get_non_null_patch_value(patch, "external-controller") {
        let raw = value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("external-controller must be a string"))?;
        let normalized = if raw.starts_with(':') {
            format!("127.0.0.1{raw}")
        } else {
            raw.to_string()
        };
        let controller: SocketAddr = normalized
            .parse()
            .context("external-controller must be an IP host:port")?;
        next.external_controller.host = controller.ip();
        next.external_controller.port.start_port = controller.port();
    }

    Ok(next)
}

fn get_non_null_patch_value<'a>(patch: &'a Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    patch.get(key).filter(|value| !value.is_null())
}

fn plan_clash_patch(patch: &Mapping, current: &ClashConfig) -> Result<ClashPatchPlan> {
    let mixed_port = get_non_null_patch_value(patch, "mixed-port").and_then(|value| value.as_u64());
    let mixed_port = mixed_port
        .map(|port| u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port")))
        .transpose()?;
    let mixed_port_changed = mixed_port
        .map(|port| port != current.mixed_port.start_port)
        .unwrap_or(false);

    let external_controller = get_non_null_patch_value(patch, "external-controller")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("external-controller must be a string"))
        })
        .transpose()?;
    let external_controller_changed = external_controller
        .as_ref()
        .map(|controller| controller != &Config::clash().data().get_client_info().server)
        .unwrap_or(false);

    Ok(ClashPatchPlan {
        mixed_port,
        mixed_port_changed,
        external_controller,
        external_controller_changed,
        mode_changed: get_non_null_patch_value(patch, "mode").is_some(),
        break_on_mode_change: current.break_connection.on_mode_change,
        requires_restart: get_non_null_patch_value(patch, "mixed-port").is_some()
            || get_non_null_patch_value(patch, "secret").is_some()
            || get_non_null_patch_value(patch, "external-controller").is_some(),
    })
}

fn validate_mixed_port_change(plan: &ClashPatchPlan, current: &ClashConfig) -> Result<()> {
    if plan.mixed_port_changed
        && current.mixed_port.kind != PortStrategyKind::Random
        && let Some(port) = plan.mixed_port
        && !port_scanner::local_port_available(port)
    {
        bail!("port already in use");
    }

    Ok(())
}

async fn validate_external_controller_change(
    client: &ChimeraClient,
    plan: &ClashPatchPlan,
    current: &ClashConfig,
) -> Result<()> {
    if !plan.external_controller_changed {
        return Ok(());
    }

    let external_controller = plan
        .external_controller
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing external-controller"))?;
    let (_, port) = external_controller
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("external-controller must be host:port"))?;
    let port = port.parse::<u16>()?;
    let strategy = PortStrategy {
        kind: current.external_controller.port.kind.clone(),
        start_port: port,
    };
    let core_state = client.core_status().await?;

    if matches!(&core_state.state, CoreState::Running) && strategy.pick_and_try_port().is_err() {
        bail!("can not select fixed: current port is not available.");
    }

    Ok(())
}

async fn apply_clash_runtime_change(client: &ChimeraClient, plan: &ClashPatchPlan) -> Result<()> {
    if !plan.requires_restart {
        return Ok(());
    }

    client.rebuild_running_config().await
}

async fn update_core_config(client: &ChimeraClient) -> Result<()> {
    match client.rebuild_running_config().await {
        Ok(_) => {
            handle::Handle::notice_message(&handle::Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&handle::Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

fn run_clash_patch_side_effects(plan: &ClashPatchPlan) {
    if plan.mixed_port.is_some() {
        log_err!(sysopt::Sysopt::global().init_sysproxy());
    }

    if plan.mode_changed {
        crate::feat::update_proxies_buff(None);
        log::debug!("systray mode changed, update proxies buff");
        log_err!(handle::Handle::update_systray_part());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{NoopPreparedLegacyMirror, PreparedLegacyMirror};
    use std::sync::{Arc, Mutex};

    struct RecordingBridge {
        mirrored_tun: Arc<Mutex<Option<bool>>>,
    }

    struct RecordingPreparedMirror {
        value: bool,
        target: Arc<Mutex<Option<bool>>>,
    }

    impl PreparedLegacyMirror for RecordingPreparedMirror {
        fn apply(self: Box<Self>) {
            *self.target.lock().unwrap() = Some(self.value);
        }
    }

    impl ClashLegacyBridge for RecordingBridge {
        fn prepare(&self, snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(RecordingPreparedMirror {
                value: snap.enable_tun_mode,
                target: self.mirrored_tun.clone(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    #[tokio::test]
    async fn existing_typed_file_wins_over_legacy_seed_on_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clash-config.yaml");
        let persisted = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        std::fs::write(&path, serde_yaml::to_string(&persisted).unwrap()).unwrap();

        let mirrored_tun = Arc::new(Mutex::new(None));
        let bridge: Arc<dyn ClashLegacyBridge> = Arc::new(RecordingBridge {
            mirrored_tun: mirrored_tun.clone(),
        });
        let path = Utf8PathBuf::from_path_buf(path).unwrap();
        let client = ClashConfigClient::new(
            path,
            ClashConfig::default(),
            bridge,
            Arc::new(LegacyRunningConfigBridge),
        )
        .await
        .unwrap();

        assert!(client.get_typed().enable_tun_mode);
        assert_eq!(*mirrored_tun.lock().unwrap(), Some(true));
    }

    #[test]
    fn raw_mapping_projects_into_typed_saved_state() {
        let current = ClashConfig::default();
        let mut patch = Mapping::new();
        patch.insert("ipv6".into(), true.into());
        patch.insert("mixed-port".into(), 17890.into());
        patch.insert("external-controller".into(), "127.0.0.1:19090".into());

        let next = apply_mapping_to_typed_clash(&current, &patch).unwrap();
        let overrides = serde_yaml::to_value(&next.overrides).unwrap();
        let ipv6 = overrides
            .as_mapping()
            .and_then(|mapping| mapping.get("ipv6"))
            .and_then(Value::as_bool);

        assert_eq!(ipv6, Some(true));
        assert_eq!(next.mixed_port.start_port, 17890);
        assert_eq!(next.external_controller.port.start_port, 19090);
    }

    #[test]
    fn noop_test_mirror_type_remains_constructible() {
        let mirror: Box<dyn PreparedLegacyMirror> = Box::new(NoopPreparedLegacyMirror);
        mirror.apply();
    }
}

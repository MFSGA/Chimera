//! Application configuration client boundary.
//!
//! Application state is actor-owned and persisted as typed `application.yaml`.
//! The legacy combined verge model remains a compatibility mirror while callers
//! are migrated to the ref-style typed facade.

use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result, bail};
use camino::Utf8PathBuf;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
#[cfg(test)]
use struct_patch::Patch;

use super::ChimeraClient;
use crate::{
    bridge::{split_legacy_verge_patch, verge::application_from_legacy},
    config::{chimera::IVerge, core::Config},
    core::{handle, sysopt},
    state::{
        application::{
            ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
        },
        mirror::VergeLegacyBridge,
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
    pub(crate) fn legacy() -> anyhow::Result<Self> {
        #[cfg(test)]
        {
            Ok(Self {
                inner: Arc::new(ApplicationClientInner::Static {
                    state: parking_lot::RwLock::new(ChimeraAppConfig::default()),
                }),
            })
        }

        #[cfg(not(test))]
        {
            let config_path = Utf8PathBuf::from_path_buf(
                crate::utils::dirs::app_config_dir()?.join("application.yaml"),
            )
            .map_err(|path| {
                anyhow::anyhow!("application config path is not UTF-8: {}", path.display())
            })?;
            let bridge: Arc<dyn VergeLegacyBridge> =
                Arc::new(crate::bridge::verge::LegacyVergeBridge);
            let seed = bridge.snapshot_legacy()?;

            tauri::async_runtime::block_on(Self::new(config_path, seed, bridge))
        }
    }

    async fn new(
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

    async fn get(&self) -> anyhow::Result<ApplicationSnapshot> {
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

    async fn patch_typed(
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

    async fn replace(&self, next: ChimeraAppConfig) -> anyhow::Result<ApplicationSnapshot> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { .. } => {
                self.call(
                    |reply| ApplicationActorMessage::Replace { state: next, reply },
                    None,
                )
                .await
            }
            #[cfg(test)]
            ApplicationClientInner::Static { state } => {
                *state.write() = next.clone();
                Ok(ApplicationSnapshot {
                    state: next,
                    version: 0,
                })
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

    #[allow(dead_code)]
    pub(crate) async fn patch_app_config(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<()> {
        self.inner.application.patch_typed(patch).await?;
        Config::verge().data().save_file()?;
        handle::Handle::refresh_verge();
        Ok(())
    }
}

struct VergePatchPlan {
    service_mode: Option<bool>,
    auto_launch_changed: bool,
    system_proxy_changed: bool,
    proxy_bypass_changed: bool,
    enable_proxy_guard: bool,
    log_level_changed: bool,
    log_max_files_changed: bool,
    refresh_systray: bool,
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
        refresh_systray: patch.enable_system_proxy.is_some()
            || clash_patch.is_some_and(|patch| patch.enable_tun_mode.is_some()),
    })
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
        handle::Handle::update_systray_part()?;
    }

    log::debug!("todo: handle other fields");
    Ok(())
}

async fn patch_legacy_uncoordinated(client: &ChimeraClient, patch: IVerge) -> Result<()> {
    let base = Config::verge().latest().clone();
    let legacy_clash = Config::clash().latest().clone();
    let split = split_legacy_verge_patch(&base, &patch, &legacy_clash)?;
    let mut desired_application = base.clone();
    desired_application.patch_config(split.application.clone());
    let desired_application = application_from_legacy(&desired_application)?;

    Config::verge()
        .draft()
        .patch_config(split.application.clone());

    let result = async {
        let plan = plan_verge_patch(&split.application, split.clash_config.as_ref())?;
        apply_verge_runtime_change(client, &plan).await?;

        if let Some(clash_patch) = split.clash_config.as_ref() {
            client
                .inner
                .clash_config
                .apply_legacy_patch_to_draft(client, clash_patch)
                .await?;
        }

        run_verge_patch_side_effects(&plan, &split.application)?;
        client
            .inner
            .application
            .replace(desired_application)
            .await?;
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            Config::verge().data().save_file()?;
            handle::Handle::refresh_verge();
            Ok(())
        }
        Err(err) => {
            Config::verge().discard();
            Err(err)
        }
    }
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

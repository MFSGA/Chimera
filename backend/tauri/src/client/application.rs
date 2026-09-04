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

use super::{
    ChimeraClient, ClientError, CompensationFailure, LegacyVergeDomain, PartialCommit,
    error::Result as ClientResult,
};
use crate::{
    bridge::typed_patches_from_legacy_patch,
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

    async fn prepare_replace(
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

    async fn replace_prepared_if_version(
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

    async fn patch_legacy(&self, owner: &ChimeraClient, patch: IVerge) -> ClientResult<()> {
        apply_legacy_verge_patch_saga(owner, patch).await
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

    pub(crate) async fn patch_verge(&self, patch: IVerge) -> ClientResult<()> {
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

enum PreparedConfigDomain {
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

enum CommittedConfigDomain {
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

async fn compensate_legacy_verge_saga(
    client: &ChimeraClient,
    mut committed: Vec<CommittedConfigDomain>,
    primary: ClientError,
    mut failed_compensations: Vec<CompensationFailure>,
) -> ClientResult<()> {
    let committed_domains = committed
        .iter()
        .map(|domain| match domain {
            CommittedConfigDomain::Application { .. } => LegacyVergeDomain::Application,
            CommittedConfigDomain::Session { .. } => LegacyVergeDomain::Session,
            CommittedConfigDomain::Clash { .. } => LegacyVergeDomain::Clash,
        })
        .collect::<Vec<_>>();
    let mut compensated_domains = Vec::new();

    while let Some(domain) = committed.pop() {
        match domain {
            CommittedConfigDomain::Application {
                committed_version,
                rollback,
            } => match client
                .inner
                .application
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {
                    compensated_domains.push(LegacyVergeDomain::Application);
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failed_compensations.push(CompensationFailure::Conflict {
                        domain: LegacyVergeDomain::Application,
                        expected_version: committed_version,
                        actual_version,
                    });
                }
                Err(error) => failed_compensations.push(CompensationFailure::Error {
                    domain: LegacyVergeDomain::Application,
                    message: format!("{error:#}"),
                }),
            },
            CommittedConfigDomain::Session {
                committed_version,
                rollback,
            } => match client
                .inner
                .session_state
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {
                    compensated_domains.push(LegacyVergeDomain::Session);
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failed_compensations.push(CompensationFailure::Conflict {
                        domain: LegacyVergeDomain::Session,
                        expected_version: committed_version,
                        actual_version,
                    });
                }
                Err(error) => failed_compensations.push(CompensationFailure::Error {
                    domain: LegacyVergeDomain::Session,
                    message: format!("{error:#}"),
                }),
            },
            CommittedConfigDomain::Clash {
                committed_version,
                rollback,
            } => match client
                .inner
                .clash_config
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {
                    compensated_domains.push(LegacyVergeDomain::Clash);
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failed_compensations.push(CompensationFailure::Conflict {
                        domain: LegacyVergeDomain::Clash,
                        expected_version: committed_version,
                        actual_version,
                    });
                }
                Err(error) => failed_compensations.push(CompensationFailure::Error {
                    domain: LegacyVergeDomain::Clash,
                    message: format!("{error:#}"),
                }),
            },
        }
    }

    let mut legacy_uncertainties = Vec::new();
    if let Err(error) = Config::verge().data().save_file() {
        legacy_uncertainties.push(format!(
            "legacy verge rollback persistence failed: {error:#}"
        ));
    }
    if let Err(error) = Config::clash().data().save_config() {
        legacy_uncertainties.push(format!(
            "legacy clash rollback persistence failed: {error:#}"
        ));
    }
    handle::Handle::refresh_verge();
    handle::Handle::refresh_clash();

    if failed_compensations.is_empty() && legacy_uncertainties.is_empty() {
        return Err(primary);
    }

    let mut partial = PartialCommit::new(
        &primary,
        committed_domains,
        compensated_domains,
        failed_compensations,
    );
    for message in legacy_uncertainties {
        partial = partial.with_legacy_state_uncertain(message);
    }
    log::error!("legacy verge saga requires reconciliation: {partial:?}");
    Err(partial.into())
}

// TODO(actor-migration): this legacy `IVerge` compatibility saga still lives beside
// `ApplicationClient` while Chimera's typed clients are composed through legacy setup.
// Move it onto the main `ChimeraClient` facade when the shared bridge set/composition
// root is aligned with REF; remove this compatibility entry once callers patch typed
// Application/Session/Clash domains directly.
async fn apply_legacy_verge_patch_saga(client: &ChimeraClient, patch: IVerge) -> ClientResult<()> {
    let base = Config::verge().latest().clone();
    let legacy_clash = Config::clash().latest().clone();
    let mut split = typed_patches_from_legacy_patch(base, &patch, &legacy_clash)?;
    let plan = plan_verge_patch(&patch, split.clash_config.as_ref())?;

    let application_pair = if let Some(application_patch) = split.application.take() {
        let snapshot = client.inner.application.get().await?;
        let mut next = snapshot.state.clone();
        next.apply(application_patch);
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
        prepared.push(PreparedConfigDomain::Application {
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
        prepared.push(PreparedConfigDomain::Session {
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
        prepared.push(PreparedConfigDomain::Clash {
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
            PreparedConfigDomain::Application {
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
                    committed.push(CommittedConfigDomain::Application {
                        committed_version: snapshot.version,
                        rollback,
                    });
                    continue;
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    ClientError::Custom(format!(
                        "application config version conflict: expected {expected_version}, actual {actual_version}"
                    ))
                }
                Err(error) => ClientError::Anyhow(
                    error.context("failed to commit application config in legacy verge saga"),
                ),
            },
            PreparedConfigDomain::Session {
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
                    committed.push(CommittedConfigDomain::Session {
                        committed_version: snapshot.version,
                        rollback,
                    });
                    continue;
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    ClientError::Custom(format!(
                        "session config version conflict: expected {expected_version}, actual {actual_version}"
                    ))
                }
                Err(error) => ClientError::Anyhow(
                    error.context("failed to commit session state in legacy verge saga"),
                ),
            },
            PreparedConfigDomain::Clash {
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
                    committed.push(CommittedConfigDomain::Clash {
                        committed_version: snapshot.version,
                        rollback,
                    });
                    continue;
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    ClientError::Custom(format!(
                        "clash config version conflict: expected {expected_version}, actual {actual_version}"
                    ))
                }
                Err(error) => ClientError::Anyhow(
                    error.context("failed to commit clash config in legacy verge saga"),
                ),
            },
        };

        return compensate_legacy_verge_saga(client, committed, commit_error, Vec::new()).await;
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
        run_verge_patch_side_effects(&plan, &patch)?;
        Config::verge().data().save_file()?;
        if split.clash_config.is_some() {
            Config::clash().data().save_config()?;
        }
        handle::Handle::refresh_verge();
        Ok::<_, anyhow::Error>(())
    }
    .await;

    if let Err(error) = finalize {
        let legacy_uncertainty = CompensationFailure::LegacyStateUncertain {
            message: format!("{error:#}"),
        };
        return compensate_legacy_verge_saga(
            client,
            committed,
            ClientError::Anyhow(error.context("failed to finalize legacy verge patch")),
            vec![legacy_uncertainty],
        )
        .await;
    }

    Ok(())
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

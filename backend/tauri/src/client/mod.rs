//! Instance-owned application facade for runtime, profile, and core operations.
//!
//! This follows REF's client ownership direction while preserving Chimera's
//! staged migration from legacy globals and Chimera-specific core support.

mod application;
mod clash_config;
mod config_saga;
mod core_bridge;
mod error;
mod event_sink;
pub(crate) mod ports;
mod profiles;
pub mod rebuild;
pub mod runtime;
mod session_state;
mod system_dns;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex as StdMutex},
};

pub use self::runtime::{Degradation, DegradationPhase, MutationOutcome};
use self::{
    application::ApplicationClient,
    clash_config::ClashConfigClient,
    core_bridge::CoreLifecyclePort,
    event_sink::UiEventSink,
    profiles::{ProfileFsPort, ProfilesReadPort, ProfilesWritePort},
    session_state::SessionStateClient,
    system_dns::SystemDnsCache,
};
pub(crate) use self::{
    core_bridge::{LegacyCoreBridge, RuntimeTransformDiagnostics},
    error::{
        ClientError, CompensationFailure, LegacyVergeDomain, PartialCommit, Result as ClientResult,
    },
    event_sink::LegacyUiEventSink,
    profiles::{LegacyProfileFsPort, LegacyProfilesReadPort, LegacyProfilesWritePort},
    system_dns::OsSystemDnsCache,
};
use anyhow::Context as _;

use crate::{
    config::profile::item_type::ProfileUid,
    state::mirror::{
        ClashLegacyBridge, VergeLegacyBridge as VergeLegacyBridgeTrait, WindowLegacyBridge,
    },
};

#[derive(Clone)]
pub(crate) struct LegacyBridgeSet {
    pub(crate) verge: Arc<dyn VergeLegacyBridgeTrait>,
    pub(crate) window: Arc<dyn WindowLegacyBridge>,
    pub(crate) clash: Arc<dyn ClashLegacyBridge>,
}

pub(crate) struct ClientSetupArgs {
    pub(crate) bridges: LegacyBridgeSet,
    pub(crate) core: Arc<dyn CoreLifecyclePort>,
    pub(crate) profiles: Arc<dyn ProfilesReadPort>,
    pub(crate) profile_files: Arc<dyn ProfileFsPort>,
    pub(crate) profile_writes: Arc<dyn ProfilesWritePort>,
    pub(crate) system_dns: Arc<dyn SystemDnsCache>,
    pub(crate) ui_sink: Arc<dyn UiEventSink>,
}

#[derive(Clone)]
pub(crate) struct ChimeraClient {
    inner: Arc<ChimeraClientInner>,
}

struct TypedConfigClients {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
}

async fn new_typed_config_clients(bridges: LegacyBridgeSet) -> anyhow::Result<TypedConfigClients> {
    let config_dir = crate::utils::dirs::app_config_dir()?;
    let application_path = camino::Utf8PathBuf::from_path_buf(config_dir.join("application.yaml"))
        .map_err(|path| {
            anyhow::anyhow!("application config path is not UTF-8: {}", path.display())
        })?;
    let session_path = camino::Utf8PathBuf::from_path_buf(config_dir.join("session-state.yaml"))
        .map_err(|path| anyhow::anyhow!("session state path is not UTF-8: {}", path.display()))?;
    let clash_path = camino::Utf8PathBuf::from_path_buf(config_dir.join("clash-config.yaml"))
        .map_err(|path| anyhow::anyhow!("clash config path is not UTF-8: {}", path.display()))?;

    let application = ApplicationClient::new(
        application_path,
        bridges.verge.snapshot_legacy()?,
        bridges.verge.clone(),
    )
    .await?;
    let session_state = SessionStateClient::new(
        session_path,
        bridges.window.snapshot_legacy()?,
        bridges.window.clone(),
    )
    .await?;
    let clash_config = ClashConfigClient::new(
        clash_path,
        bridges.clash.snapshot_legacy()?,
        bridges.clash.clone(),
        Arc::new(core_bridge::LegacyRunningConfigBridge),
    )
    .await?;

    let typed = TypedConfigClients {
        application,
        session_state,
        clash_config,
    };
    sync_legacy_mirrors(&typed, &bridges).await?;
    Ok(typed)
}

async fn sync_legacy_mirrors(
    typed: &TypedConfigClients,
    bridges: &LegacyBridgeSet,
) -> anyhow::Result<()> {
    let application = typed
        .application
        .get()
        .await
        .context("failed to read loaded application config")?
        .state;
    bridges
        .verge
        .prepare(&application)
        .context("failed to prepare loaded application config legacy mirror")?
        .apply();

    let session_state = typed
        .session_state
        .get()
        .await
        .context("failed to read loaded session state")?
        .state;
    bridges
        .window
        .prepare(&session_state)
        .context("failed to prepare loaded session state legacy mirror")?
        .apply();

    let clash_config = typed
        .clash_config
        .get_snapshot()
        .await
        .context("failed to read loaded clash config")?
        .state;
    bridges
        .clash
        .prepare(&clash_config)
        .context("failed to prepare loaded clash config legacy mirror")?
        .apply();

    Ok(())
}

struct ChimeraClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    core: Arc<dyn CoreLifecyclePort>,
    profiles: Arc<dyn ProfilesReadPort>,
    profile_files: Arc<dyn ProfileFsPort>,
    profile_writes: Arc<dyn ProfilesWritePort>,
    system_dns: Arc<dyn SystemDnsCache>,
    ui_sink: Arc<dyn UiEventSink>,
    profile_commit: tokio::sync::Mutex<()>,
    pending_refreshes: StdMutex<HashSet<ProfileUid>>,
}

impl ChimeraClient {
    pub(crate) fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            bridges,
            core,
            profiles,
            profile_files,
            profile_writes,
            system_dns,
            ui_sink,
        } = args;
        let typed = tauri::async_runtime::block_on(new_typed_config_clients(bridges))?;
        Ok(Self::with_parts_and_typed_config(
            typed,
            core,
            profiles,
            profile_files,
            profile_writes,
            system_dns,
            ui_sink,
        ))
    }

    #[cfg(test)]
    fn with_parts(
        core: Arc<dyn CoreLifecyclePort>,
        profiles: Arc<dyn ProfilesReadPort>,
        profile_files: Arc<dyn ProfileFsPort>,
        profile_writes: Arc<dyn ProfilesWritePort>,
        system_dns: Arc<dyn SystemDnsCache>,
        ui_sink: Arc<dyn UiEventSink>,
    ) -> Self {
        let typed = TypedConfigClients {
            application: ApplicationClient::legacy()
                .expect("test application client should initialize"),
            session_state: SessionStateClient::legacy()
                .expect("test session state client should initialize"),
            clash_config: ClashConfigClient::legacy()
                .expect("test clash config client should initialize"),
        };
        Self::with_parts_and_typed_config(
            typed,
            core,
            profiles,
            profile_files,
            profile_writes,
            system_dns,
            ui_sink,
        )
    }

    fn with_parts_and_typed_config(
        typed: TypedConfigClients,
        core: Arc<dyn CoreLifecyclePort>,
        profiles: Arc<dyn ProfilesReadPort>,
        profile_files: Arc<dyn ProfileFsPort>,
        profile_writes: Arc<dyn ProfilesWritePort>,
        system_dns: Arc<dyn SystemDnsCache>,
        ui_sink: Arc<dyn UiEventSink>,
    ) -> Self {
        let inner = ChimeraClientInner {
            application: typed.application,
            session_state: typed.session_state,
            clash_config: typed.clash_config,
            core,
            profiles,
            profile_files,
            profile_writes,
            system_dns,
            ui_sink,
            profile_commit: tokio::sync::Mutex::new(()),
            pending_refreshes: StdMutex::new(HashSet::new()),
        };
        Self {
            inner: Arc::new(inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chimera_ipc::api::status::CoreState;

    use super::core_bridge::{CoreLifecycleLease, CoreStatusSnapshot};
    use super::*;
    use crate::client::system_dns::{NoopSystemDnsCache, SystemDnsCache};
    use crate::{
        config::{
            chimera::ClashCore,
            profile::{
                builder::ProfileBuilder,
                item::{
                    Profile, ProfileMetaGetter,
                    local::LocalProfile,
                    remote::{
                        PreparedSubscriptionUpdate, RemoteProfile, RemoteProfileOptions,
                        RemoteProfileOptionsBuilder, SubscriptionInfo,
                    },
                    shared::ProfileShared,
                },
                item_type::ProfileUid,
                profiles::Profiles,
            },
        },
        core::RunType,
    };

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        fail_rebuild: bool,
    }

    struct RecordingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
        fail_rebuild: bool,
    }

    #[async_trait]
    impl CoreLifecycleLease for RecordingLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: chimera_config::clash::config::ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            if self.fail_rebuild {
                anyhow::bail!("injected rebuild failure");
            }
            Ok(())
        }

        async fn run_core_from(&mut self, _config_path: &std::path::Path) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("run-from");
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop");
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("change-core");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            self.events.lock().unwrap().push("begin");
            Ok(Box::new(RecordingLease {
                events: self.events.clone(),
                fail_rebuild: self.fail_rebuild,
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 7,
                run_type: RunType::Normal,
            })
        }

        async fn on_profile_change(&self, _break_when: bool) {
            self.events.lock().unwrap().push("profile-change");
        }
    }

    struct StaticProfilesRead {
        profiles: Profiles,
    }

    impl ProfilesReadPort for StaticProfilesRead {
        fn snapshot(&self) -> anyhow::Result<Profiles> {
            Ok(self.profiles.clone())
        }
    }

    struct NoopProfileFs;

    #[async_trait]
    impl ProfileFsPort for NoopProfileFs {
        async fn resolve_path(&self, file: &str) -> anyhow::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from(file))
        }
        async fn read(&self, _file: &str) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn write_atomic(&self, _file: &str, _content: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove(&self, _file: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct RecordingProfileFs {
        previous_file: String,
        reads: Arc<Mutex<Vec<String>>>,
        writes: Arc<Mutex<Vec<(String, String)>>>,
        fail_write: bool,
    }

    #[async_trait]
    impl ProfileFsPort for RecordingProfileFs {
        async fn resolve_path(&self, file: &str) -> anyhow::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from(file))
        }
        async fn read(&self, file: &str) -> anyhow::Result<String> {
            self.reads.lock().unwrap().push(file.to_string());
            Ok(self.previous_file.clone())
        }

        async fn write_atomic(&self, file: &str, content: &str) -> anyhow::Result<()> {
            self.writes
                .lock()
                .unwrap()
                .push((file.to_string(), content.to_string()));
            if self.fail_write {
                anyhow::bail!("injected profile file write failure");
            }
            Ok(())
        }

        async fn remove(&self, _file: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct NoopProfilesWrite {
        fail_refresh: bool,
        refresh_commits: Option<Arc<Mutex<usize>>>,
        patch_commits: Option<Arc<Mutex<usize>>>,
    }

    #[async_trait]
    impl ProfilesWritePort for NoopProfilesWrite {
        async fn add(&self, profile: Profile) -> anyhow::Result<(ProfileUid, bool)> {
            Ok((profile.uid().to_string(), false))
        }
        async fn delete(&self, uid: &ProfileUid) -> anyhow::Result<(String, bool)> {
            Ok((format!("{uid}.yaml"), false))
        }
        async fn patch_profile(
            &self,
            _uid: &ProfileUid,
            _profile: ProfileBuilder,
        ) -> anyhow::Result<()> {
            if let Some(commits) = &self.patch_commits {
                *commits.lock().unwrap() += 1;
            }
            Ok(())
        }
        async fn patch_metadata(
            &self,
            _uid: &ProfileUid,
            _name: Option<String>,
            _desc: Option<Option<String>>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn patch_remote_options(
            &self,
            _uid: &ProfileUid,
            _user_agent: Option<Option<String>>,
            _with_proxy: Option<bool>,
            _self_proxy: Option<bool>,
            _update_interval_minutes: Option<u64>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn reorder(
            &self,
            _active_id: &ProfileUid,
            _over_id: &ProfileUid,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn reorder_by_list(&self, _list: &[ProfileUid]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn set_current(&self, _uid: Option<&ProfileUid>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn set_valid_fields(&self, _fields: &[String]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn set_profile_transform_chain(
            &self,
            _uid: &ProfileUid,
            _transforms: &[ProfileUid],
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
        async fn set_global_transform_chain(
            &self,
            _transforms: &[ProfileUid],
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
        async fn apply_remote_options(
            &self,
            _uid: &ProfileUid,
            _options: RemoteProfileOptionsBuilder,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn commit_refreshed(
            &self,
            _uid: &ProfileUid,
            _updated: RemoteProfile,
        ) -> anyhow::Result<bool> {
            if let Some(commits) = &self.refresh_commits {
                *commits.lock().unwrap() += 1;
            }
            if self.fail_refresh {
                anyhow::bail!("injected profile state failure");
            }
            Ok(false)
        }
        async fn replace_remote_definition(
            &self,
            _uid: &ProfileUid,
            _file: &str,
            _updated_at: Option<usize>,
            _url: url::Url,
            _option: Option<RemoteProfileOptions>,
            _subscription: Option<SubscriptionInfo>,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
    }

    struct RecordingUi {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingSystemDns {
        flushes: Arc<Mutex<usize>>,
        fail: bool,
    }

    impl SystemDnsCache for RecordingSystemDns {
        fn flush(&self) -> anyhow::Result<()> {
            *self.flushes.lock().unwrap() += 1;
            if self.fail {
                anyhow::bail!("injected DNS cache flush failure");
            }
            Ok(())
        }
    }

    impl UiEventSink for RecordingUi {
        fn refresh_clash(&self) {
            self.events.lock().unwrap().push("refresh-ui");
        }
        fn refresh_runtime_transform_diagnostics(&self) {
            self.events.lock().unwrap().push("refresh-diagnostics");
        }
        fn refresh_profiles(&self) {
            self.events.lock().unwrap().push("refresh-profiles");
        }
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TypedMirrorCapture {
        application_auto_check: Option<bool>,
        session_window_width: Option<u32>,
        clash_tun: Option<bool>,
    }

    enum PreparedTypedMirror {
        Application {
            capture: Arc<Mutex<TypedMirrorCapture>>,
            value: bool,
        },
        Session {
            capture: Arc<Mutex<TypedMirrorCapture>>,
            value: Option<u32>,
        },
        Clash {
            capture: Arc<Mutex<TypedMirrorCapture>>,
            value: bool,
        },
    }

    impl crate::state::mirror::PreparedLegacyMirror for PreparedTypedMirror {
        fn apply(self: Box<Self>) {
            match *self {
                PreparedTypedMirror::Application { capture, value } => {
                    capture.lock().unwrap().application_auto_check = Some(value);
                }
                PreparedTypedMirror::Session { capture, value } => {
                    capture.lock().unwrap().session_window_width = value;
                }
                PreparedTypedMirror::Clash { capture, value } => {
                    capture.lock().unwrap().clash_tun = Some(value);
                }
            }
        }
    }

    struct RecordingVergeBridge {
        capture: Arc<Mutex<TypedMirrorCapture>>,
    }

    impl VergeLegacyBridgeTrait for RecordingVergeBridge {
        fn prepare(
            &self,
            snap: &chimera_config::application::ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
            Ok(Box::new(PreparedTypedMirror::Application {
                capture: self.capture.clone(),
                value: snap.enable_auto_check_update,
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<chimera_config::application::ChimeraAppConfig> {
            Ok(chimera_config::application::ChimeraAppConfig::default())
        }
    }

    struct RecordingWindowBridge {
        capture: Arc<Mutex<TypedMirrorCapture>>,
    }

    impl WindowLegacyBridge for RecordingWindowBridge {
        fn prepare(
            &self,
            snap: &chimera_config::state::PersistentState,
        ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
            let main = chimera_config::state::window::WindowLabel("main".into());
            Ok(Box::new(PreparedTypedMirror::Session {
                capture: self.capture.clone(),
                value: snap.window_state.get(&main).map(|state| state.width),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<chimera_config::state::PersistentState> {
            Ok(chimera_config::state::PersistentState::default())
        }
    }

    struct RecordingClashBridge {
        capture: Arc<Mutex<TypedMirrorCapture>>,
    }

    impl ClashLegacyBridge for RecordingClashBridge {
        fn prepare(
            &self,
            snap: &chimera_config::clash::config::ClashConfig,
        ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
            Ok(Box::new(PreparedTypedMirror::Clash {
                capture: self.capture.clone(),
                value: snap.enable_tun_mode,
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<chimera_config::clash::config::ClashConfig> {
            Ok(chimera_config::clash::config::ClashConfig::default())
        }
    }

    #[tokio::test]
    async fn typed_setup_mirrors_loaded_state_through_composition_root() {
        let dir = tempfile::tempdir().unwrap();
        let application_path = dir.path().join("application.yaml");
        let session_path = dir.path().join("session-state.yaml");
        let clash_path = dir.path().join("clash-config.yaml");

        let application = chimera_config::application::ChimeraAppConfig {
            enable_auto_check_update: false,
            ..chimera_config::application::ChimeraAppConfig::default()
        };
        std::fs::write(
            &application_path,
            serde_yaml::to_string(&application).unwrap(),
        )
        .unwrap();

        let session = chimera_config::state::PersistentState {
            window_state: std::collections::BTreeMap::from([(
                chimera_config::state::window::WindowLabel("main".into()),
                chimera_config::state::window::WindowState {
                    width: 1234,
                    height: 720,
                    x: 10,
                    y: 20,
                    maximized: false,
                    fullscreen: false,
                },
            )]),
        };
        std::fs::write(&session_path, serde_yaml::to_string(&session).unwrap()).unwrap();

        let clash = chimera_config::clash::config::ClashConfig {
            enable_tun_mode: true,
            ..chimera_config::clash::config::ClashConfig::default()
        };
        std::fs::write(&clash_path, serde_yaml::to_string(&clash).unwrap()).unwrap();

        let capture = Arc::new(Mutex::new(TypedMirrorCapture::default()));
        let bridges = LegacyBridgeSet {
            verge: Arc::new(RecordingVergeBridge {
                capture: capture.clone(),
            }),
            window: Arc::new(RecordingWindowBridge {
                capture: capture.clone(),
            }),
            clash: Arc::new(RecordingClashBridge {
                capture: capture.clone(),
            }),
        };

        let application = ApplicationClient::new(
            camino::Utf8PathBuf::from_path_buf(application_path).unwrap(),
            bridges.verge.snapshot_legacy().unwrap(),
            bridges.verge.clone(),
        )
        .await
        .unwrap();
        let session_state = SessionStateClient::new(
            camino::Utf8PathBuf::from_path_buf(session_path).unwrap(),
            bridges.window.snapshot_legacy().unwrap(),
            bridges.window.clone(),
        )
        .await
        .unwrap();
        let clash_config = ClashConfigClient::new(
            camino::Utf8PathBuf::from_path_buf(clash_path).unwrap(),
            bridges.clash.snapshot_legacy().unwrap(),
            bridges.clash.clone(),
            Arc::new(core_bridge::LegacyRunningConfigBridge),
        )
        .await
        .unwrap();
        let typed = TypedConfigClients {
            application,
            session_state,
            clash_config,
        };

        assert_eq!(*capture.lock().unwrap(), TypedMirrorCapture::default());
        sync_legacy_mirrors(&typed, &bridges).await.unwrap();
        assert_eq!(
            *capture.lock().unwrap(),
            TypedMirrorCapture {
                application_auto_check: Some(false),
                session_window_width: Some(1234),
                clash_tun: Some(true),
            }
        );
    }

    fn test_local_profile(uid: &str) -> Profile {
        Profile::Local(LocalProfile {
            shared: ProfileShared {
                uid: uid.into(),
                name: "Local Test".into(),
                file: format!("{uid}.yaml"),
                desc: None,
                updated: 7,
            },
            symlinks: None,
            chain: Vec::new(),
        })
    }

    fn test_remote_profile() -> RemoteProfile {
        RemoteProfile {
            url: url::Url::parse("https://example.com/profile.yaml").unwrap(),
            option: RemoteProfileOptions::default(),
            shared: ProfileShared {
                uid: "r-test".into(),
                name: "Test".into(),
                file: "r-test.yaml".into(),
                desc: None,
                updated: 7,
            },
            chain: Vec::new(),
            extra: SubscriptionInfo::default(),
        }
    }

    fn recording_client_with_profiles(
        profiles: Profiles,
        fail_rebuild: bool,
    ) -> (ChimeraClient, Arc<Mutex<Vec<&'static str>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild,
            }),
            Arc::new(StaticProfilesRead { profiles }),
            Arc::new(NoopProfileFs),
            Arc::new(NoopProfilesWrite::default()),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );
        (client, events)
    }

    fn recording_client(fail_rebuild: bool) -> (ChimeraClient, Arc<Mutex<Vec<&'static str>>>) {
        recording_client_with_profiles(Profiles::default(), fail_rebuild)
    }

    #[test]
    fn duplicate_profile_refresh_is_rejected_until_guard_drops() {
        let (client, _) = recording_client(false);
        let uid = "r-test".to_string();
        let first = client.begin_profile_refresh(&uid).unwrap();
        let error = match client.begin_profile_refresh(&uid) {
            Ok(_) => panic!("duplicate refresh should be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("already in progress"));
        drop(first);
        assert!(client.begin_profile_refresh(&uid).is_ok());
    }

    #[test]
    fn refresh_fingerprint_tracks_definition_but_not_display_metadata() {
        let profile = test_remote_profile();
        let fingerprint = ChimeraClient::remote_profile_fingerprint(&profile).unwrap();
        let mut renamed = profile.clone();
        renamed.shared.name = "Renamed".into();
        assert!(ChimeraClient::ensure_refresh_is_current(&fingerprint, &renamed).is_ok());
        let mut changed_url = profile.clone();
        changed_url.url = url::Url::parse("https://example.com/changed.yaml").unwrap();
        assert!(ChimeraClient::ensure_refresh_is_current(&fingerprint, &changed_url).is_err());
        let mut refreshed = profile;
        refreshed.shared.updated += 1;
        assert!(ChimeraClient::ensure_refresh_is_current(&fingerprint, &refreshed).is_err());
    }

    #[tokio::test]
    async fn refresh_file_write_failure_does_not_commit_profile_state() {
        let remote = test_remote_profile();
        let uid = remote.shared.uid.clone();
        let profiles = Profiles {
            items: vec![Profile::Remote(remote.clone())],
            ..Profiles::default()
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let reads = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let refresh_commits = Arc::new(Mutex::new(0));
        let previous_file = "mode: rule\n".to_string();
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead { profiles }),
            Arc::new(RecordingProfileFs {
                previous_file: previous_file.clone(),
                reads: reads.clone(),
                writes: writes.clone(),
                fail_write: true,
            }),
            Arc::new(NoopProfilesWrite {
                refresh_commits: Some(refresh_commits.clone()),
                ..NoopProfilesWrite::default()
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let (_, snapshot_file) = client.remote_profile_snapshot(&uid).await.unwrap();
        assert_eq!(snapshot_file, previous_file);

        let mut data = serde_yaml::Mapping::new();
        data.insert("mode".into(), "global".into());
        let prepared = PreparedSubscriptionUpdate::for_test(
            data,
            SubscriptionInfo {
                upload: 10,
                download: 20,
                total: 30,
                expire: 40,
            },
        );
        let fingerprint = ChimeraClient::remote_profile_fingerprint(&remote).unwrap();

        let error = client
            .commit_refreshed_profile(uid, fingerprint, previous_file, prepared)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("injected profile file write failure")
        );
        assert_eq!(*refresh_commits.lock().unwrap(), 0);
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refresh_state_failure_restores_file_through_profile_fs_port() {
        let remote = test_remote_profile();
        let uid = remote.shared.uid.clone();
        let profiles = Profiles {
            items: vec![Profile::Remote(remote.clone())],
            ..Profiles::default()
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let reads = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let previous_file = "mode: rule\n".to_string();
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead { profiles }),
            Arc::new(RecordingProfileFs {
                previous_file: previous_file.clone(),
                reads: reads.clone(),
                writes: writes.clone(),
                fail_write: false,
            }),
            Arc::new(NoopProfilesWrite {
                fail_refresh: true,
                ..NoopProfilesWrite::default()
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let (_, snapshot_file) = client.remote_profile_snapshot(&uid).await.unwrap();
        assert_eq!(snapshot_file, previous_file);
        assert_eq!(reads.lock().unwrap().as_slice(), ["r-test.yaml"]);

        let mut data = serde_yaml::Mapping::new();
        data.insert("mode".into(), "global".into());
        let prepared = PreparedSubscriptionUpdate::for_test(
            data,
            SubscriptionInfo {
                upload: 10,
                download: 20,
                total: 30,
                expire: 40,
            },
        );
        let fingerprint = ChimeraClient::remote_profile_fingerprint(&remote).unwrap();

        let error = client
            .commit_refreshed_profile(uid, fingerprint, previous_file.clone(), prepared)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("injected profile state failure"));
        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].0, "r-test.yaml");
        assert!(writes[0].1.contains("mode: global"));
        assert_eq!(writes[1], ("r-test.yaml".to_string(), previous_file));
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn profile_patch_uses_injected_write_port_before_runtime_rebuild() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let patch_commits = Arc::new(Mutex::new(0));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead {
                profiles: Profiles::default(),
            }),
            Arc::new(NoopProfileFs),
            Arc::new(NoopProfilesWrite {
                patch_commits: Some(patch_commits.clone()),
                ..NoopProfilesWrite::default()
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let outcome = client
            .patch_profile("l-test".into(), ProfileBuilder::Local(Default::default()))
            .await
            .unwrap();

        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(*patch_commits.lock().unwrap(), 1);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "refresh-profiles",
                "begin",
                "rebuild",
                "refresh-ui",
                "profile-change"
            ]
        );
    }

    #[tokio::test]
    async fn flush_system_dns_cache_forwards_to_injected_adapter() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let flushes = Arc::new(Mutex::new(0));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead {
                profiles: Profiles::default(),
            }),
            Arc::new(NoopProfileFs),
            Arc::new(NoopProfilesWrite::default()),
            Arc::new(RecordingSystemDns {
                flushes: flushes.clone(),
                fail: false,
            }),
            Arc::new(RecordingUi { events }),
        );

        client.flush_system_dns_cache().await.unwrap();
        assert_eq!(*flushes.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn flush_system_dns_cache_propagates_adapter_failure() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(StaticProfilesRead {
                profiles: Profiles::default(),
            }),
            Arc::new(NoopProfileFs),
            Arc::new(NoopProfilesWrite::default()),
            Arc::new(RecordingSystemDns {
                flushes: Arc::new(Mutex::new(0)),
                fail: true,
            }),
            Arc::new(RecordingUi { events }),
        );

        let error = client.flush_system_dns_cache().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected DNS cache flush failure")
        );
    }

    #[tokio::test]
    async fn core_status_is_read_through_the_injected_lifecycle_port() {
        let (client, _) = recording_client(false);
        let snapshot = client.core_status().await.unwrap();
        assert!(matches!(snapshot.state, CoreState::Stopped(None)));
        assert_eq!(snapshot.state_changed_at, 7);
        assert_eq!(snapshot.run_type, RunType::Normal);
    }

    #[tokio::test]
    async fn change_core_runs_through_the_injected_lifecycle_lease() {
        let (client, events) = recording_client(false);
        client.change_core(ClashCore::Mihomo).await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "change-core"]);
    }

    #[tokio::test]
    async fn stop_core_runs_through_the_injected_lifecycle_lease() {
        let (client, events) = recording_client(false);
        client.stop_core().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "stop"]);
    }

    #[tokio::test]
    async fn core_update_lease_keeps_stop_and_restart_on_one_lifecycle_lease() {
        let (client, events) = recording_client(false);
        let mut lease = client.begin_core_update().await.unwrap();
        lease.stop().await.unwrap();
        lease
            .run_core_from(std::path::Path::new("runtime.yaml"))
            .await
            .unwrap();
        drop(lease);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "stop", "run-from"]
        );
    }

    #[tokio::test]
    async fn runtime_rebuild_does_not_emit_profile_change_side_effects() {
        let (client, events) = recording_client(false);
        client.rebuild_running_config().await.unwrap();
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-ui"]
        );
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true);
        let error = client.rebuild_running_config().await.unwrap_err();
        assert!(error.to_string().contains("injected rebuild failure"));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-diagnostics"]
        );
    }

    #[tokio::test]
    async fn active_local_profile_file_save_rebuilds_runtime() {
        let profiles = Profiles {
            current: vec!["l-active".into()],
            items: vec![test_local_profile("l-active")],
            ..Profiles::default()
        };
        let (client, events) = recording_client_with_profiles(profiles, false);
        let outcome = client
            .save_profile_file("l-active".into(), "proxies: []\n".into())
            .await
            .unwrap();
        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-ui", "profile-change"]
        );
    }

    #[tokio::test]
    async fn inactive_local_profile_file_save_does_not_rebuild_runtime() {
        let profiles = Profiles {
            current: vec!["l-active".into()],
            items: vec![test_local_profile("l-active"), test_local_profile("l-idle")],
            ..Profiles::default()
        };
        let (client, events) = recording_client_with_profiles(profiles, false);
        let outcome = client
            .save_profile_file("l-idle".into(), "proxies: []\n".into())
            .await
            .unwrap();
        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_commit_rebuild_failure_is_structured_degradation() {
        let (client, events) = recording_client(true);
        let outcome = client.after_profile_runtime_commit("test mutation").await;
        assert!(matches!(outcome, MutationOutcome::CommittedDegraded { .. }));
        assert_eq!(outcome.degradations().len(), 1);
        let degradation = &outcome.degradations()[0];
        assert_eq!(degradation.phase, DegradationPhase::RuntimeBuild);
        assert_eq!(degradation.code, "runtime_rebuild_failed");
        assert!(degradation.retryable);
        assert!(degradation.message.contains("injected rebuild failure"));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-diagnostics"]
        );
    }

    #[test]
    fn mutation_outcome_serializes_ref_wire() {
        let applied = MutationOutcome::from_parts((), Vec::new());
        assert_eq!(
            serde_json::to_string(&applied).unwrap(),
            r#"{"status":"applied","value":null}"#
        );
        let degraded = MutationOutcome::from_parts(
            (),
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                code: "runtime_rebuild_failed".into(),
                message: "boom".into(),
                retryable: true,
            }],
        );
        let json = serde_json::to_string(&degraded).unwrap();
        assert!(json.contains(r#""status":"committed_degraded""#));
        assert!(json.contains(r#""phase":"runtime_build""#));
    }
}

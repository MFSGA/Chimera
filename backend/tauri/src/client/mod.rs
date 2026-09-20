//! Instance-owned application facade for runtime, profile, and core operations.
//!
//! This follows REF's client ownership direction while preserving Chimera's
//! staged migration from legacy globals and Chimera-specific core support.

mod application;
mod clash_api;
mod clash_config;
pub(crate) mod core_lifecycle;
mod event_sink;
pub(crate) mod ports;
mod profiles;
pub mod runtime;
pub(crate) mod runtime_inspection;
mod session_state;
mod system_dns;

use std::sync::Arc;

#[allow(unused_imports)]
pub(crate) use self::core_lifecycle::LegacyCoreBridge;
pub use self::runtime::{Degradation, DegradationPhase, MutationOutcome};
use self::{
    application::ApplicationClient,
    clash_config::ClashConfigClient,
    core_lifecycle::{CoreLifecycleClient, CoreLifecyclePort},
    event_sink::UiEventSink,
    profiles::{ProfileFsPort, ProfilesReadPort, ProfilesWritePort},
    session_state::SessionStateClient,
    system_dns::SystemDnsCache,
};
#[allow(unused_imports)]
pub(crate) use self::{
    core_lifecycle::RuntimeTransformDiagnostics,
    event_sink::{LegacyUiEventSink, NoopUiEventSink, TauriUiEventSink},
    ports::SessionPortResolver,
    profiles::{LegacyProfileFsPort, ProfilesClient},
    runtime_inspection::{RuntimeInspection, RuntimeInspectionContent},
    system_dns::OsSystemDnsCache,
};

use crate::{
    state::mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge},
    utils::path::PathResolver,
};

#[derive(Clone)]
pub(crate) struct LegacyBridgeSet {
    pub(crate) verge: Arc<dyn VergeLegacyBridge>,
    pub(crate) window: Arc<dyn WindowLegacyBridge>,
    pub(crate) clash: Arc<dyn ClashLegacyBridge>,
}

pub(crate) struct ClientSetupArgs {
    pub(crate) paths: PathResolver,
    pub(crate) bridges: LegacyBridgeSet,
    pub(crate) core: Arc<dyn CoreLifecyclePort>,
    pub(crate) service: Arc<dyn core_lifecycle::ServiceLifecyclePort>,
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

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<camino::Utf8PathBuf> {
    camino::Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

async fn new_typed_config_clients(
    paths: &PathResolver,
    bridges: &LegacyBridgeSet,
    core: Arc<dyn CoreLifecyclePort>,
) -> anyhow::Result<TypedConfigClients> {
    let application = ApplicationClient::new(
        utf8_path(paths.application_config_path())?,
        bridges.verge.snapshot_legacy()?,
        bridges.verge.clone(),
    )
    .await?;
    let session_state = SessionStateClient::new(
        utf8_path(paths.session_state_path())?,
        bridges.window.snapshot_legacy()?,
        bridges.window.clone(),
    )
    .await?;
    let clash_config = ClashConfigClient::new(
        utf8_path(paths.clash_config_path())?,
        bridges.clash.snapshot_legacy()?,
        bridges.clash.clone(),
        Arc::new(core_lifecycle::LegacyRunningConfigBridge::new(core)),
    )
    .await?;

    Ok(TypedConfigClients {
        application,
        session_state,
        clash_config,
    })
}

struct ChimeraClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    core_lifecycle: CoreLifecycleClient,
    core: Arc<dyn CoreLifecyclePort>,
    profiles: Arc<dyn ProfilesReadPort>,
    profile_files: Arc<dyn ProfileFsPort>,
    profile_writes: Arc<dyn ProfilesWritePort>,
    system_dns: Arc<dyn SystemDnsCache>,
    ui_sink: Arc<dyn UiEventSink>,
    profile_commit: tokio::sync::Mutex<()>,
}

impl ChimeraClient {
    pub(crate) fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            paths,
            bridges,
            core,
            service,
            profiles,
            profile_files,
            profile_writes,
            system_dns,
            ui_sink,
        } = args;
        let typed = tauri::async_runtime::block_on(new_typed_config_clients(
            &paths,
            &bridges,
            core.clone(),
        ))?;
        let runtime_paths =
            runtime::RuntimePaths::from_config_root(paths.app_config_dir().to_path_buf());
        let core_lifecycle =
            tauri::async_runtime::block_on(CoreLifecycleClient::spawn_with_service(
                core.clone(),
                typed.application.clone(),
                typed.clash_config.clone(),
                profiles.clone(),
                runtime_paths,
                service,
            ))?;
        Ok(Self::with_parts_and_typed_config(
            typed,
            core_lifecycle,
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
        let core_lifecycle = CoreLifecycleClient::direct(
            core.clone(),
            typed.application.clone(),
            typed.clash_config.clone(),
            profiles.clone(),
            runtime::RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        );
        Self::with_parts_and_typed_config(
            typed,
            core_lifecycle,
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
        core_lifecycle: CoreLifecycleClient,
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
            core_lifecycle,
            core,
            profiles,
            profile_files,
            profile_writes,
            system_dns,
            ui_sink,
            profile_commit: tokio::sync::Mutex::new(()),
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

    use super::core_lifecycle::CoreStatusSnapshot;
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
                    remote::{RemoteProfileOptions, RemoteProfileOptionsBuilder, SubscriptionInfo},
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

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn reconcile(
            &self,
            _clash: chimera_config::clash::config::ClashConfig,
            _profiles: Profiles,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            if self.fail_rebuild {
                anyhow::bail!("injected rebuild failure");
            }
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop");
            Ok(())
        }

        async fn change_core(
            &self,
            _profiles: Profiles,
            _clash_core: ClashCore,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("change-core");
            Ok(())
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 7,
                run_type: RunType::Normal,
                applied: None,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {
            self.events.lock().unwrap().push("profile-change");
        }
    }

    struct StaticProfilesRead {
        profiles: Profiles,
    }

    impl ProfilesReadPort for StaticProfilesRead {
        fn versioned_snapshot(&self) -> anyhow::Result<crate::client::profiles::ProfilesSnapshot> {
            Ok(crate::client::profiles::ProfilesSnapshot::new(
                1,
                self.profiles.clone(),
            ))
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
        async fn refresh_remote(
            &self,
            _uid: &ProfileUid,
            _options: Option<RemoteProfileOptionsBuilder>,
        ) -> anyhow::Result<bool> {
            if let Some(commits) = &self.refresh_commits {
                *commits.lock().unwrap() += 1;
            }
            if self.fail_refresh {
                anyhow::bail!("injected profile state failure");
            }
            Ok(false)
        }
        async fn import_remote(
            &self,
            _url: url::Url,
            _name: Option<String>,
            _option: Option<RemoteProfileOptionsBuilder>,
            _mode: crate::config::profile::item::remote::RemoteProfileImportMode,
        ) -> anyhow::Result<(ProfileUid, bool)> {
            Ok(("r-import".to_string(), false))
        }
        async fn replace_remote_definition(
            &self,
            _uid: &ProfileUid,
            _file: &str,
            _updated_at: Option<usize>,
            _url: url::Url,
            _option: Option<RemoteProfileOptions>,
            _subscription: Option<SubscriptionInfo>,
            _transforms: &[ProfileUid],
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

    #[tokio::test]
    async fn refresh_profile_propagates_actor_failure_without_runtime_side_effects() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let refresh_commits = Arc::new(Mutex::new(0));
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
                fail_refresh: true,
                refresh_commits: Some(refresh_commits.clone()),
                ..NoopProfilesWrite::default()
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let error = client
            .refresh_profile("r-test".into(), None)
            .await
            .expect_err("actor refresh failure should propagate");

        assert!(error.to_string().contains("injected profile state failure"));
        assert_eq!(*refresh_commits.lock().unwrap(), 1);
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn refresh_profile_success_refreshes_ui_without_unneeded_runtime_rebuild() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let refresh_commits = Arc::new(Mutex::new(0));
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
                refresh_commits: Some(refresh_commits.clone()),
                ..NoopProfilesWrite::default()
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );

        let outcome = client
            .refresh_profile("r-test".into(), None)
            .await
            .expect("actor refresh should succeed");

        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(*refresh_commits.lock().unwrap(), 1);
        assert_eq!(events.lock().unwrap().as_slice(), ["refresh-profiles"]);
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
    async fn change_core_runs_through_the_injected_lifecycle_port() {
        let (client, events) = recording_client(false);
        client.change_core(ClashCore::Mihomo).await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["change-core"]);
    }

    #[tokio::test]
    async fn stop_core_runs_through_the_injected_lifecycle_port() {
        let (client, events) = recording_client(false);
        client.stop_core().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["stop"]);
    }

    #[tokio::test]
    async fn runtime_rebuild_does_not_emit_profile_change_side_effects() {
        let (client, events) = recording_client(false);
        client.rebuild_running_config().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild", "refresh-ui"]);
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true);
        let error = client.rebuild_running_config().await.unwrap_err();
        assert!(error.to_string().contains("injected rebuild failure"));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["rebuild", "refresh-diagnostics"]
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
            ["rebuild", "refresh-ui", "profile-change"]
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
            ["rebuild", "refresh-diagnostics"]
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

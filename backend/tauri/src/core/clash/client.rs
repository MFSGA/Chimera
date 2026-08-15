//! Transitional instance-owned facade for the runtime/rebuild path.
//!
//! This mirrors REF's `NyanpasuClient` ownership direction without pretending the
//! rest of Chimera has already completed the actor/DI migration. Legacy globals
//! are contained behind ports here so IPC callers can migrate incrementally.

use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use chimera_ipc::api::status::CoreState;

use super::{
    core::{CoreLifecycleLease as CoreManagerLifecycleLease, CoreManager, RunType},
    rebuild::RebuildCoordinator,
    transaction::{RuntimePatchCoordinator, TransactionOutcome},
};
use crate::{
    config::{
        chimera::ClashCore,
        core::Config,
        profile::{
            item::ProfileKindGetter,
            item_type::{ProfileItemType, ProfileUid},
            profiles::Profiles,
        },
        runtime::ClashConfigOverrides,
    },
    core::{connection_interruption::ConnectionInterruptionService, handle::Handle},
};

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
}

#[async_trait]
pub(crate) trait CoreLifecycleLease: Send {
    async fn rebuild_running_config(&mut self) -> anyhow::Result<()>;
    #[allow(dead_code)]
    async fn stop(&mut self) -> anyhow::Result<()>;
    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn on_profile_change(&self);
}

pub(crate) trait UiEventSink: Send + Sync {
    fn refresh_clash(&self);
    fn refresh_profiles(&self);
}

struct LegacyCoreLifecyclePort;

struct LegacyCoreLifecycleLease {
    lease: CoreManagerLifecycleLease<'static>,
}

#[async_trait]
impl CoreLifecycleLease for LegacyCoreLifecycleLease {
    async fn rebuild_running_config(&mut self) -> anyhow::Result<()> {
        self.lease.rebuild_running_config().await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.lease.stop_core().await
    }

    async fn change_core(&mut self, clash_core: ClashCore) -> anyhow::Result<()> {
        self.lease.change_core(clash_core).await
    }
}

#[async_trait]
impl CoreLifecyclePort for LegacyCoreLifecyclePort {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core ownership has not migrated to the CoreActor yet.
        // Remove when: the composition root injects the actor-owned lifecycle port.
        Ok(Box::new(LegacyCoreLifecycleLease {
            lease: CoreManager::global().begin_lifecycle().await,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at, run_type) = CoreManager::global().status().await;
        Ok(CoreStatusSnapshot {
            state: state.into_owned(),
            state_changed_at,
            run_type,
        })
    }

    async fn on_profile_change(&self) {
        let _ = ConnectionInterruptionService::on_profile_change().await;
    }
}

struct LegacyUiEventSink;

impl UiEventSink for LegacyUiEventSink {
    fn refresh_clash(&self) {
        Handle::refresh_clash();
    }

    fn refresh_profiles(&self) {
        Handle::refresh_profiles();
    }
}

#[derive(Clone)]
pub(crate) struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

struct NyanpasuClientInner {
    core: Arc<dyn CoreLifecyclePort>,
    ui_sink: Arc<dyn UiEventSink>,
    runtime_patch: RuntimePatchCoordinator,
    rebuild: RebuildCoordinator,
}

impl NyanpasuClient {
    pub(crate) fn legacy() -> Self {
        Self::with_parts(
            Arc::new(LegacyCoreLifecyclePort),
            Arc::new(LegacyUiEventSink),
        )
    }

    fn with_parts(core: Arc<dyn CoreLifecyclePort>, ui_sink: Arc<dyn UiEventSink>) -> Self {
        let inner = NyanpasuClientInner {
            core,
            ui_sink,
            runtime_patch: RuntimePatchCoordinator::default(),
            rebuild: RebuildCoordinator::new(),
        };
        let client = Self {
            inner: Arc::new(inner),
        };
        client.start_rebuild_worker();
        client
    }

    fn start_rebuild_worker(&self) {
        let weak = Arc::downgrade(&self.inner);
        self.inner.rebuild.start_worker(move || {
            let weak = weak.clone();
            async move {
                let Some(inner) = weak.upgrade() else {
                    return Ok(());
                };
                NyanpasuClient { inner }.rebuild_running_config().await
            }
        });
    }

    pub(crate) fn request_rebuild(&self) {
        self.inner.rebuild.notifier().request_rebuild();
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.core.status().await
    }

    pub(crate) async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.change_core(clash_core).await
    }

    pub(crate) async fn read_profile_file(&self, uid: ProfileUid) -> anyhow::Result<String> {
        let profiles = Config::profiles();
        let profiles = profiles.latest();
        let item = profiles.get_item(&uid)?;
        let raw = item.read_file()?;
        let data = serde_yaml::from_str::<serde_yaml::Mapping>(&raw)?;
        serde_yaml::to_string(&data).context("failed to convert yaml to string")
    }

    fn persist_profiles(
        &self,
        update: impl FnOnce(&mut Profiles) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let profiles = Config::profiles();
        let result = {
            let mut draft = profiles.draft();
            update(&mut draft).and_then(|_| draft.save_file())
        };
        if let Err(error) = result {
            profiles.discard();
            return Err(error);
        }
        profiles.apply();
        self.inner.ui_sink.refresh_profiles();
        Ok(())
    }

    pub(crate) async fn patch_profile_metadata(
        &self,
        uid: ProfileUid,
        name: Option<String>,
        desc: Option<Option<String>>,
    ) -> anyhow::Result<()> {
        self.persist_profiles(|profiles| profiles.patch_metadata(&uid, name, desc))
    }

    pub(crate) async fn patch_remote_profile_options(
        &self,
        uid: ProfileUid,
        user_agent: Option<Option<String>>,
        with_proxy: Option<bool>,
        self_proxy: Option<bool>,
        update_interval_minutes: Option<u64>,
    ) -> anyhow::Result<()> {
        self.persist_profiles(|profiles| {
            profiles.patch_remote_options(
                &uid,
                user_agent,
                with_proxy,
                self_proxy,
                update_interval_minutes,
            )
        })
    }

    pub(crate) async fn save_profile_file(
        &self,
        uid: ProfileUid,
        file_data: String,
    ) -> anyhow::Result<()> {
        let profiles = Config::profiles();
        let profiles = profiles.latest();
        let item = profiles.get_item(&uid)?;
        anyhow::ensure!(
            !matches!(item.kind(), ProfileItemType::Remote),
            "remote profiles are updater-owned"
        );
        serde_yaml::from_str::<serde_yaml::Mapping>(&file_data)
            .context("failed to parse profile YAML")?;
        item.save_file(file_data)?;
        Ok(())
    }

    /// Serialize API-first runtime patches inside the client graph, matching REF's
    /// instance-owned patch gate direction while persistence still uses Chimera's
    /// legacy desired-state writer during this transition.
    pub(crate) async fn patch_running_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        let mapping = overrides.to_mapping();
        let persist_overrides = overrides.clone();

        self.inner
            .runtime_patch
            .apply(
                mapping,
                super::api::get_configs,
                |patch| async move { super::api::patch_configs(&patch).await },
                move |_patch| {
                    let overrides = persist_overrides.clone();
                    async move { crate::feat::patch_clash_overrides(overrides).await }
                },
            )
            .await
    }

    pub(crate) async fn rebuild_running_config(&self) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.rebuild_running_config().await?;
        self.inner.ui_sink.refresh_clash();
        self.inner.core.on_profile_change().await;
        Ok(())
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> anyhow::Result<()> {
        let mut lease = self.inner.core.begin().await?;
        lease.rebuild_running_config().await
    }

    pub(crate) async fn shutdown(&self) {
        self.inner.rebuild.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

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
        async fn rebuild_running_config(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            if self.fail_rebuild {
                anyhow::bail!("injected rebuild failure");
            }
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

        async fn on_profile_change(&self) {
            self.events.lock().unwrap().push("profile-change");
        }
    }

    struct RecordingUi {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl UiEventSink for RecordingUi {
        fn refresh_clash(&self) {
            self.events.lock().unwrap().push("refresh-ui");
        }

        fn refresh_profiles(&self) {
            self.events.lock().unwrap().push("refresh-profiles");
        }
    }

    fn recording_client(fail_rebuild: bool) -> (NyanpasuClient, Arc<Mutex<Vec<&'static str>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = NyanpasuClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild,
            }),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        );
        (client, events)
    }

    #[tokio::test]
    async fn core_status_is_read_through_the_injected_lifecycle_port() {
        let (client, _) = recording_client(false);

        let snapshot = client.core_status().await.unwrap();

        assert!(matches!(snapshot.state, CoreState::Stopped(None)));
        assert_eq!(snapshot.state_changed_at, 7);
        assert_eq!(snapshot.run_type, RunType::Normal);
        client.shutdown().await;
    }

    #[tokio::test]
    async fn change_core_runs_through_the_injected_lifecycle_lease() {
        let (client, events) = recording_client(false);

        client.change_core(ClashCore::Mihomo).await.unwrap();

        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "change-core"]);
        client.shutdown().await;
    }

    #[tokio::test]
    async fn rebuild_runs_runtime_then_ui_then_profile_side_effects() {
        let (client, events) = recording_client(false);

        client.rebuild_running_config().await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["begin", "rebuild", "refresh-ui", "profile-change"]
        );
        client.shutdown().await;
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true);

        let error = client.rebuild_running_config().await.unwrap_err();

        assert!(error.to_string().contains("injected rebuild failure"));
        assert_eq!(events.lock().unwrap().as_slice(), ["begin", "rebuild"]);
        client.shutdown().await;
    }
}

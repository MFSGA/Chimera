//! Transitional instance-owned facade for the runtime/rebuild path.
//!
//! This mirrors REF's `NyanpasuClient` ownership direction without pretending the
//! rest of Chimera has already completed the actor/DI migration. Legacy globals
//! are contained behind ports here so IPC callers can migrate incrementally.

use std::sync::Arc;

use async_trait::async_trait;
use chimera_ipc::api::status::CoreState;

use super::{
    core::{CoreManager, RunType},
    rebuild::RebuildCoordinator,
    transaction::{RuntimePatchCoordinator, TransactionOutcome},
};
use crate::{
    config::runtime::ClashConfigOverrides,
    core::{connection_interruption::ConnectionInterruptionService, handle::Handle},
};

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
}

#[async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync {
    async fn rebuild_running_config(&self) -> anyhow::Result<()>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn on_profile_change(&self);
}

pub(crate) trait UiEventSink: Send + Sync {
    fn refresh_clash(&self);
}

struct LegacyCoreLifecyclePort;

#[async_trait]
impl CoreLifecyclePort for LegacyCoreLifecyclePort {
    async fn rebuild_running_config(&self) -> anyhow::Result<()> {
        CoreManager::global()
            .restart_core_with_generated_config()
            .await
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
        self.inner.core.rebuild_running_config().await?;
        self.inner.ui_sink.refresh_clash();
        self.inner.core.on_profile_change().await;
        Ok(())
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

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn rebuild_running_config(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            if self.fail_rebuild {
                anyhow::bail!("injected rebuild failure");
            }
            Ok(())
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
    async fn rebuild_runs_runtime_then_ui_then_profile_side_effects() {
        let (client, events) = recording_client(false);

        client.rebuild_running_config().await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["rebuild", "refresh-ui", "profile-change"]
        );
        client.shutdown().await;
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true);

        let error = client.rebuild_running_config().await.unwrap_err();

        assert!(error.to_string().contains("injected rebuild failure"));
        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
        client.shutdown().await;
    }
}

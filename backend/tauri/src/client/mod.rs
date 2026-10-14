//! Instance-owned application facade for runtime, profile, and core operations.
//!
//! This follows REF's client ownership direction while preserving Chimera's
//! staged migration from legacy globals and Chimera-specific core support.

mod application;
pub(crate) mod application_workflow;
mod clash_api;
mod clash_config;
pub mod convergence;
pub(crate) mod core_lifecycle;
pub(crate) mod effects;
mod event_sink;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod hotkey;
pub(crate) mod ports;
pub(crate) mod profiles;
pub mod runtime;
pub(crate) mod runtime_inspection;
mod session_state;
mod system_dns;
pub(crate) mod traffic;

use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use chimera_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch, ProfileSource, Profiles,
    RemoteProfileOptions, RemoteProfileOptionsPatch,
};
use struct_patch::Patch as _;

#[allow(unused_imports)]
pub(crate) use self::core_lifecycle::LegacyCoreBridge;
pub use self::runtime::{Degradation, DegradationPhase, MutationOutcome};
use self::{
    application::ApplicationClient,
    clash_config::ClashConfigClient,
    core_lifecycle::{CoreLifecycleClient, CoreLifecyclePort},
    event_sink::UiEventSink,
    session_state::SessionStateClient,
    system_dns::SystemDnsCache,
};
#[allow(unused_imports)]
pub(crate) use self::{
    core_lifecycle::RuntimeTransformDiagnostics,
    event_sink::{LegacyUiEventSink, NoopUiEventSink, TauriUiEventSink},
    ports::SessionPortResolver,
    runtime_inspection::{RuntimeInspection, RuntimeInspectionContent},
    system_dns::OsSystemDnsCache,
};

use crate::{
    service::profile_file::ProfileFileService,
    state::mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge},
    state::mutation::MutationCoordinator,
    state::profiles::{CommitReport, NewProfileRequest, ProfilesError, ReorderOp},
    utils::path::PathResolver,
};

pub use runtime::RuntimePaths;

#[derive(Clone)]
pub(crate) struct LegacyBridgeSet {
    pub(crate) verge: Arc<dyn VergeLegacyBridge>,
    pub(crate) window: Arc<dyn WindowLegacyBridge>,
    pub(crate) clash: Arc<dyn ClashLegacyBridge>,
}

pub(crate) struct ClientSetupArgs {
    pub(crate) paths: PathResolver,
    pub runtime_paths: RuntimePaths,
    pub(crate) bridges: LegacyBridgeSet,
    pub(crate) core: Arc<dyn CoreLifecyclePort>,
    pub(crate) service: Arc<dyn core_lifecycle::ServiceLifecyclePort>,

    pub(crate) profile_service: Arc<ProfileFileService>,

    pub(crate) system_dns: Arc<dyn SystemDnsCache>,
    pub(crate) traffic_store: Option<Arc<dyn chimera_traffic::TrafficStore>>,
    pub(crate) ui_sink: Arc<dyn UiEventSink>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) effects: Arc<dyn effects::ports::ApplicationEffectsPort>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) window: Arc<dyn hotkey::ports::WindowControl>,
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

fn url_derived_name(url: &url::Url) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| {
            segment
                .trim_end_matches(".yaml")
                .trim_end_matches(".yml")
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .or_else(|| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "Remote Profile".into())
}

struct ChimeraClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    core_lifecycle: CoreLifecycleClient,
    core: Arc<dyn CoreLifecyclePort>,
    typed_profiles: Option<profiles::ProfilesClient>,
    profile_service: Arc<ProfileFileService>,
    system_dns: Arc<dyn SystemDnsCache>,
    traffic_store: parking_lot::Mutex<Option<Arc<dyn chimera_traffic::TrafficStore>>>,
    traffic: parking_lot::RwLock<Option<crate::core::traffic::TrafficClient>>,
    ui_sink: Arc<dyn UiEventSink>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    effects: effects::actor::EffectsClient,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    hotkey_mutation: tokio::sync::Mutex<()>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    window: Arc<dyn hotkey::ports::WindowControl>,
    #[cfg(test)]
    _test_profile_dir: Option<Arc<tempfile::TempDir>>,
}

#[cfg(test)]
struct NoSelfProxyPort;

#[cfg(test)]
impl crate::service::profile_file::SelfProxyPortSource for NoSelfProxyPort {
    fn mixed_port(&self) -> Option<u16> {
        None
    }
}

impl ChimeraClient {
    pub(crate) fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            paths,
            runtime_paths,
            bridges,
            core,
            service,
            profile_service,
            system_dns,
            traffic_store,
            ui_sink,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
                effects: effects_port,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            accelerators,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            window,
        } = args;
        let typed = tauri::async_runtime::block_on(new_typed_config_clients(
            &paths,
            &bridges,
            core.clone(),
        ))?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = tauri::async_runtime::block_on(async {
            let initial = effects::plan::ApplicationEffectInputs::project(
                &typed.application.get_typed(),
                &typed
                    .clash_config
                    .get()
                    .expect("test clash config should be readable"),
                None,
            );
            effects::actor::EffectsClient::spawn(effects::actor::EffectsArgs {
                port: effects_port,
                ui: ui_sink.clone(),
                initial,
            })
            .await
        })?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let mutations = MutationCoordinator::pending_with_notifications(Arc::new(effects.clone()));
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let mutations = MutationCoordinator::pending();
        let typed_profiles = tauri::async_runtime::block_on(profiles::ProfilesClient::new(
            mutations.clone(),
            utf8_path(paths.profiles_path())?,
            profile_service.clone(),
            profile_service.clone(),
            profile_service.clone(),
        ))?;
        let core_lifecycle =
            tauri::async_runtime::block_on(CoreLifecycleClient::spawn_with_profiles(
                core.clone(),
                typed.application.clone(),
                typed.clash_config.clone(),
                Some(typed_profiles.snapshot_handle()),
                runtime_paths,
                Arc::new(core_lifecycle::FsBinaryInstaller),
                service,
            ))?;
        tauri::async_runtime::block_on(mutations.connect(core_lifecycle.clone()))?;
        let client = Self::with_parts_and_typed_config(
            typed,
            core_lifecycle,
            core,
            profile_service,
            Some(typed_profiles),
            system_dns,
            ui_sink,
            #[cfg(test)]
            None,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            accelerators,
            window,
        );
        *client.inner.traffic_store.lock() = traffic_store;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        tauri::async_runtime::block_on(client.reconcile_hotkeys());
        Ok(client)
    }

    #[cfg(test)]
    async fn with_parts(
        core: Arc<dyn CoreLifecyclePort>,
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
        let test_profile_dir = Arc::new(tempfile::tempdir().expect("test profile directory"));
        let paths = PathResolver::with_base_dirs(
            test_profile_dir.path().join("config"),
            test_profile_dir.path().join("data"),
        );
        let profile_service = Arc::new(ProfileFileService::new(
            paths.clone(),
            Arc::new(NoSelfProxyPort),
        ));
        let runtime_paths = RuntimePaths::from_config_root(paths.app_config_dir().to_path_buf());
        let core_lifecycle = CoreLifecycleClient::direct(
            core.clone(),
            typed.application.clone(),
            typed.clash_config.clone(),
            runtime_paths,
        );

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = {
            let initial = effects::plan::ApplicationEffectInputs::project(
                &typed.application.get_typed(),
                &typed
                    .clash_config
                    .get()
                    .expect("test clash config should be readable"),
                None,
            );
            effects::actor::EffectsClient::spawn(effects::actor::EffectsArgs {
                port: Arc::new(effects::ports::NoopApplicationEffects),
                ui: ui_sink.clone(),
                initial,
            })
            .await
            .expect("test effects actor should start")
        };

        Self::with_parts_and_typed_config(
            typed,
            core_lifecycle,
            core,
            profile_service,
            None,
            system_dns,
            ui_sink,
            Some(test_profile_dir),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Arc::new(hotkey::ports::MockWindowControl::new()),
        )
    }

    fn with_parts_and_typed_config(
        typed: TypedConfigClients,
        core_lifecycle: CoreLifecycleClient,
        core: Arc<dyn CoreLifecyclePort>,
        profile_service: Arc<ProfileFileService>,
        typed_profiles: Option<profiles::ProfilesClient>,
        system_dns: Arc<dyn SystemDnsCache>,
        ui_sink: Arc<dyn UiEventSink>,
        #[cfg(test)] _test_profile_dir: Option<Arc<tempfile::TempDir>>,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        effects: effects::actor::EffectsClient,
        #[cfg(not(any(target_os = "android", target_os = "ios")))] accelerators: Arc<
            dyn hotkey::ports::AcceleratorValidator,
        >,
        #[cfg(not(any(target_os = "android", target_os = "ios")))] window: Arc<
            dyn hotkey::ports::WindowControl,
        >,
    ) -> Self {
        let inner = ChimeraClientInner {
            application: typed.application,
            session_state: typed.session_state,
            clash_config: typed.clash_config,
            core_lifecycle,
            core,
            typed_profiles,
            profile_service,
            system_dns,
            traffic_store: parking_lot::Mutex::new(None),
            traffic: parking_lot::RwLock::new(None),
            ui_sink,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            hotkey_mutation: tokio::sync::Mutex::new(()),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            accelerators,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            window,
            #[cfg(test)]
            _test_profile_dir,
        };
        Self {
            inner: Arc::new(inner),
        }
    }
}

impl ChimeraClient {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn unregister_hotkeys(&self) {
        for status in self.inner.effects.shutdown().await {
            if let crate::client::effects::status::EffectHealth::Degraded {
                code, message, ..
            } = status.health
            {
                tracing::warn!(%code, %message, "failed to release application effects at shutdown");
            }
        }
    }
}

impl ChimeraClient {
    fn profiles_client(&self) -> anyhow::Result<&profiles::ProfilesClient> {
        self.inner
            .typed_profiles
            .as_ref()
            .ok_or_else(|| anyhow!("typed ProfilesClient is not configured"))
    }

    fn refresh_profiles(&self) {
        crate::core::clash::proxies::ProxiesGuard::global()
            .write()
            .invalidate();
        self.inner.ui_sink.refresh_profiles();
    }

    async fn collect_post_commit_degradations(&self, report: &CommitReport) -> Vec<Degradation> {
        let mut degradations = report
            .degradations
            .iter()
            .map(|degradation| {
                tracing::warn!(
                    phase = ?degradation.phase,
                    code = ?degradation.code,
                    retryable = degradation.code.retryable(),
                    message = %degradation.message,
                    "profile commit completed with a degraded side effect",
                );
                application_workflow::profiles::map_profile_degradation(degradation)
            })
            .collect::<Vec<_>>();
        degradations.extend(report.runtime_degradations.clone());
        degradations
    }

    async fn after_commit(&self, report: &CommitReport) -> MutationOutcome<()> {
        MutationOutcome::from_parts((), self.collect_post_commit_degradations(report).await)
            .with_commit(report.receipt.clone())
    }

    fn auto_activation_failure_degradation(error: &impl std::fmt::Display) -> Degradation {
        tracing::warn!(
            %error,
            "profile auto-activation failed after commit; retaining committed profile id",
        );
        Degradation {
            phase: DegradationPhase::SystemEffect,
            code: "profile_auto_activation_failed".into(),
            message: error.to_string(),
            retryable: true,
        }
    }

    async fn try_auto_activate_if_none(&self, uid: ProfileId) -> MutationOutcome<()> {
        let profiles = match self.profiles_client() {
            Ok(profiles) => profiles.clone(),
            Err(error) => {
                return MutationOutcome::from_parts(
                    (),
                    vec![Self::auto_activation_failure_degradation(&error)],
                );
            }
        };
        match profiles.set_current_if_none(uid).await {
            Ok(None) => MutationOutcome::from_parts((), Vec::new()),
            Ok(Some(report)) => {
                self.refresh_profiles();
                self.after_commit(&report).await
            }
            Err(error) => MutationOutcome::from_parts(
                (),
                vec![Self::auto_activation_failure_degradation(&error)],
            ),
        }
    }

    pub(crate) fn get_profiles(&self) -> anyhow::Result<Arc<Profiles>> {
        Ok(self.profiles_client()?.snapshot())
    }

    pub(crate) async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        if matches!(
            request.definition.source(),
            Some(ProfileSource::Remote { .. })
        ) {
            return Err(anyhow!(
                "remote profiles must be created via import_profile"
            ));
        }
        let report = self.profiles_client()?.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| anyhow!("profile add committed without a created uid"))?;
        self.refresh_profiles();
        Ok(MutationOutcome::from_parts(
            created,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
    }

    pub(crate) async fn create_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        let is_config = matches!(request.definition, ProfileDefinition::Config { .. });
        let mut outcome = self.add_profile(request, initial_file).await?;
        if is_config {
            let uid = outcome.value().clone();
            outcome = outcome.append_commit_result(self.try_auto_activate_if_none(uid).await);
        }
        Ok(outcome)
    }

    pub(crate) async fn import_profile(
        &self,
        url: url::Url,
        name: Option<String>,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        let update_interval_explicit = patch
            .as_ref()
            .and_then(|patch| patch.update_interval_minutes)
            .is_some();
        let (name, custom_name) = match name {
            Some(name) if !name.trim().is_empty() => (name, true),
            _ => (url_derived_name(&url), false),
        };
        let mut options = RemoteProfileOptions::default();
        if let Some(patch) = patch {
            options.apply(patch);
        }
        let report = self
            .profiles_client()?
            .import(
                url,
                ProfileMetadata {
                    name,
                    desc: None,
                    custom_name,
                },
                options,
                update_interval_explicit,
            )
            .await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| anyhow!("profile import committed without a created uid"))?;
        self.refresh_profiles();
        let outcome = MutationOutcome::from_parts(
            created.clone(),
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone());
        Ok(outcome.append_commit_result(self.try_auto_activate_if_none(created).await))
    }

    pub(crate) async fn delete_profile(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.delete(uid).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn reorder_profile(
        &self,
        active: ProfileId,
        over: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .reorder(ReorderOp::Move { active, over })
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .reorder(ReorderOp::ByList(list))
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.refresh(uid, patch).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.patch_metadata(uid, patch).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .patch_remote_options(uid, patch)
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self
            .profiles_client()?
            .replace_definition(uid, definition)
            .await
        {
            Ok(report) => report,
            Err(error) => {
                self.inner.ui_sink.refresh_runtime_transform_diagnostics();
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_current(uid).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self.profiles_client()?.set_global_transforms(ids).await {
            Ok(report) => report,
            Err(error) => {
                self.inner.ui_sink.refresh_runtime_transform_diagnostics();
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_valid_fields(fields).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn save_profile_file(
        &self,
        uid: ProfileId,
        data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self.profiles_client()?.save_file(uid, data).await {
            Ok(report) => report,
            Err(error) => {
                self.inner.ui_sink.refresh_runtime_transform_diagnostics();
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn read_profile_file(&self, uid: ProfileId) -> anyhow::Result<String> {
        let snapshot = self.profiles_client()?.snapshot();
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        let raw = crate::state::profiles::ports::ProfileFsPort::read(
            self.inner.profile_service.as_ref(),
            &source.materialized().file,
        )?;
        profile_file_text_for_editor(raw, item.definition.is_config())
    }

    pub(crate) async fn get_profile_materialized_path(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<std::path::PathBuf> {
        let snapshot = self.profiles_client()?.snapshot();
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        self.inner
            .profile_service
            .resolve_path(&source.materialized().file)
            .map_err(Into::into)
    }
}

/// REF parity: config Profiles are YAML mappings, but transforms and scripts
/// must be returned byte-for-byte as text (even when they are not valid YAML).
fn profile_file_text_for_editor(raw: String, is_config: bool) -> anyhow::Result<String> {
    if is_config {
        crate::service::profile_file::normalize_yaml_document(&raw)
            .context("failed to normalize profile YAML")
    } else {
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    #[test]
    fn transform_profile_file_read_preserves_non_yaml_source_exactly() {
        let javascript = "function main(config) { return config; }\n// [invalid YAML";
        let lua = "return { [\"proxies\"] = {} }\n-- not a YAML mapping";
        for source in [javascript, lua] {
            assert_eq!(
                super::profile_file_text_for_editor(source.into(), false).unwrap(),
                source,
                "script and merge transform content must never be normalized as YAML"
            );
        }
    }

    #[test]
    fn config_profile_file_read_normalizes_yaml_and_rejects_invalid_mapping() {
        let normalized = super::profile_file_text_for_editor("mode: rule\n".into(), true).unwrap();
        assert_eq!(normalized, "mode: rule\n");
        let error =
            super::profile_file_text_for_editor("not: [valid YAML".into(), true).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to normalize profile YAML")
        );
    }

    use async_trait::async_trait;
    use chimera_ipc::api::status::CoreState;

    use super::core_lifecycle::CoreStatusSnapshot;
    use super::*;
    use crate::client::system_dns::{NoopSystemDnsCache, SystemDnsCache};
    use crate::{config::chimera::ClashCore, core::RunType};

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        fail_rebuild: bool,
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn reconcile(
            &self,
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

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop");
            Ok(())
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("change-core");
            Ok(())
        }
        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 7,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {
            self.events.lock().unwrap().push("profile-change");
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

    async fn recording_client(
        fail_rebuild: bool,
    ) -> (ChimeraClient, Arc<Mutex<Vec<&'static str>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild,
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        )
        .await;
        (client, events)
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
            Arc::new(RecordingSystemDns {
                flushes: flushes.clone(),
                fail: false,
            }),
            Arc::new(RecordingUi { events }),
        )
        .await;

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
            Arc::new(RecordingSystemDns {
                flushes: Arc::new(Mutex::new(0)),
                fail: true,
            }),
            Arc::new(RecordingUi { events }),
        )
        .await;

        let error = client.flush_system_dns_cache().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected DNS cache flush failure")
        );
    }

    #[tokio::test]
    async fn core_status_is_read_through_the_injected_lifecycle_port() {
        let (client, _) = recording_client(false).await;
        let snapshot = client.core_status().await.unwrap();
        assert!(matches!(snapshot.state, CoreState::Stopped(None)));
        assert_eq!(snapshot.state_changed_at, 7);
        assert_eq!(snapshot.run_type, RunType::Normal);
    }

    #[tokio::test]
    async fn change_core_runs_through_the_injected_lifecycle_port() {
        let (client, events) = recording_client(false).await;
        client.change_core(ClashCore::Mihomo).await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["change-core"]);
    }

    #[tokio::test]
    async fn stop_core_runs_through_the_injected_lifecycle_port() {
        let (client, events) = recording_client(false).await;
        client.stop_core().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["stop"]);
    }

    #[tokio::test]
    async fn runtime_rebuild_does_not_emit_profile_change_side_effects() {
        let (client, events) = recording_client(false).await;
        client.rebuild_running_config().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild", "refresh-ui"]);
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true).await;
        let error = client.rebuild_running_config().await.unwrap_err();
        assert!(error.to_string().contains("injected rebuild failure"));
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
            r#"{"status":"committed","value":null,"commits":[],"notifications_pending":true}"#
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

//! Compatibility transaction on the existing committed Application and Clash owners.
//! The canonical candidate gate and source CAS remain authoritative.
use super::committed::SourceVersions;
use super::{inputs::RuntimeInputs, source_capture::SourceCapture};
use crate::client::{
    application::ApplicationClient,
    application_operations::checked_application_patch_eligible,
    clash_config::ClashConfigClient,
    core_lifecycle::{command::ApplicationPatchResult, state::CoreLifecycleState},
};
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_profile_io::content_source::FsProfileContentSource;
use std::sync::Arc;
use struct_patch::Patch;

pub struct SourceTransaction<'a> {
    pub state: &'a CoreLifecycleState,
    pub application: &'a ApplicationClient,
    pub clash: &'a ClashConfigClient,
}

pub enum ApplicationPatchCommit {
    Settled(ApplicationPatchResult),
    /// Used only by the existing isolated compatibility tests.
    LegacyFixture {
        committed_version: u64,
        expected_sources: SourceVersions,
        candidate: ChimeraAppConfig,
    },
}

impl SourceTransaction<'_> {
    pub async fn commit_application_patch(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
        preflight: impl AsyncFnOnce(ChimeraAppConfig, bool, bool) -> anyhow::Result<Option<String>>,
        allow_legacy_fixture: bool,
    ) -> anyhow::Result<ApplicationPatchCommit> {
        let canonical = self.state.canonical_convergence_session();
        if !crate::state::mutation::impact::application_patch_requires_reconcile(
            &patch,
            service_connected,
        ) && !(canonical && patch.enable_service_mode.is_some())
        {
            self.application.patch_typed(patch).await?;
            return Ok(ApplicationPatchCommit::Settled(ApplicationPatchResult {
                degradations: Vec::new(),
                notified_version: None,
            }));
        }

        anyhow::ensure!(
            canonical || allow_legacy_fixture,
            "Application runtime commit requires the canonical Reconciler"
        );

        let previous = self.application.get().await?;
        // Unlike the old single-domain fence, ref reconciles the complete
        // committed runtime target. Track Clash and Profiles as well so an
        // overlapping domain commit cannot be claimed as a successful Apply.
        let expected_sources =
            allow_legacy_fixture.then(|| self.committed_runtime_source_versions());
        let use_checked = self.state.checked_gate.get().is_some()
            && checked_application_patch_eligible(&patch, &previous.state, service_connected);
        let mut candidate = previous.state.clone();
        candidate.apply(patch);
        let prepared = self.application.prepare_replace(candidate.clone()).await?;

        // The ref check ACKs *exact* prepared bytes with the one Core owner.
        // For an eligible local-host update we will notify CheckedSourceCommitted
        // after durable CAS; host changes request a fresh ownership plan.
        let checked_request = preflight(candidate.clone(), service_connected, use_checked).await?;

        let committed = self
            .application
            .replace_prepared_if_version(previous.version, prepared)
            .await;
        let committed_version = match committed {
            Ok(crate::state::ConditionalReplaceResult::Replaced(snapshot)) => snapshot.version,
            Ok(crate::state::ConditionalReplaceResult::Conflict { actual_version }) => {
                if let Some(request) = checked_request.as_ref() {
                    self.state
                        .checked_gate
                        .get()
                        .expect("checked Application gate admitted this proposal")
                        .abort_with_receipt(
                            chimera_contracts::mutation::ConfigDomain::Application,
                            request.clone(),
                        );
                }
                anyhow::bail!(
                    "application config version conflict: expected {}, actual {actual_version}",
                    previous.version
                );
            }
            Err(error) => {
                if let Some(request) = checked_request.as_ref() {
                    self.state
                        .checked_gate
                        .get()
                        .expect("checked Application gate admitted this proposal")
                        .abort_with_receipt(
                            chimera_contracts::mutation::ConfigDomain::Application,
                            request.clone(),
                        );
                }
                return Err(error.context("failed to commit checked application config"));
            }
        };

        if use_checked {
            // Like the upstream source owner, publish the exact durable CAS
            // version immediately. The one CoreLifecycle Actor receives the
            // checked notice while this command is active, and dispatches
            // ApplyChecked only after this source command finishes. No UI
            // callback is needed to make a successful source commit observable.
            let gate = self
                .state
                .checked_gate
                .get()
                .expect("checked gate admitted this commit");
            gate.committed_with_receipt(
                chimera_contracts::mutation::ConfigDomain::Application,
                crate::state::Version::new(committed_version),
                checked_request.expect("admitted checked Application has a receipt"),
            );
            // Do not run the legacy inline reconcile on this same version.
            return Ok(ApplicationPatchCommit::Settled(ApplicationPatchResult {
                degradations: Vec::new(),
                notified_version: Some(committed_version),
            }));
        }

        if canonical {
            // Host changes cannot authorize an ApplyChecked on the old
            // selected owner. Publish the durable version to ref S2/S3/S4;
            // that actor will re-observe the actual owner and latest source.
            self.state
                .checked_gate
                .get()
                .expect("canonical session has CandidateCheck")
                .committed_for_reconcile(
                    chimera_contracts::mutation::ConfigDomain::Application,
                    crate::state::Version::new(committed_version),
                );
            return Ok(ApplicationPatchCommit::Settled(ApplicationPatchResult {
                degradations: Vec::new(),
                notified_version: Some(committed_version),
            }));
        }

        anyhow::ensure!(
            allow_legacy_fixture,
            "Application runtime commit requires the canonical Reconciler"
        );
        Ok(ApplicationPatchCommit::LegacyFixture {
            committed_version,
            expected_sources: expected_sources.expect("legacy fixture source fence was captured"),
            candidate,
        })
    }

    pub fn startup_source_identity(
        &self,
        profiles: Option<&Arc<chimera_config::profile::Profiles>>,
    ) -> anyhow::Result<String> {
        let app = self.application.get_typed();
        let clash = self.clash.get()?;
        let app_target = crate::state::mutation::impact::application_target(&app)
            .ok_or_else(|| anyhow::anyhow!("application runtime identity cannot be serialized"))?;
        let clash_target = crate::state::mutation::impact::clash_target(&clash)
            .ok_or_else(|| anyhow::anyhow!("clash runtime identity cannot be serialized"))?;
        let profile_target = match profiles {
            Some(profiles) => crate::state::mutation::impact::profiles_target(profiles)
                .ok_or_else(|| anyhow::anyhow!("profiles runtime identity cannot be serialized"))?,
            None => "no-profile-runtime".into(),
        };
        let projection = serde_json::to_vec(&(
            app_target,
            clash_target,
            profile_target,
            "profile-content-unavailable",
        ))?;
        Ok(crate::state::profiles::payload_digest(&projection))
    }
    pub async fn unresolved_startup_target_identity(&self) -> Option<String> {
        let profiles = self
            .state
            .profiles
            .as_ref()
            .map(|profiles| Arc::new(profiles.load().state.clone()));
        let identity = self.startup_source_identity(profiles.as_ref()).ok()?;
        Some(format!("profile-content-unavailable:{identity}"))
    }
    pub fn committed_runtime_source_versions(
        &self,
    ) -> crate::client::reconciler::committed::SourceVersions {
        crate::client::reconciler::committed::SourceVersions::from_committed(
            self.application,
            self.clash,
            self.state.profiles.as_ref().map(|p| *p.load().version),
        )
    }
    pub async fn check_candidate(
        &self,
        candidate: crate::state::mutation::RuntimeCandidate,
        use_checked: bool,
    ) -> Option<anyhow::Result<Option<String>>> {
        if let Some(gate) = self.state.checked_gate.get() {
            if use_checked {
                // The ACK proves that one Core owner retained the exact
                // validated candidate before the source is made durable.
                return Some(
                    gate.check_with_receipt(candidate)
                        .await
                        .map(Some)
                        .map_err(anyhow::Error::new),
                );
            }
            // Core selection and Service mode require a fresh ownership
            // plan; avoid an orphaned direct Checked inbox entry.
            return Some(
                gate.validate_only(candidate)
                    .await
                    .map(|()| None)
                    .map_err(anyhow::Error::new),
            );
        }
        None
    }

    pub async fn capture_reconcile_target(
        &self,
        profiles_dir: impl FnOnce() -> anyhow::Result<std::path::PathBuf> + Send,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<(
        String,
        ChimeraAppConfig,
        chimera_config::clash::config::ClashConfig,
        Arc<chimera_config::profile::Profiles>,
        std::collections::BTreeMap<String, String>,
    )> {
        let inputs = self
            .capture_profile_inputs_for(profiles_dir, app, clash, profiles, staged_content)
            .await?;
        let target = inputs.checked_target_key()?;
        let (app, clash, profiles, staged_content) = inputs.into_parts().map_err(|error| {
            anyhow::Error::new(
                super::workflow_mutation::RuntimeCheckFailure::candidate_unavailable(
                    error.to_string(),
                ),
            )
        })?;
        Ok((target, app, clash, profiles, staged_content))
    }

    pub async fn capture_profile_inputs_for(
        &self,
        profiles_dir: impl FnOnce() -> anyhow::Result<std::path::PathBuf> + Send,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<RuntimeInputs> {
        let source = FsProfileContentSource::new(profiles_dir()?);
        SourceCapture::new(
            self.state.content_reader.as_ref(),
            self.state.profiles.as_ref(),
            &source,
        )
        .capture_inputs(app, clash, profiles, staged_content)
        .await
    }

    pub async fn capture_committed_profile_inputs_for(
        &self,
        profiles_dir: impl FnOnce() -> anyhow::Result<std::path::PathBuf> + Send,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
    ) -> anyhow::Result<RuntimeInputs> {
        if self.state.content_reader.is_none() && self.state.profiles.is_none() {
            anyhow::bail!("Profile state is unavailable for committed runtime capture");
        }
        let source = FsProfileContentSource::new(profiles_dir()?);
        SourceCapture::new(
            self.state.content_reader.as_ref(),
            self.state.profiles.as_ref(),
            &source,
        )
        .capture_committed_inputs(app, clash)
        .await
    }

    pub async fn startup_target_identity(
        &self,
        profiles_dir: impl FnOnce() -> anyhow::Result<std::path::PathBuf> + Send,
    ) -> anyhow::Result<String> {
        if self.state.profiles.is_some() {
            return self
                .capture_committed_profile_inputs_for(
                    profiles_dir,
                    self.application.get_typed(),
                    self.clash.get()?,
                )
                .await?
                .checked_target_key();
        }
        self.startup_source_identity(None)
    }

    pub async fn reconcile_profiles(
        &self,
        profiles_dir: impl FnOnce() -> anyhow::Result<std::path::PathBuf> + Send,
        service_connected: impl FnOnce() -> bool + Send,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let (target, app, clash, profiles, staged_content) = self
            .capture_reconcile_target(profiles_dir, app, clash, profiles, staged_content)
            .await?;
        let target_core = app.core;
        let run_type = crate::client::core_lifecycle::status::RunType::classify(
            app.enable_service_mode,
            service_connected(),
        );
        self.state
            .core
            .reconcile_profiles(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                staged_content,
                Some(target),
            )
            .await
    }
}

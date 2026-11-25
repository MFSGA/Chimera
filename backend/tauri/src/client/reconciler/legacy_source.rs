//! Compatibility source transaction and frozen input capture.
//! The canonical source actors still own CAS; this bridge uses their clients.
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::reconciler::{inputs::RuntimeInputs, legacy_preparation::RuntimePreparation};
#[cfg(test)]
use crate::config::chimera::ClashCore;
use crate::enhance::FsProfileContentSource;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_core::actor_v2::endpoint::ExecutionHost;
use std::sync::Arc;
use struct_patch::Patch;

pub use chimera_core::client::core_lifecycle::command::ApplicationPatchResult;

/// Use ref checked-source handoff only for an isolated local-host runtime
/// enhancement patch. Core/Service ownership and other application settings
/// may carry separate effects; admitting a combined patch would skip their
/// legacy postcommit workflow after publishing CheckedSourceCommitted.
pub(in crate::client) fn checked_application_patch_eligible(
    patch: &ChimeraAppConfigPatch,
    previous: &ChimeraAppConfig,
    service_connected: bool,
) -> bool {
    use struct_patch::Status as _;

    if patch.enable_builtin_enhanced.is_none() || previous.enable_service_mode || service_connected
    {
        return false;
    }
    // Compare every field, including nested patch DTOs, against the empty
    // patch so a future app setting never silently becomes Checked-eligible.
    let mut remainder = patch.clone();
    remainder.enable_builtin_enhanced = None;
    remainder.is_empty()
}

#[cfg(test)]
mod checked_application_route_tests {
    use super::*;

    #[test]
    fn only_local_enhancement_requests_may_use_checked_apply() {
        let mut previous = ChimeraAppConfig::default();
        previous.enable_service_mode = false;
        let mut patch = ChimeraAppConfigPatch {
            enable_builtin_enhanced: Some(!previous.enable_builtin_enhanced),
            ..Default::default()
        };
        assert!(checked_application_patch_eligible(&patch, &previous, false));
        assert!(
            !checked_application_patch_eligible(&patch, &previous, true),
            "a connected Service host retains its dedicated handoff"
        );
        previous.enable_service_mode = true;
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        previous.enable_service_mode = false;
        patch.core = Some(previous.core);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.core = None;
        patch.enable_service_mode = Some(false);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_service_mode = None;
        patch.enable_builtin_enhanced = None;
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));

        // A combined enhancement/OS-effect patch must stay with the existing
        // owner that handles proxy, launch and network action side effects.
        patch.enable_builtin_enhanced = Some(false);
        patch.enable_system_proxy = Some(true);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_system_proxy = None;
        patch.enable_auto_launch = Some(true);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_auto_launch = None;
        patch.hotkeys = Some(vec!["toggle_system_proxy,Ctrl+Shift+P".into()]);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.hotkeys = None;
        assert!(checked_application_patch_eligible(&patch, &previous, false));
    }
}

impl CoreLifecycleWorkflow {
    pub(in crate::client) async fn capture_reconcile_target(
        &self,
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
            .capture_profile_inputs_for(app, clash, profiles, staged_content)
            .await?;
        let target = inputs.checked_target_key()?;
        let (app, clash, profiles, staged_content) = inputs.into_parts().map_err(|error| {
            anyhow::Error::new(
                crate::client::application_workflow::mutation::RuntimeCheckFailure::
                    candidate_unavailable(error.to_string()),
            )
        })?;
        Ok((target, app, clash, profiles, staged_content))
    }

    pub(in crate::client) async fn capture_profile_inputs_for(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<RuntimeInputs> {
        let source = FsProfileContentSource::new(crate::utils::dirs::app_profiles_dir()?);
        RuntimePreparation::new(
            self.content_reader.as_ref(),
            self.profiles.as_ref(),
            &source,
        )
        .capture_inputs(app, clash, profiles, staged_content)
        .await
    }

    /// Ref: sample committed Profiles and capture all managed file inputs
    /// under one fence. The old workflow otherwise could pair a promoted file
    /// with a snapshot from before its successful source commit.
    pub(in crate::client) async fn capture_committed_profile_inputs_for(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
    ) -> anyhow::Result<RuntimeInputs> {
        if self.content_reader.is_none() && self.profiles.is_none() {
            anyhow::bail!("Profile state is unavailable for committed runtime capture");
        }
        let source = FsProfileContentSource::new(crate::utils::dirs::app_profiles_dir()?);
        RuntimePreparation::new(
            self.content_reader.as_ref(),
            self.profiles.as_ref(),
            &source,
        )
        .capture_committed_inputs(app, clash)
        .await
    }

    pub(in crate::client) async fn capture_reconcile_committed_target(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
    ) -> anyhow::Result<(
        String,
        ChimeraAppConfig,
        chimera_config::clash::config::ClashConfig,
        Arc<chimera_config::profile::Profiles>,
        std::collections::BTreeMap<String, String>,
    )> {
        let inputs = self
            .capture_committed_profile_inputs_for(app, clash)
            .await?;
        let target = inputs.checked_target_key()?;
        let (app, clash, profiles, content) = inputs.into_parts()?;
        Ok((target, app, clash, profiles, content))
    }

    pub(in crate::client) async fn startup_target_identity(&self) -> anyhow::Result<String> {
        if self.profiles.is_some() {
            return self
                .capture_committed_profile_inputs_for(
                    self.application.get_typed(),
                    self.clash.get()?,
                )
                .await?
                .checked_target_key();
        }
        self.startup_source_identity(None)
    }

    pub(in crate::client) async fn unresolved_startup_target_identity(&self) -> Option<String> {
        let profiles = self
            .profiles
            .as_ref()
            .map(|profiles| Arc::new(profiles.load().state.clone()));
        let identity = self.startup_source_identity(profiles.as_ref()).ok()?;
        Some(format!("profile-content-unavailable:{identity}"))
    }

    pub(in crate::client) fn startup_source_identity(
        &self,
        profiles: Option<&Arc<chimera_config::profile::Profiles>>,
    ) -> anyhow::Result<String> {
        let app = self.application.get_typed();
        let clash = self.clash.get()?;
        let app_target = chimera_core::state::mutation::impact::application_target(&app)
            .ok_or_else(|| anyhow::anyhow!("application runtime identity cannot be serialized"))?;
        let clash_target = chimera_core::state::mutation::impact::clash_target(&clash)
            .ok_or_else(|| anyhow::anyhow!("clash runtime identity cannot be serialized"))?;
        let profile_target = match profiles {
            Some(profiles) => chimera_core::state::mutation::impact::profiles_target(profiles)
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

    /// The ref source transition: a checked target is persisted before any
    /// running Core mutation. Only CoreLifecycle owns the later apply; no
    /// independent actor or second runtime writer is spawned by this path.
    #[cfg(test)]
    pub(in crate::client) async fn patch_application_config(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<Vec<crate::client::runtime::Degradation>> {
        Ok(self
            .patch_application_config_with_result(patch, service_connected)
            .await?
            .degradations)
    }

    /// Snapshot every committed source version before the legacy validation
    /// starts, without sending an RPC to a source writer blocked on this
    /// CoreLifecycle actor.
    pub(in crate::client) fn committed_runtime_source_versions(
        &self,
    ) -> crate::client::reconciler::committed::SourceVersions {
        crate::client::reconciler::committed::SourceVersions::from_committed(
            &self.application,
            &self.clash,
            self.profiles.as_ref().map(|p| *p.load().version),
        )
    }

    pub(in crate::client) async fn patch_application_config_with_result(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<ApplicationPatchResult> {
        let canonical = self.canonical_convergence_session();
        if !chimera_core::state::mutation::impact::application_patch_requires_reconcile(
            &patch,
            service_connected,
        ) && !(canonical && patch.enable_service_mode.is_some())
        {
            self.application.patch_typed(patch).await?;
            return Ok(ApplicationPatchResult {
                degradations: Vec::new(),
                notified_version: None,
            });
        }

        #[cfg(not(test))]
        anyhow::ensure!(
            canonical,
            "Application runtime commit requires the canonical Reconciler"
        );

        let previous = self.application.get().await?;
        // Unlike the old single-domain fence, ref reconciles the complete
        // committed runtime target. Track Clash and Profiles as well so an
        // overlapping domain commit cannot be claimed as a successful Apply.
        #[cfg(test)]
        let expected_sources = self.committed_runtime_source_versions();
        let use_checked = self.checked_gate.get().is_some()
            && checked_application_patch_eligible(&patch, &previous.state, service_connected);
        let mut candidate = previous.state.clone();
        candidate.apply(patch);
        let prepared = self.application.prepare_replace(candidate.clone()).await?;

        // The ref check ACKs *exact* prepared bytes with the one Core owner.
        // For an eligible local-host update we will notify CheckedSourceCommitted
        // after durable CAS; host changes request a fresh ownership plan.
        let checked_request = self
            .check_application_candidate(candidate.clone(), service_connected, use_checked)
            .await?;

        let committed = self
            .application
            .replace_prepared_if_version(previous.version, prepared)
            .await;
        let committed_version = match committed {
            Ok(crate::state::ConditionalReplaceResult::Replaced(snapshot)) => snapshot.version,
            Ok(crate::state::ConditionalReplaceResult::Conflict { actual_version }) => {
                if let Some(request) = checked_request.as_ref() {
                    self.checked_gate
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
                    self.checked_gate
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
                .checked_gate
                .get()
                .expect("checked gate admitted this commit");
            gate.committed_with_receipt(
                chimera_contracts::mutation::ConfigDomain::Application,
                chimera_core::state::Version::new(committed_version),
                checked_request.expect("admitted checked Application has a receipt"),
            );
            // Do not run the legacy inline reconcile on this same version.
            return Ok(ApplicationPatchResult {
                degradations: Vec::new(),
                notified_version: Some(committed_version),
            });
        }

        if canonical {
            // Host changes cannot authorize an ApplyChecked on the old
            // selected owner. Publish the durable version to ref S2/S3/S4;
            // that actor will re-observe the actual owner and latest source.
            self.checked_gate
                .get()
                .expect("canonical session has CandidateCheck")
                .committed_for_reconcile(
                    chimera_contracts::mutation::ConfigDomain::Application,
                    chimera_core::state::Version::new(committed_version),
                );
            return Ok(ApplicationPatchResult {
                degradations: Vec::new(),
                notified_version: Some(committed_version),
            });
        }

        #[cfg(not(test))]
        return Err(anyhow::anyhow!(
            "Application runtime commit requires the canonical Reconciler"
        ));
        #[cfg(test)]
        {
            // Source publication is complete. Ref's committed-target reconciler
            // always consumes the latest version; until the full actor is ported,
            // fence this compatibility apply before and after CoreLifecycle work.
            // Failure to read the source after commit is deferred, not rolled back.
            let degradation = crate::client::reconciler::committed::apply_committed(
                "application",
                committed_version,
                || async { self.application.get().await.map(|latest| latest.version) },
                || {
                    crate::client::reconciler::committed::apply_with_dependency_fence(
                        crate::client::reconciler::committed::SourceDomain::Application,
                        expected_sources,
                        || async { self.committed_runtime_source_versions() },
                        || self.reconcile_application(candidate, service_connected),
                    )
                },
            )
            .await;
            if let Some(degradation) = degradation {
                tracing::warn!(
                    code = %degradation.code,
                    message = %degradation.message,
                    "application source committed; runtime reconcile deferred"
                );
                Ok(ApplicationPatchResult {
                    degradations: vec![degradation],
                    notified_version: None,
                })
            } else {
                Ok(ApplicationPatchResult {
                    degradations: Vec::new(),
                    notified_version: None,
                })
            }
        }
    }

    /// Independent ref-style advisory check. The document and managed bytes
    /// are frozen under the ProfileContentReader fence. An unsupported or
    /// unavailable checker is not proof that the candidate is valid.
    pub(in crate::client) async fn check_application_candidate(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
        use_checked: bool,
    ) -> anyhow::Result<Option<String>> {
        use crate::state::mutation::{DomainChange, RuntimeCandidate};

        if self.profiles.is_none() {
            // Only isolated test adapters may omit the authoritative Profiles
            // domain. Production must never silently waive a Core check.
            #[cfg(test)]
            return Ok(None);
            #[cfg(not(test))]
            anyhow::bail!("Application preflight requires the typed Profiles source");
        }
        let candidate = RuntimeCandidate {
            change: DomainChange::Application {
                candidate: Arc::new(app.clone()),
            },
            staged: Default::default(),
        };
        if let Some(gate) = self.checked_gate.get() {
            if use_checked {
                // The ACK proves that one Core owner retained the exact
                // validated candidate before the source is made durable.
                return gate
                    .check_with_receipt(candidate)
                    .await
                    .map(Some)
                    .map_err(anyhow::Error::new);
            }
            // Core selection and Service mode require a fresh ownership
            // plan; avoid an orphaned direct Checked inbox entry.
            return gate
                .validate_only(candidate)
                .await
                .map(|()| None)
                .map_err(anyhow::Error::new);
        }
        #[cfg(not(test))]
        {
            let _ = service_connected;
            anyhow::bail!("Application preflight requires the installed production CandidateCheck");
        }
        // Isolated legacy workflow tests have no canonical runtime session.
        #[cfg(test)]
        crate::client::reconciler::legacy_check::check_domain_candidate(
            self.core.as_ref(),
            self.application.get_typed(),
            self.clash.get()?,
            self.profiles
                .as_ref()
                .expect("checked Profiles availability"),
            self.content_reader.as_ref(),
            candidate,
            service_connected,
        )
        .await
        .map(|()| None)
        .map_err(anyhow::Error::new)
    }

    #[cfg(test)]
    pub(in crate::client) async fn reconcile_application(
        &self,
        app: ChimeraAppConfig,
        service_connected: bool,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let Some(profiles) = &self.profiles else {
            let target_core = app.core;
            let run_type =
                crate::core::RunType::classify(app.enable_service_mode, service_connected);
            return self.core.reconcile(clash, target_core, run_type).await;
        };
        let (target, app, clash, profiles, staged_content) =
            self.capture_reconcile_committed_target(app, clash).await?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(app.enable_service_mode, service_connected);
        self.core
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

    pub(in crate::client) async fn reconcile_application_after_service_release(
        &self,
        app: ChimeraAppConfig,
        expected_target: Option<&str>,
    ) -> anyhow::Result<()> {
        // The earlier rejected Service candidate is not the Local fallback's
        // terminal; a second admitted Core command must supersede its verdict.
        self.prepared_outcome.lock().take();
        use crate::client::reconciler::attempt::{
            CommittedTargetAdvanced, OwnershipTransitionPending,
        };
        if let Some(expected) = expected_target
            && self.startup_target_identity().await? != expected
        {
            return Err(CommittedTargetAdvanced.into());
        }
        let clash = self.clash.get()?;
        #[cfg(test)]
        if self.profiles.is_none() {
            if let Some(target) = expected_target {
                self.progress
                    .lock()
                    .stage(target.to_owned(), ExecutionHost::Local);
            }
            return self
                .core
                .reconcile_after_service_release(clash, app.core)
                .await;
        }
        #[cfg(not(test))]
        anyhow::ensure!(
            self.canonical_convergence_session(),
            "Service release requires the canonical Reconciler"
        );
        let (target, app, clash, profiles, staged_content) =
            self.capture_reconcile_committed_target(app, clash).await?;

        if expected_target.is_some_and(|expected| expected != target) {
            return Err(CommittedTargetAdvanced.into());
        }

        if self.checked_gate.get().is_some()
            && let Some(authority) = self.source_authority.as_ref()
            && self.core.runtime_preparation_session().is_some()
        {
            anyhow::ensure!(
                !self.core.outcome_uncertain(),
                "Service-to-Local handback cannot begin while the Core outcome is uncertain"
            );
            // A newer committed source may have reversed the preferred host
            // while the Service release was being proved. Do not change the
            // host at all for a target that has already been superseded.
            anyhow::ensure!(
                authority
                    .verify_committed_prepared_target(&target, None)
                    .await?
                    .is_some(),
                "authoritative source cannot prove the released Service fallback target"
            );
            // The caller holds the transition lease and has *already* proved
            // that the rejected Service Core was released. Preserve this
            // evidence when selecting Local; do not submit via the router's
            // old combined host-handoff + inline-build path.
            self.core
                .select_local_after_confirmed_service_release()
                .await?;
            self.progress
                .lock()
                .stage(target.clone(), ExecutionHost::Local);
            // Once S2 has moved the owner, missing Core/source proof must be
            // Waiting, not a license for a second legacy submit or handoff.
            return self
                .try_reconcile_prepared_local(&target, &app, &clash, crate::core::RunType::Normal)
                .await
                .unwrap_or_else(|| {
                    Err(crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                        "Local Core ownership was not proven after the Service release handback",
                    )
                    .into())
                });
        }

        if self.canonical_convergence_session() {
            return Err(OwnershipTransitionPending::waiting(
                "Service release lost the canonical preparation or source authority",
            )
            .into());
        }

        #[cfg(not(test))]
        return Err(OwnershipTransitionPending::waiting(
            "Service release requires prepared runtime admission",
        )
        .into());

        #[cfg(test)]
        self.core
            .reconcile_profiles_after_service_release(
                clash,
                app.core,
                profiles,
                app,
                staged_content,
                Some(target),
            )
            .await
    }

    pub(in crate::client) async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        let clash = self.clash.get()?;
        let app = self.application.get_typed();
        let (target, app, clash, profiles, staged_content) = self
            .capture_reconcile_target(app, clash, profiles, staged_content)
            .await?;
        let target_core = app.core;
        let run_type = crate::core::RunType::classify(
            app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        );
        self.core
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

    pub(in crate::client) async fn retry_deferred_profiles(
        &self,
        expected_digest: &str,
    ) -> anyhow::Result<()> {
        chimera_core::client::reconciler::convergence_executor::retry_deferred_profiles(
            self,
            expected_digest,
        )
        .await
    }

    pub(in crate::client) async fn legacy_retry_deferred_profiles(
        &self,
        inputs: RuntimeInputs,
        expected_digest: &str,
    ) -> anyhow::Result<()> {
        #[cfg(not(test))]
        {
            let _ = (inputs, expected_digest);
            Err(
                crate::client::reconciler::attempt::OwnershipTransitionPending::waiting(
                    "deferred Profile retry requires the installed CandidateCheck",
                )
                .into(),
            )
        }
        #[cfg(test)]
        {
            let (app, clash, profiles, profile_content) =
                inputs.clone().into_parts().map_err(|error| {
                    anyhow::Error::new(
                    crate::client::application_workflow::mutation::RuntimeCheckFailure::
                        candidate_unavailable(error.to_string()),
                )
                })?;
            let target_core = app.core;
            let run_type = crate::core::RunType::classify(
                app.enable_service_mode,
                crate::core::service::ipc::get_ipc_state().is_connected(),
            );
            self.core
                .validate_profile_runtime(
                    clash.clone(),
                    target_core,
                    run_type,
                    profiles.clone(),
                    app.clone(),
                    profile_content.clone(),
                )
                .await?;
            self.core
                .reconcile_profiles(
                    clash,
                    target_core,
                    run_type,
                    profiles,
                    app,
                    profile_content,
                    Some(expected_digest.to_owned()),
                )
                .await
        }
    }

    #[cfg(test)]
    pub(in crate::client) async fn select_core(&self, core: ClashCore) -> anyhow::Result<()> {
        let Some(profiles) = &self.profiles else {
            return self.core.change_core(core).await;
        };

        let previous_app = self.application.get_typed();
        let mut candidate_app = previous_app.clone();
        candidate_app.core = core;
        let profile_snapshot = Arc::new(profiles.load().state.clone());
        let clash = self.clash.get()?;
        let (candidate_target, candidate_app, candidate_clash, candidate_profiles, staged_content) =
            self.capture_reconcile_target(
                candidate_app,
                clash.clone(),
                profile_snapshot.clone(),
                Default::default(),
            )
            .await?;
        let run_type = crate::core::RunType::classify(
            candidate_app.enable_service_mode,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        );

        self.core
            .reconcile_profiles(
                candidate_clash,
                core,
                run_type,
                candidate_profiles,
                candidate_app.clone(),
                staged_content,
                Some(candidate_target),
            )
            .await?;

        if let Err(error) = async {
            self.application
                .patch_typed(chimera_config::application::ChimeraAppConfigPatch {
                    core: Some(candidate_app.core),
                    ..Default::default()
                })
                .await?;
            crate::config::core::Config::verge().data().save_file()?;
            crate::core::handle::Handle::refresh_verge();
            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            let previous_core = previous_app.core;
            let (previous_target, previous_app, previous_clash, previous_profiles, staged_content) =
                self.capture_reconcile_target(
                    previous_app,
                    clash,
                    profile_snapshot,
                    Default::default(),
                )
                .await?;
            let rollback = self
                .core
                .reconcile_profiles(
                    previous_clash,
                    previous_core,
                    run_type,
                    previous_profiles,
                    previous_app,
                    staged_content,
                    Some(previous_target),
                )
                .await;
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(anyhow::anyhow!(
                    "failed to persist selected core: {error}; failed to restore the previous Profile runtime: {rollback_error}"
                )),
            };
        }
        Ok(())
    }
}

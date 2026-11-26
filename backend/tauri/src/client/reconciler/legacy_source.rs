//! Compatibility source transaction and frozen input capture.
//! The canonical source actors still own CAS; this bridge uses their clients.
use crate::client::core_lifecycle::CoreLifecycleWorkflow;
use crate::client::reconciler::inputs::RuntimeInputs;
#[cfg(test)]
use crate::config::chimera::ClashCore;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use std::sync::Arc;

pub use chimera_core::client::core_lifecycle::command::ApplicationPatchResult;

impl CoreLifecycleWorkflow {
    fn source_transaction(
        &self,
    ) -> chimera_core::client::reconciler::source_transaction::SourceTransaction<'_> {
        chimera_core::client::reconciler::source_transaction::SourceTransaction {
            state: self.shared_state(),
            application: &self.application,
            clash: &self.clash,
        }
    }

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
        self.source_transaction()
            .capture_reconcile_target(
                crate::utils::dirs::app_profiles_dir,
                app,
                clash,
                profiles,
                staged_content,
            )
            .await
    }

    pub(in crate::client) async fn capture_profile_inputs_for(
        &self,
        app: ChimeraAppConfig,
        clash: chimera_config::clash::config::ClashConfig,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<RuntimeInputs> {
        self.source_transaction()
            .capture_profile_inputs_for(
                crate::utils::dirs::app_profiles_dir,
                app,
                clash,
                profiles,
                staged_content,
            )
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
        self.source_transaction()
            .capture_committed_profile_inputs_for(crate::utils::dirs::app_profiles_dir, app, clash)
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
        chimera_core::client::reconciler::startup_convergence::capture_target(self, app, clash)
            .await
    }

    pub(in crate::client) async fn startup_target_identity(&self) -> anyhow::Result<String> {
        self.source_transaction()
            .startup_target_identity(crate::utils::dirs::app_profiles_dir)
            .await
    }

    pub(in crate::client) async fn unresolved_startup_target_identity(&self) -> Option<String> {
        self.source_transaction()
            .unresolved_startup_target_identity()
            .await
    }

    pub(in crate::client) fn startup_source_identity(
        &self,
        profiles: Option<&Arc<chimera_config::profile::Profiles>>,
    ) -> anyhow::Result<String> {
        self.source_transaction().startup_source_identity(profiles)
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
        self.source_transaction()
            .committed_runtime_source_versions()
    }

    pub(in crate::client) async fn patch_application_config_with_result(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<ApplicationPatchResult> {
        use chimera_core::client::reconciler::source_transaction::ApplicationPatchCommit;
        match self
            .source_transaction()
            .commit_application_patch(
                patch,
                service_connected,
                async |candidate, connected, checked| {
                    self.check_application_candidate(candidate, connected, checked)
                        .await
                },
                cfg!(test),
            )
            .await?
        {
            ApplicationPatchCommit::Settled(result) => Ok(result),
            ApplicationPatchCommit::LegacyFixture {
                committed_version,
                expected_sources,
                candidate,
            } => {
                #[cfg(not(test))]
                {
                    let _ = (committed_version, expected_sources, candidate);
                    anyhow::bail!("Application runtime commit requires the canonical Reconciler");
                }

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
        if self.checked_gate.get().is_some() {
            return self
                .source_transaction()
                .check_candidate(candidate, use_checked)
                .await
                .expect("installed candidate gate is retained");
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
        chimera_core::client::reconciler::startup_convergence::reconcile_application_after_service_release(
            self.shared_state(), self, cfg!(test), app, expected_target,
        ).await
    }

    pub(in crate::client) async fn reconcile_profiles(
        &self,
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        self.source_transaction()
            .reconcile_profiles(
                crate::utils::dirs::app_profiles_dir,
                || crate::core::service::ipc::get_ipc_state().is_connected(),
                profiles,
                staged_content,
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

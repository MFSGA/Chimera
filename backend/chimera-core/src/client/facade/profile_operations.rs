//! Profile mutation facade. The actor remains the sole commit owner.
use super::ChimeraClient;
use crate::{
    client::event_sink::StateChanged,
    state::profiles::{CommitReport, ProfilesError, ReorderOp},
};
use anyhow::{Context as _, anyhow};
use chimera_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileLayout, ProfileMetadata,
    ProfileMetadataPatch, ProfileSource, RemoteProfileOptions, RemoteProfileOptionsPatch,
    TransformKind,
};
use chimera_contracts::runtime::{
    Degradation, DegradationPhase, DegradationReason, MutationOutcome,
};
use struct_patch::Patch as _;

impl ChimeraClient {
    fn refresh_profiles(&self) {
        self.inner.proxies.invalidate();
        self.inner
            .ui_sink
            .state_changed(StateChanged::Profiles { files: Vec::new() });
    }

    async fn collect_post_commit_degradations(&self, report: &CommitReport) -> Vec<Degradation> {
        let mut degradations: Vec<Degradation> = report
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
                crate::client::reconciler::profiles::map_profile_degradation(degradation)
            })
            .collect();
        degradations.extend(report.runtime_degradations.clone());
        degradations
    }

    async fn after_commit(&self, report: &CommitReport) -> MutationOutcome<()> {
        MutationOutcome::from_parts((), self.collect_post_commit_degradations(report).await)
            .with_commit(report.receipt.clone())
    }

    /// A failed second commit cannot erase the successfully created profile ID.
    async fn try_auto_activate_if_none(
        &self,
        profiles: &crate::client::profiles::ProfilesClient,
        uid: ProfileId,
    ) -> MutationOutcome<()> {
        match profiles.set_current_if_none(uid.clone()).await {
            Ok(None) => MutationOutcome::from_parts((), Vec::new()),
            Ok(Some(report)) => {
                self.refresh_profiles();
                self.after_commit(&report).await
            }
            Err(error) => MutationOutcome::from_parts(
                (),
                vec![auto_activation_failure_degradation(uid, &error)],
            ),
        }
    }

    pub async fn add_profile(
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

    pub async fn probe_profile_url(
        &self,
        url: url::Url,
        options: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<crate::profile_import::ProfileUrlProbe> {
        let mut option = RemoteProfileOptions::default();
        if let Some(patch) = options {
            option.apply(patch);
        }
        Ok(self.profiles_client()?.probe(url, option).await?)
    }

    pub async fn delete_profile(&self, uid: ProfileId) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.delete(uid).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn reorder_profile(
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

    pub async fn reorder_profiles_by_list(
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

    pub async fn set_profile_layout(
        &self,
        layout: ProfileLayout,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_layout(layout).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn create_profile_folder(
        &self,
        name: String,
        members: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<FolderId>> {
        let (report, folder) = self.profiles_client()?.create_folder(name, members).await?;
        self.refresh_profiles();
        Ok(MutationOutcome::from_parts(
            folder,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
    }

    pub async fn set_proxy_group_layout(
        &self,
        profile: ProfileId,
        layout: chimera_config::profile::ProxyGroupLayout,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .set_proxy_group_layout(profile, layout)
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn create_proxy_group_folder(
        &self,
        profile: ProfileId,
        name: String,
        members: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<FolderId>> {
        let (report, folder) = self
            .profiles_client()?
            .create_proxy_group_folder(profile, name, members)
            .await?;
        self.refresh_profiles();
        Ok(MutationOutcome::from_parts(
            folder,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
    }

    pub async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.refresh(uid, patch).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.patch_metadata(uid, patch).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn patch_remote_profile_options(
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

    pub async fn replace_profile_definition(
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
                self.inner
                    .ui_sink
                    .state_changed(StateChanged::RuntimeTransformDiagnostics);
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_current(uid).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self.profiles_client()?.set_global_transforms(ids).await {
            Ok(report) => report,
            Err(error) => {
                self.inner
                    .ui_sink
                    .state_changed(StateChanged::RuntimeTransformDiagnostics);
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_valid_fields(fields).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn save_profile_file(
        &self,
        uid: ProfileId,
        data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self.profiles_client()?.save_file(uid, data).await {
            Ok(report) => report,
            Err(error) => {
                self.inner
                    .ui_sink
                    .state_changed(StateChanged::RuntimeTransformDiagnostics);
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub async fn create_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        let profiles = self.profiles_client()?;
        let is_config = matches!(request.definition, ProfileDefinition::Config { .. });
        let mut outcome = self.add_profile(request, initial_file).await?;
        if is_config {
            let uid = outcome.value().clone();
            outcome =
                outcome.append_commit_result(self.try_auto_activate_if_none(profiles, uid).await);
        }
        Ok(outcome)
    }

    pub async fn import_profile_with_transform(
        &self,
        url: url::Url,
        name: Option<String>,
        patch: Option<RemoteProfileOptionsPatch>,
        transform: Option<TransformKind>,
        folder: Option<FolderId>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        let profiles = self.profiles_client()?;
        let update_interval_explicit = patch
            .as_ref()
            .and_then(|patch| patch.update_interval_minutes)
            .is_some();
        let (name, custom_name) = match name {
            Some(name) if !name.trim().is_empty() => (name, true),
            _ => (crate::profile_import::url_derived_name(&url), false),
        };
        let mut options = RemoteProfileOptions::default();
        if let Some(patch) = patch {
            options.apply(patch);
        }
        let report = profiles
            .import_with_transform(
                url,
                transform,
                ProfileMetadata {
                    name,
                    desc: None,
                    custom_name,
                },
                options,
                update_interval_explicit,
                folder,
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
        if transform.is_some() {
            return Ok(outcome);
        }
        Ok(outcome.append_commit_result(self.try_auto_activate_if_none(profiles, created).await))
    }

    pub async fn read_profile_file(&self, uid: ProfileId) -> anyhow::Result<String> {
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
            self.inner.profile_fs.as_ref(),
            &source.materialized().file,
        )?;
        profile_file_text_for_editor(raw, item.definition.is_config())
    }

    pub async fn get_profile_materialized_path(
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
            .profile_paths
            .resolve_path(&source.materialized().file)
            .map_err(Into::into)
    }
}

/// The real owner category is retained even for legacy source errors which
/// cannot supply a more specific upstream cause. The desktop keeps its existing
/// diagnostic message; a future remote registry must project its own redaction.
pub fn auto_activation_failure_degradation(
    profile: ProfileId,
    error: &ProfilesError,
) -> Degradation {
    tracing::warn!(%error, "profile auto-activation failed after commit; retaining committed profile id");
    Degradation {
        phase: DegradationPhase::SystemEffect,
        reason: DegradationReason::ProfileAutoActivationFailed {
            profile,
            cause: Box::new(error.typed_failure()),
        },
        message: error.to_string(),
        retryable: true,
    }
}

/// REF parity: config Profiles are YAML mappings, but transforms and scripts
/// must be returned byte-for-byte as text (even when they are not valid YAML).
fn profile_file_text_for_editor(raw: String, is_config: bool) -> anyhow::Result<String> {
    if is_config {
        chimera_profile_io::yaml::normalize_yaml_document(&raw)
            .map_err(anyhow::Error::new)
            .context("failed to normalize profile YAML")
    } else {
        Ok(raw)
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    #[test]
    fn transform_profile_file_read_preserves_non_yaml_source_exactly() {
        let javascript = "function main(config) { return config; }\n// [invalid YAML";
        let lua = "return { [\"proxies\"] = {} }\n-- not a YAML mapping";
        for source in [javascript, lua] {
            assert_eq!(
                profile_file_text_for_editor(source.into(), false).unwrap(),
                source,
                "script and merge transform content must never be normalized as YAML"
            );
        }
    }

    #[test]
    fn config_profile_file_read_normalizes_yaml_and_rejects_invalid_mapping() {
        let normalized = profile_file_text_for_editor("mode: rule\n".into(), true).unwrap();
        assert_eq!(normalized, "mode: rule\n");
        let error = profile_file_text_for_editor("not: [valid YAML".into(), true).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to normalize profile YAML")
        );
    }
}

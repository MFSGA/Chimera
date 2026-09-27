//! Profile application facade over the shared ProfilesClient.

use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use chimera_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch, ProfileSource, Profiles,
    RemoteProfileOptions, RemoteProfileOptionsPatch,
};
use struct_patch::Patch as _;

use super::{
    ChimeraClient, Degradation, DegradationPhase, MutationOutcome, profiles::ProfilesClient,
};
use crate::state::profiles::{
    CommitReport, NewProfileRequest, ProfilesError, ReorderOp, ports::ProfileFsPort,
};

fn post_commit_degradations(report: &CommitReport) -> Vec<Degradation> {
    let mut degradations = report
        .degradations
        .iter()
        .map(super::application_workflow::profiles::map_profile_degradation)
        .collect::<Vec<_>>();
    degradations.extend(report.runtime_degradations.clone());
    degradations
}

fn outcome<T>(value: T, report: &CommitReport) -> MutationOutcome<T> {
    MutationOutcome::from_parts(value, post_commit_degradations(report))
        .with_commit(report.receipt.clone())
}

fn auto_activation_failure(error: impl std::fmt::Display) -> Degradation {
    Degradation {
        phase: DegradationPhase::SystemEffect,
        code: "profile_auto_activation_failed".into(),
        message: error.to_string(),
        retryable: true,
    }
}

async fn try_auto_activate_if_none(client: &ChimeraClient, uid: ProfileId) -> MutationOutcome<()> {
    let profiles = match client.profiles_client() {
        Ok(profiles) => profiles.clone(),
        Err(error) => {
            return MutationOutcome::from_parts((), vec![auto_activation_failure(error)]);
        }
    };
    match profiles.set_current_if_none(uid).await {
        Ok(None) => MutationOutcome::from_parts((), Vec::new()),
        Ok(Some(report)) => outcome((), &report),
        Err(error) => MutationOutcome::from_parts((), vec![auto_activation_failure(error)]),
    }
}

fn with_more_degradations<T>(
    result: MutationOutcome<T>,
    extra: MutationOutcome<()>,
) -> MutationOutcome<T> {
    result.append_commit_result(extra)
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

impl ChimeraClient {
    fn profiles_client(&self) -> anyhow::Result<&ProfilesClient> {
        self.inner
            .typed_profiles
            .as_ref()
            .ok_or_else(|| anyhow!("typed ProfilesClient is not configured"))
    }

    pub(crate) fn get_profiles(&self) -> anyhow::Result<Arc<Profiles>> {
        Ok(self.profiles_client()?.snapshot())
    }

    pub(crate) async fn create_profile(
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
        let is_config = matches!(request.definition, ProfileDefinition::Config { .. });
        let report = self.profiles_client()?.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| anyhow!("profile add committed without a created uid"))?;
        self.inner.ui_sink.refresh_profiles();
        let result = outcome(created.clone(), &report);
        if is_config {
            let extra = try_auto_activate_if_none(self, created).await;
            Ok(with_more_degradations(result, extra))
        } else {
            Ok(result)
        }
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
        self.inner.ui_sink.refresh_profiles();
        let result = outcome(created.clone(), &report);
        Ok(with_more_degradations(
            result,
            try_auto_activate_if_none(self, created).await,
        ))
    }

    pub(crate) async fn delete_profile(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.delete(uid).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
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
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .reorder(ReorderOp::ByList(list))
            .await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn update_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.refresh(uid, patch).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.patch_metadata(uid, patch).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
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
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .replace_definition(uid, definition)
            .await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_current(uid).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_global_transforms(ids).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_valid_fields(fields).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
    }

    pub(crate) async fn save_profile_file(
        &self,
        uid: ProfileId,
        data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.save_file(uid, data).await?;
        self.inner.ui_sink.refresh_profiles();
        Ok(outcome((), &report))
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
        let raw = self
            .inner
            .profile_service
            .read(&source.materialized().file)?;
        if item.definition.is_config() {
            crate::service::profile_file::normalize_yaml_document(&raw)
                .context("failed to normalize profile YAML")
        } else {
            Ok(raw)
        }
    }

    pub(crate) async fn profile_path(&self, uid: ProfileId) -> anyhow::Result<std::path::PathBuf> {
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

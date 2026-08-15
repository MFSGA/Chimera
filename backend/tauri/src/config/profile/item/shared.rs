use std::{fs::OpenOptions, io::Write, path::PathBuf};

use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::config::profile::{
    item::{ProfileMetaGetter, utils::resolve_managed_profile_path},
    item_type::ProfileItemType,
};

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type)]
#[builder(
    derive(Debug, serde::Serialize, serde::Deserialize, specta::Type),
    build_fn(skip)
)]
#[builder_update(patch_fn = "apply", getter)]
pub struct ProfileShared {
    /// Profile ID
    pub uid: String,
    /// profile name
    pub name: String,
    /// profile holds the file
    // #[serde(alias = "file", deserialize_with = "deserialize_option_single_or_vec")]
    #[builder(default = "self.default_files()?")]
    pub file: String,
    /// profile description
    #[builder(default, setter(strip_option))]
    pub desc: Option<String>,
    #[builder(default = "chrono::Local::now().timestamp() as usize")]
    /// update time
    pub updated: usize,
}

impl ProfileShared {
    pub fn get_default_builder(kind: &ProfileItemType) -> ProfileSharedBuilder {
        let mut builder = ProfileSharedBuilder::default();
        builder
            .name(ProfileSharedBuilder::default_name(kind).to_string())
            .uid(ProfileSharedBuilder::default_uid(kind));
        builder
    }
}

impl ProfileSharedBuilder {
    fn default_uid(kind: &ProfileItemType) -> String {
        super::utils::generate_uid(kind)
    }

    pub fn default_name(kind: &ProfileItemType) -> &'static str {
        match kind {
            ProfileItemType::Remote => "Remote Profile",
            ProfileItemType::Local => "Local Profile",
            // ProfileItemType::Merge => "Merge Profile",
            // ProfileItemType::Script(_) => "Script Profile",
        }
    }

    pub fn default_file_name(kind: &ProfileItemType, uid: &str) -> String {
        match kind {
            ProfileItemType::Remote => format!("{uid}.yaml"),
            ProfileItemType::Local => format!("{uid}.yaml"),
            // ProfileItemType::Merge => format!("{uid}.yaml"),
            // ProfileItemType::Script(ScriptType::JavaScript) => format!("{uid}.js"),
            // ProfileItemType::Script(ScriptType::Lua) => format!("{uid}.lua"),
        }
    }

    pub fn assign_managed_identity(&mut self, kind: &ProfileItemType, uid: String) {
        let file = Self::default_file_name(kind, &uid);
        self.uid(uid).file(file);
    }

    pub fn build(
        &self,
        kind: &ProfileItemType,
    ) -> Result<ProfileShared, ProfileSharedBuilderError> {
        let mut builder = self.clone();
        if self.uid.is_none() {
            builder.uid = Some(Self::default_uid(kind));
        }
        if self.name.is_none() {
            builder.name = Some(Self::default_name(kind).to_string());
        }
        if self.file.is_none() {
            builder.file = Some(Self::default_file_name(kind, builder.uid.as_ref().unwrap()));
        }

        Ok(ProfileShared {
            uid: builder.uid.unwrap(),
            name: builder.name.unwrap(),
            file: builder.file.unwrap(),
            desc: builder.desc.clone().unwrap_or_default(),
            updated: builder
                .updated
                .unwrap_or_else(|| chrono::Local::now().timestamp() as usize),
        })
    }
}

pub(crate) const PROFILE_RESERVATION_MAGIC: &str = "clash-chimera-profile-reservation-v1";

pub(crate) fn profile_reservation_marker(file: &str) -> String {
    format!("{PROFILE_RESERVATION_MAGIC}\n{file}\n")
}

pub(crate) struct PreparedProfileFile {
    target: PathBuf,
    reservation: PathBuf,
    materialized: bool,
    committed: bool,
}

impl PreparedProfileFile {
    pub(crate) fn reserve(file: &str) -> anyhow::Result<Option<Self>> {
        let target = resolve_managed_profile_path(file)?;
        let reservation = resolve_managed_profile_path(&format!(".{file}.reserve"))?;
        Self::reserve_paths(target, reservation)
    }

    fn reserve_paths(target: PathBuf, reservation: PathBuf) -> anyhow::Result<Option<Self>> {
        let target_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("managed profile target has no UTF-8 file name"))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut reservation_file = match options.open(&reservation) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let write_result = reservation_file
            .write_all(profile_reservation_marker(target_name).as_bytes())
            .and_then(|()| reservation_file.sync_all());
        drop(reservation_file);
        if let Err(primary_error) = write_result {
            return match std::fs::remove_file(&reservation) {
                Ok(()) => Err(primary_error.into()),
                Err(cleanup_error) => Err(anyhow::anyhow!(
                    "failed to initialize profile reservation: {primary_error}; cleanup failed: {cleanup_error}"
                )),
            };
        }

        if target.exists() {
            std::fs::remove_file(&reservation)?;
            return Ok(None);
        }

        Ok(Some(Self {
            target,
            reservation,
            materialized: false,
            committed: false,
        }))
    }

    pub(crate) fn mark_materialized(&mut self) {
        self.materialized = true;
    }

    pub(crate) fn commit(mut self) {
        self.committed = true;
        if let Err(error) = std::fs::remove_file(&self.reservation) {
            tracing::warn!(
                path = %self.reservation.display(),
                %error,
                "failed to remove committed profile reservation"
            );
        }
    }

    #[cfg(test)]
    fn reserve_in(root: &std::path::Path, file: &str) -> anyhow::Result<Option<Self>> {
        Self::reserve_paths(root.join(file), root.join(format!(".{file}.reserve")))
    }
}

impl Drop for PreparedProfileFile {
    fn drop(&mut self) {
        if !self.committed
            && self.materialized
            && self.target.exists()
            && let Err(error) = std::fs::remove_file(&self.target)
        {
            tracing::warn!(
                path = %self.target.display(),
                %error,
                "failed to remove uncommitted profile materialization"
            );
        }
        if self.reservation.exists()
            && let Err(error) = std::fs::remove_file(&self.reservation)
        {
            tracing::warn!(
                path = %self.reservation.display(),
                %error,
                "failed to remove profile reservation"
            );
        }
    }
}

impl ProfileMetaGetter for ProfileShared {
    fn uid(&self) -> &str {
        &self.uid
    }
}

impl super::ProfileMetaSetter for ProfileShared {
    fn set_uid(&mut self, uid: String) {
        self.uid = uid;
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn set_desc(&mut self, desc: Option<String>) {
        self.desc = desc;
    }

    fn set_file(&mut self, file: String) {
        self.file = file;
    }

    fn set_updated(&mut self, updated: usize) {
        self.updated = updated;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_identity_overrides_client_uid_and_file() {
        let mut builder = ProfileSharedBuilder::default();
        builder
            .uid("l-client".to_string())
            .file("victim.yaml".to_string())
            .name("Client profile".to_string());

        builder.assign_managed_identity(&ProfileItemType::Local, "l-server".to_string());
        let shared = builder.build(&ProfileItemType::Local).unwrap();

        assert_eq!(shared.uid, "l-server");
        assert_eq!(shared.file, "l-server.yaml");
    }

    #[test]
    fn reservation_refuses_an_existing_materialized_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("l-existing.yaml");
        std::fs::write(&target, "mode: rule\n").unwrap();

        let prepared = PreparedProfileFile::reserve_in(dir.path(), "l-existing.yaml").unwrap();

        assert!(prepared.is_none());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "mode: rule\n");
        assert!(!dir.path().join(".l-existing.yaml.reserve").exists());
    }

    #[tokio::test]
    async fn cancelled_creation_removes_materialization_and_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("l-cancelled.yaml");
        let reservation = dir.path().join(".l-cancelled.yaml.reserve");
        let task_target = target.clone();
        let task_reservation = reservation.clone();
        let task_root = dir.path().to_path_buf();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

        let task = tokio::spawn(async move {
            let mut prepared = PreparedProfileFile::reserve_in(&task_root, "l-cancelled.yaml")
                .unwrap()
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(&task_reservation).unwrap(),
                profile_reservation_marker("l-cancelled.yaml")
            );
            std::fs::write(&task_target, "mode: direct\n").unwrap();
            prepared.mark_materialized();
            let _ = ready_tx.send(());
            std::future::pending::<()>().await;
            prepared.commit();
        });

        ready_rx.await.unwrap();
        assert!(target.exists());
        assert!(reservation.exists());
        task.abort();
        let _ = task.await;

        assert!(!target.exists());
        assert!(!reservation.exists());
    }

    #[test]
    fn committed_creation_keeps_materialization_and_releases_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("l-committed.yaml");
        let reservation = dir.path().join(".l-committed.yaml.reserve");
        let mut prepared = PreparedProfileFile::reserve_in(dir.path(), "l-committed.yaml")
            .unwrap()
            .unwrap();
        std::fs::write(&target, "mode: rule\n").unwrap();
        prepared.mark_materialized();

        prepared.commit();

        assert_eq!(std::fs::read_to_string(target).unwrap(), "mode: rule\n");
        assert!(!reservation.exists());
    }
}

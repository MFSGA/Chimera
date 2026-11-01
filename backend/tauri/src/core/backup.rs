//! Full snapshots of the config dir and `storage.db` under `<data>/backups/`.
//!
//! A backup is a stateless filesystem operation: every input arrives through
//! the request, so migrations and [`ChimeraClient`](crate::client::ChimeraClient)
//! share it.
//!
//! Chimera currently uses this module only for pre-client migration snapshots.
//! Its storage owner lacks the ref live snapshot API, so live/manual backups
//! remain unconnected; migration snapshots use a closed storage file.

use crate::utils::path::PathResolver;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, UtcOffset};

mod verify;

pub const MIGRATION_PREFIX: &str = "migration-";
pub const MANUAL_PREFIX: &str = "manual-";
pub const KEEP_MIGRATION_BACKUPS: usize = 3;
pub const KEEP_MANUAL_BACKUPS: usize = 3;

const MANIFEST_FILE: &str = "manifest.json";
const CONFIG_DIR: &str = "config";
const DATA_DIR: &str = "data";
const PARTIAL_SUFFIX: &str = ".partial";
/// `from` of a migration backup whose state file records no earlier version.
const UNKNOWN_VERSION: &str = "unknown";
/// Length of the `YYYYMMDDTHHMMSSZ` stamp that follows a backup name's prefix.
const STAMP_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("failed to access {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("backup source or destination is not a safe file or directory: {}", path.display())]
    InvalidNode { path: PathBuf },
    #[error("failed to verify backup contents")]
    Verification(#[from] verify::VerificationError),
    #[error("failed to encode the backup manifest")]
    Manifest(#[source] serde_json::Error),
}

pub enum BackupKind<'a> {
    Migration {
        from: Option<&'a Version>,
        target: &'a Version,
    },
    Manual,
}

/// Where `storage.db` comes from.
pub enum StorageSource<'a> {
    /// No process has the file open (the migration subprocess): copy it.
    File(&'a Path),
}

pub struct BackupRequest<'a> {
    pub paths: &'a PathResolver,
    pub storage: StorageSource<'a>,
    pub kind: BackupKind<'a>,
    pub now: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub name: String,
    pub path: PathBuf,
}

impl BackupInfo {
    /// A committed directory is not proof that its contents are recoverable.
    /// The reporter may advertise only a complete, content-verified snapshot.
    pub(crate) fn is_available(&self) -> bool {
        verify::verify_backup(self).is_ok()
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    kind: String,
    created_at: String,
    app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_version: Option<String>,
    sources: ManifestSources,
    symlinks: Vec<ManifestSymlink>,
    /// Additive extension to the ref manifest; old snapshots are left intact
    /// but cannot claim content verification until they have a file inventory.
    format_version: u32,
    files: Vec<verify::ManifestFile>,
    directories: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct ManifestSources {
    config: PathBuf,
    data: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct ManifestSymlink {
    path: String,
    target: PathBuf,
}

/// Creates a full backup. A failure leaves neither a backup directory nor a
/// `.partial` behind.
pub fn create_backup(req: &BackupRequest<'_>) -> Result<BackupInfo, BackupError> {
    let backups_dir = req.paths.backups_dir();
    // The backup root itself may be a dangling or live symlink. Never follow it.
    match fs::symlink_metadata(&backups_dir) {
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err(BackupError::InvalidNode { path: backups_dir });
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(&backups_dir).map_err(io_error(&backups_dir))?;
        }
        Err(error) => return Err(io_error(&backups_dir)(error)),
    }
    let actual = fs::symlink_metadata(&backups_dir).map_err(io_error(&backups_dir))?;
    if !actual.is_dir() || actual.file_type().is_symlink() {
        return Err(BackupError::InvalidNode { path: backups_dir });
    }
    // Do not delete unknown/in-progress .partial folders. A different process
    // may still own one; this call cleans only the partial it reserved. (ref
    // cleanup is intentionally deferred until safe stale-owner detection.)

    let (name, partial) = reserve_partial(&backups_dir, &backup_name(req))?;
    let target = backups_dir.join(&name);

    let result = write_backup(req, &partial)
        .and_then(|()| fs::rename(&partial, &target).map_err(io_error(&target)));
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&partial);
        return Err(error);
    }

    Ok(BackupInfo { name, path: target })
}

/// Prunes only committed backups with a valid matching manifest, preserving
/// unknown directories and all `.partial` nodes that may have a live owner.
pub fn prune_backups(backups_dir: &Path, prefix: &str, keep: usize) -> Result<(), BackupError> {
    match fs::symlink_metadata(backups_dir) {
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err(BackupError::InvalidNode {
                path: backups_dir.to_owned(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(backups_dir)(error)),
    }
    let entries = match fs::read_dir(backups_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(backups_dir)(error)),
    };

    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error(backups_dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !entry.file_type().map_err(io_error(&path))?.is_dir() {
            continue;
        }

        if is_partial(&name) {
            continue;
        }
        if let Some(rest) = name.strip_prefix(prefix) {
            // Never spend the retention allowance on a damaged snapshot and
            // then delete an older intact copy. Ref-only/unindexed snapshots
            // remain untouched; they cannot be proved safe to discard.
            let candidate = BackupInfo {
                name: name.clone(),
                path: path.clone(),
            };
            if !candidate.is_available() {
                continue;
            }
            let stamp = rest.get(..STAMP_LEN).unwrap_or_default().to_owned();
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
            backups.push(((stamp, modified, name), path));
        }
    }

    backups.sort_by(|a, b| a.0.cmp(&b.0));
    let stale = backups.len().saturating_sub(keep);
    for (_, path) in backups.into_iter().take(stale) {
        fs::remove_dir_all(&path).map_err(io_error(&path))?;
    }
    Ok(())
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> BackupError + '_ {
    move |source| BackupError::Io {
        path: path.to_owned(),
        source,
    }
}

fn is_partial(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(PARTIAL_SUFFIX)
}

fn backup_name(req: &BackupRequest<'_>) -> String {
    let stamp = file_stamp(req.now);
    match &req.kind {
        BackupKind::Migration { from, target } => {
            let from = from.map_or_else(|| UNKNOWN_VERSION.to_owned(), ToString::to_string);
            format!("{MIGRATION_PREFIX}{stamp}-{from}-to-{target}")
        }
        BackupKind::Manual => format!("{MANUAL_PREFIX}{stamp}"),
    }
}

/// Claims the first free name (`base`, `base-1`, `base-2`, ...) by creating its
/// `.partial` directory, so concurrent backups cannot share one.
fn reserve_partial(backups_dir: &Path, base: &str) -> Result<(String, PathBuf), BackupError> {
    for attempt in 0_u32.. {
        let name = match attempt {
            0 => base.to_owned(),
            n => format!("{base}-{n}"),
        };
        match fs::symlink_metadata(backups_dir.join(&name)) {
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(backups_dir)(error)),
        }

        let partial = backups_dir.join(format!(".{name}{PARTIAL_SUFFIX}"));
        match fs::create_dir(&partial) {
            Ok(()) => return Ok((name, partial)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(&partial)(error)),
        }
    }
    unreachable!("the attempt counter does not run out")
}

fn write_backup(req: &BackupRequest<'_>, partial: &Path) -> Result<(), BackupError> {
    let skipped = skipped_paths(req.paths);
    let mut symlinks = Vec::new();
    copy_dir(
        req.paths.app_config_dir(),
        &partial.join(CONFIG_DIR),
        Path::new(CONFIG_DIR),
        &skipped,
        &mut symlinks,
    )?;

    backup_storage(req, &partial.join(DATA_DIR))?;

    // Hash the actual staged bytes, not the originals: files may have changed
    // between the config read and the copy. Integrity covers what we saved.
    let inventory = verify::scan_snapshot(partial)?;
    let manifest = manifest(req, symlinks, inventory);
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(BackupError::Manifest)?;
    let manifest_path = partial.join(MANIFEST_FILE);
    fs::write(&manifest_path, manifest).map_err(io_error(&manifest_path))?;
    // Validate the staged snapshot before making it visible as a committed
    // backup. Any mismatch causes create_backup to remove only its own partial.
    verify::verify_at(partial)?;
    Ok(())
}

/// Data-dir paths that sit inside the config dir when both are the same tree.
/// None of them is config: the backups themselves, the stores redb keeps
/// locked, and runtime data the backup does not cover.
fn skipped_paths(paths: &PathResolver) -> Vec<PathBuf> {
    [
        paths.backups_dir(),
        paths.storage_path(),
        paths.jobs_path(),
        paths.app_logs_dir(),
        paths.cache_dir(),
        paths.scripts_dir(),
        paths.clash_pid_path(),
    ]
    .into_iter()
    .collect()
}

/// Copies `src` into `dst`. Symlinks are neither followed nor recreated; they
/// are recorded under `rel`, their path relative to the backup root.
fn copy_dir(
    src: &Path,
    dst: &Path,
    rel: &Path,
    skipped: &[PathBuf],
    symlinks: &mut Vec<ManifestSymlink>,
) -> Result<(), BackupError> {
    let root = fs::symlink_metadata(src).map_err(io_error(src))?;
    if !root.is_dir() || root.file_type().is_symlink() {
        return Err(BackupError::InvalidNode {
            path: src.to_owned(),
        });
    }
    fs::create_dir_all(dst).map_err(io_error(dst))?;
    for entry in fs::read_dir(src).map_err(io_error(src))? {
        let entry = entry.map_err(io_error(src))?;
        let path = entry.path();
        if skipped.contains(&path) {
            continue;
        }

        let dst = dst.join(entry.file_name());
        let rel = rel.join(entry.file_name());
        let file_type = entry.file_type().map_err(io_error(&path))?;
        if file_type.is_symlink() {
            symlinks.push(ManifestSymlink {
                path: rel.to_string_lossy().replace('\\', "/"),
                target: fs::read_link(&path).map_err(io_error(&path))?,
            });
        } else if file_type.is_dir() {
            copy_dir(&path, &dst, &rel, skipped, symlinks)?;
        } else if file_type.is_file() {
            fs::copy(&path, &dst).map_err(io_error(&path))?;
        } else {
            return Err(BackupError::InvalidNode { path });
        }
    }
    Ok(())
}

fn backup_storage(req: &BackupRequest<'_>, data_dir: &Path) -> Result<(), BackupError> {
    let storage_path = req.paths.storage_path();
    let dest = data_dir.join(storage_path.file_name().unwrap_or_default());

    match &req.storage {
        StorageSource::File(source) => {
            match fs::symlink_metadata(source) {
                Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                    return Err(BackupError::InvalidNode {
                        path: (*source).to_path_buf(),
                    });
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(io_error(source)(error)),
            }
            fs::create_dir_all(data_dir).map_err(io_error(data_dir))?;
            fs::copy(source, &dest).map_err(io_error(source))?;
        }
    }
    Ok(())
}

fn manifest(
    req: &BackupRequest<'_>,
    symlinks: Vec<ManifestSymlink>,
    inventory: verify::Inventory,
) -> Manifest {
    let (kind, from_version, target_version) = match &req.kind {
        BackupKind::Migration { from, target } => (
            "migration",
            Some(from.map_or_else(|| UNKNOWN_VERSION.to_owned(), ToString::to_string)),
            Some(target.to_string()),
        ),
        BackupKind::Manual => ("manual", None, None),
    };
    Manifest {
        kind: kind.to_owned(),
        created_at: rfc3339(req.now),
        app_version: crate::utils::dirs::APP_VERSION.to_owned(),
        from_version,
        target_version,
        sources: ManifestSources {
            config: req.paths.app_config_dir().to_owned(),
            data: req.paths.app_data_dir().to_owned(),
        },
        symlinks,
        format_version: verify::FORMAT_VERSION,
        files: inventory.files,
        directories: inventory.directories,
    }
}

fn file_stamp(now: OffsetDateTime) -> String {
    let now = now.to_offset(UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn rfc3339(now: OffsetDateTime) -> String {
    let now = now.to_offset(UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-09-21T14:13:20Z
    const NOW: i64 = 1_790_000_000;
    const STAMP: &str = "20260921T141320Z";

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(NOW).unwrap()
    }

    fn paths(root: &Path) -> PathResolver {
        PathResolver::with_base_dirs(root.join("config"), root.join("data"))
    }

    fn write(path: &(impl AsRef<Path> + ?Sized), content: &str) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn manual<'a>(paths: &'a PathResolver, storage: StorageSource<'a>) -> BackupRequest<'a> {
        BackupRequest {
            paths,
            storage,
            kind: BackupKind::Manual,
            now: now(),
        }
    }

    fn read_manifest(backup: &BackupInfo) -> serde_json::Value {
        serde_json::from_slice(&fs::read(backup.path.join(MANIFEST_FILE)).unwrap()).unwrap()
    }

    fn names(dir: &(impl AsRef<Path> + ?Sized)) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir.as_ref())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn verified_snapshot_restores_exact_original_bytes_to_a_new_sandbox() {
        use redb::ReadableDatabase as _;
        const VALUES: redb::TableDefinition<&str, &str> =
            redb::TableDefinition::new("restore-proof");
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(temp.path());
        write(&paths.profiles_path(), "current: [old-profile]\n");
        write(
            &paths.app_profiles_dir().join("nested/a.yaml"),
            "profile: old\n",
        );
        fs::create_dir_all(paths.app_profiles_dir().join("empty")).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let storage_path = paths.storage_path();
        {
            let db = redb::Database::create(&storage_path).unwrap();
            let tx = db.begin_write().unwrap();
            tx.open_table(VALUES)
                .unwrap()
                .insert("mode", "before-upgrade")
                .unwrap();
            tx.commit().unwrap();
        }
        let backup = create_backup(&manual(&paths, StorageSource::File(&storage_path))).unwrap();
        assert!(backup.is_available());
        let manifest = read_manifest(&backup);
        assert_eq!(manifest["format_version"], verify::FORMAT_VERSION);
        assert!(
            manifest["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file["path"] == "data/storage.db")
        );
        assert!(
            manifest["directories"]
                .as_array()
                .unwrap()
                .iter()
                .any(|dir| dir == "config/profiles/empty")
        );

        // The source changes after the snapshot. The restored copy must still
        // contain the exact original profile and database transaction.
        fs::write(paths.profiles_path(), b"current: [new-profile]\n").unwrap();
        {
            let db = redb::Database::open(&storage_path).unwrap();
            let tx = db.begin_write().unwrap();
            tx.open_table(VALUES)
                .unwrap()
                .insert("mode", "after-upgrade")
                .unwrap();
            tx.commit().unwrap();
        }
        let restored = temp.path().join("isolated-restore");
        verify::restore_to_empty_sandbox(&backup, &restored).unwrap();
        assert_eq!(
            fs::read_to_string(restored.join("config/profiles.yaml")).unwrap(),
            "current: [old-profile]\n"
        );
        assert_eq!(
            fs::read_to_string(restored.join("config/profiles/nested/a.yaml")).unwrap(),
            "profile: old\n"
        );
        assert!(restored.join("config/profiles/empty").is_dir());
        let restored_db = redb::Database::open(restored.join("data/storage.db")).unwrap();
        let tx = restored_db.begin_read().unwrap();
        let table = tx.open_table(VALUES).unwrap();
        assert_eq!(
            table.get("mode").unwrap().unwrap().value(),
            "before-upgrade"
        );
        assert_eq!(
            fs::read_to_string(paths.profiles_path()).unwrap(),
            "current: [new-profile]\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn restore_recreates_recorded_symlink_nodes_without_copying_external_files() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(temp.path());
        write(&paths.profiles_path(), "original\n");
        let external = temp.path().join("external-source.yaml");
        fs::write(&external, b"do not copy this content").unwrap();
        let linked = paths.app_profiles_dir().join("linked.yaml");
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        symlink(&external, &linked).unwrap();
        let storage = paths.storage_path();
        let backup = create_backup(&manual(&paths, StorageSource::File(&storage))).unwrap();
        assert!(backup.is_available());
        let restore = temp.path().join("restored-link-nodes");
        verify::restore_to_empty_sandbox(&backup, &restore).unwrap();
        assert_eq!(
            fs::read_link(restore.join("config/profiles/linked.yaml")).unwrap(),
            external
        );
        assert_eq!(fs::read(&external).unwrap(), b"do not copy this content");
        assert!(!backup.path.join("config/profiles/linked.yaml").exists());
    }

    #[test]
    fn damaged_or_incomplete_snapshot_is_never_advertised_or_extracted() {
        for corruption in [
            "same-size-edit",
            "remove",
            "extra",
            "legacy-manifest",
            "unsafe-entry",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let paths = paths(temp.path());
            write(&paths.profiles_path(), "original data\n");
            let storage = paths.storage_path();
            let backup = create_backup(&manual(&paths, StorageSource::File(&storage))).unwrap();
            assert!(backup.is_available());
            let stored = backup.path.join("config/profiles.yaml");
            match corruption {
                "same-size-edit" => fs::write(&stored, b"modified data\n").unwrap(),
                "remove" => fs::remove_file(&stored).unwrap(),
                "extra" => fs::write(backup.path.join("config/unlisted.yaml"), "injected").unwrap(),
                "legacy-manifest" | "unsafe-entry" => {
                    let file = backup.path.join(MANIFEST_FILE);
                    let mut json = read_manifest(&backup);
                    if corruption == "legacy-manifest" {
                        let object = json.as_object_mut().unwrap();
                        object.remove("format_version");
                        object.remove("files");
                        object.remove("directories");
                    } else {
                        json["files"][0]["path"] = serde_json::json!("config/../../escape.yaml");
                    }
                    fs::write(file, serde_json::to_vec(&json).unwrap()).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                !backup.is_available(),
                "corruption {corruption} must be detected"
            );
            let restore = temp.path().join("should-not-be-created");
            assert!(verify::restore_to_empty_sandbox(&backup, &restore).is_err());
            assert!(
                !restore.exists(),
                "verification precedes destination creation"
            );
            assert_eq!(
                fs::read_to_string(paths.profiles_path()).unwrap(),
                "original data\n"
            );
        }
    }

    #[test]
    fn sandbox_restore_never_overwrites_a_preexisting_destination() {
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(temp.path());
        write(&paths.profiles_path(), "old\n");
        let storage = paths.storage_path();
        let backup = create_backup(&manual(&paths, StorageSource::File(&storage))).unwrap();
        let existing = temp.path().join("user-existing-data");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("keep.txt"), b"user data").unwrap();
        assert!(verify::restore_to_empty_sandbox(&backup, &existing).is_err());
        assert_eq!(fs::read(existing.join("keep.txt")).unwrap(), b"user data");
    }

    #[test]
    fn copies_the_config_dir_and_storage_with_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        write(&paths.app_profiles_dir().join("a.yaml"), "a");
        write(&paths.storage_path(), "db");
        let from = Version::new(1, 6, 1);
        let target = Version::new(2, 0, 0);

        let backup = create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(paths.storage_path().as_path()),
            kind: BackupKind::Migration {
                from: Some(&from),
                target: &target,
            },
            now: now(),
        })
        .unwrap();

        assert_eq!(backup.name, format!("migration-{STAMP}-1.6.1-to-2.0.0"));
        assert_eq!(backup.path, paths.backups_dir().join(&backup.name));
        let config = backup.path.join(CONFIG_DIR);
        assert_eq!(
            fs::read_to_string(config.join("profiles.yaml")).unwrap(),
            "profiles"
        );
        assert_eq!(
            fs::read_to_string(config.join("profiles").join("a.yaml")).unwrap(),
            "a"
        );
        assert_eq!(
            fs::read_to_string(backup.path.join("data").join("storage.db")).unwrap(),
            "db"
        );

        let manifest = read_manifest(&backup);
        assert_eq!(manifest["kind"], "migration");
        assert_eq!(manifest["created_at"], "2026-09-21T14:13:20Z");
        assert_eq!(manifest["from_version"], "1.6.1");
        assert_eq!(manifest["target_version"], "2.0.0");
        assert_eq!(
            manifest["sources"]["config"],
            paths.app_config_dir().to_str().unwrap()
        );
        assert_eq!(
            manifest["sources"]["data"],
            paths.app_data_dir().to_str().unwrap()
        );
        assert_eq!(manifest["symlinks"], serde_json::json!([]));
        assert_eq!(names(&paths.backups_dir()), [backup.name]);
    }

    #[test]
    fn a_manual_backup_without_storage_records_no_versions() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_path()),
        ))
        .unwrap();

        assert_eq!(backup.name, format!("manual-{STAMP}"));
        assert!(!backup.path.join("data").exists());
        let manifest = read_manifest(&backup);
        assert_eq!(manifest["kind"], "manual");
        assert!(manifest.get("from_version").is_none());
        assert!(manifest.get("target_version").is_none());
    }

    #[test]
    fn a_migration_without_a_recorded_version_is_named_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        let target = Version::new(2, 0, 0);

        let backup = create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(paths.storage_path().as_path()),
            kind: BackupKind::Migration {
                from: None,
                target: &target,
            },
            now: now(),
        })
        .unwrap();

        assert_eq!(backup.name, format!("migration-{STAMP}-unknown-to-2.0.0"));
        assert_eq!(read_manifest(&backup)["from_version"], "unknown");
    }

    #[test]
    fn a_data_dir_inside_the_config_dir_does_not_copy_the_backups() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let paths = PathResolver::with_base_dirs(config.clone(), config.clone());
        write(&paths.profiles_path(), "profiles");
        write(&paths.storage_path(), "db");
        write(&paths.app_logs_dir().join("app.log"), "log");
        write(&paths.jobs_path(), "jobs");

        let storage = paths.storage_path();
        let first = create_backup(&manual(&paths, StorageSource::File(storage.as_path()))).unwrap();
        let mut request = manual(&paths, StorageSource::File(storage.as_path()));
        request.now = now() + time::Duration::seconds(1);
        let second = create_backup(&request).unwrap();

        assert_eq!(names(&second.path.join(CONFIG_DIR)), ["profiles.yaml"]);
        assert_eq!(names(&second.path.join("data")), ["storage.db"]);
        assert!(first.path.exists());
    }

    #[test]
    fn symlinks_are_recorded_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let external = dir.path().join("external.yaml");
        write(&external, "external");
        fs::create_dir_all(paths.app_profiles_dir()).unwrap();
        let link = paths.app_profiles_dir().join("local.yaml");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&external, &link).is_err() {
            // Creating symlinks needs developer mode or elevation.
            return;
        }

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_path()),
        ))
        .unwrap();

        assert!(
            !backup
                .path
                .join(CONFIG_DIR)
                .join("profiles")
                .join("local.yaml")
                .exists()
        );
        let manifest = read_manifest(&backup);
        assert_eq!(
            manifest["symlinks"],
            serde_json::json!([{
                "path": "config/profiles/local.yaml",
                "target": external.to_str().unwrap(),
            }])
        );
    }

    #[test]
    fn a_failure_midway_leaves_neither_a_backup_nor_a_partial() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        // A directory cannot be copied as the storage file.
        let unreadable = dir.path().join("unreadable");
        fs::create_dir_all(&unreadable).unwrap();

        let error = create_backup(&manual(&paths, StorageSource::File(&unreadable))).unwrap_err();

        assert!(
            matches!(error, BackupError::InvalidNode { .. }),
            "{error:?}"
        );
        assert!(names(&paths.backups_dir()).is_empty());
    }

    #[test]
    fn a_second_backup_in_the_same_second_gets_a_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        let source = paths.storage_path();

        let first = create_backup(&manual(&paths, StorageSource::File(source.as_path()))).unwrap();
        let second = create_backup(&manual(&paths, StorageSource::File(source.as_path()))).unwrap();

        assert_eq!(first.name, format!("manual-{STAMP}"));
        assert_eq!(second.name, format!("manual-{STAMP}-1"));
        assert!(first.path.join(MANIFEST_FILE).exists());
    }

    #[test]
    fn pruning_never_deletes_legacy_unverified_backups_or_unowned_partials() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path().join("backups");
        for name in [
            "migration-20260101T000000Z-1.0.0-to-2.0.0",
            "migration-20260301T000000Z-1.0.0-to-2.0.0",
            "migration-20260201T000000Z-1.0.0-to-2.0.0",
            "migration-20260401T000000Z-1.0.0-to-2.0.0",
            "manual-20250101T000000Z",
            ".migration-20260501T000000Z-1.0.0-to-2.0.0.partial",
            "unrelated",
        ] {
            fs::create_dir_all(backups.join(name)).unwrap();
            if name.starts_with("migration-") {
                fs::write(
                    backups.join(name).join(MANIFEST_FILE),
                    r#"{"kind":"migration"}"#,
                )
                .unwrap();
            }
        }
        fs::write(backups.join("migration-note.txt"), "keep").unwrap();

        prune_backups(&backups, MIGRATION_PREFIX, 2).unwrap();

        assert_eq!(
            names(&backups),
            [
                ".migration-20260501T000000Z-1.0.0-to-2.0.0.partial",
                "manual-20250101T000000Z",
                "migration-20260101T000000Z-1.0.0-to-2.0.0",
                "migration-20260201T000000Z-1.0.0-to-2.0.0",
                "migration-20260301T000000Z-1.0.0-to-2.0.0",
                "migration-20260401T000000Z-1.0.0-to-2.0.0",
                "migration-note.txt",
                "unrelated",
            ]
        );
    }

    #[test]
    fn pruning_only_counts_verified_backups_and_never_evicts_good_for_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "good original\n");
        let from = Version::new(1, 6, 1);
        let target = Version::new(2, 0, 0);
        let storage_path = paths.storage_path();
        let mut snapshots = Vec::new();
        for i in 0..5 {
            let backup = create_backup(&BackupRequest {
                paths: &paths,
                storage: StorageSource::File(&storage_path),
                kind: BackupKind::Migration {
                    from: Some(&from),
                    target: &target,
                },
                now: now() + time::Duration::seconds(i),
            })
            .unwrap();
            snapshots.push(backup);
        }
        assert!(snapshots.iter().all(BackupInfo::is_available));
        // The most recent directory is damaged. It must not take a slot from
        // the two newest intact snapshots, nor be removed automatically.
        fs::write(
            snapshots[4].path.join("config/profiles.yaml"),
            "tampered data\n",
        )
        .unwrap();
        assert!(!snapshots[4].is_available());
        let legacy = paths
            .backups_dir()
            .join("migration-20260101T000000Z-legacy");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("manifest.json"), r#"{"kind":"migration"}"#).unwrap();
        prune_backups(&paths.backups_dir(), MIGRATION_PREFIX, 2).unwrap();
        assert!(!snapshots[0].path.exists());
        assert!(!snapshots[1].path.exists());
        assert!(snapshots[2].is_available());
        assert!(snapshots[3].is_available());
        assert!(
            snapshots[4].path.exists(),
            "corrupt snapshots remain for manual inspection"
        );
        assert!(legacy.exists(), "older ref-only backups remain untouched");
    }

    #[test]
    fn prune_does_not_delete_a_foreign_directory_without_a_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("backups");
        let unrelated = root.join("migration-20200101T000000Z-not-a-backup");
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), b"private").unwrap();
        prune_backups(&root, MIGRATION_PREFIX, 0).unwrap();
        assert_eq!(fs::read(unrelated.join("keep")).unwrap(), b"private");
    }

    #[cfg(unix)]
    #[test]
    fn pruning_a_linked_directory_never_deletes_external_content() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let external = temp.path().join("keep-private");
        fs::create_dir_all(&external).unwrap();
        let backup = external.join("migration-20200101T000000Z-foreign");
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join(MANIFEST_FILE), r#"{"kind":"migration"}"#).unwrap();
        fs::write(backup.join("private"), b"keep").unwrap();
        let link = temp.path().join("backups");
        symlink(&external, &link).unwrap();
        assert!(matches!(
            prune_backups(&link, MIGRATION_PREFIX, 0),
            Err(BackupError::InvalidNode { .. })
        ));
        assert_eq!(fs::read(backup.join("private")).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_backups_root_is_rejected_without_touching_the_target() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(temp.path());
        write(&paths.profiles_path(), "user settings");
        let external = temp.path().join("other-location");
        fs::create_dir_all(&external).unwrap();
        let backups_dir = paths.backups_dir();
        fs::create_dir_all(backups_dir.parent().unwrap()).unwrap();
        symlink(&external, &backups_dir).unwrap();
        let storage = paths.storage_path();
        let error = create_backup(&manual(&paths, StorageSource::File(&storage)))
            .expect_err("backup must not follow a directory link");
        assert!(
            matches!(error, BackupError::InvalidNode { .. }),
            "{error:?}"
        );
        assert_eq!(fs::read_link(&backups_dir).unwrap(), external);
        assert!(names(&external).is_empty());
        assert_eq!(
            fs::read_to_string(paths.profiles_path()).unwrap(),
            "user settings"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_config_root_does_not_copy_an_external_tree() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(temp.path());
        let external = temp.path().join("private-config");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("personal.yaml"), b"private").unwrap();
        symlink(&external, paths.app_config_dir()).unwrap();
        let storage = paths.storage_path();
        let error = create_backup(&manual(&paths, StorageSource::File(&storage)))
            .expect_err("backup must reject a linked configuration root");
        assert!(
            matches!(error, BackupError::InvalidNode { .. }),
            "{error:?}"
        );
        assert!(names(&paths.backups_dir()).is_empty());
        assert_eq!(
            fs::read(external.join("personal.yaml")).unwrap(),
            b"private"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_storage_file_is_rejected_without_copying_external_data() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let paths = paths(temp.path());
        write(&paths.profiles_path(), "user settings");
        let external = temp.path().join("private-storage.redb");
        fs::write(&external, b"keep this private").unwrap();
        let source = paths.storage_path();
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        symlink(&external, &source).unwrap();
        let error = create_backup(&manual(&paths, StorageSource::File(&source))).unwrap_err();
        assert!(
            matches!(error, BackupError::InvalidNode { .. }),
            "{error:?}"
        );
        assert!(names(&paths.backups_dir()).is_empty());
        assert_eq!(fs::read_link(&source).unwrap(), external);
        assert_eq!(fs::read(&external).unwrap(), b"keep this private");
    }

    #[test]
    fn pruning_a_missing_dir_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();

        prune_backups(&dir.path().join("backups"), MANUAL_PREFIX, 3).unwrap();
    }

    #[test]
    fn creating_a_backup_does_not_remove_an_unowned_partial() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write(&paths.profiles_path(), "profiles");
        let leftover = paths.backups_dir().join(".manual-old.partial");
        fs::create_dir_all(&leftover).unwrap();

        let backup = create_backup(&manual(
            &paths,
            StorageSource::File(paths.storage_path().as_path()),
        ))
        .unwrap();

        assert_eq!(
            names(&paths.backups_dir()),
            [".manual-old.partial", &backup.name]
        );
    }
}

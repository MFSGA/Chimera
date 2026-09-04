use std::{io::Write, path::Path};

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::utils::dirs::{APP_VERSION, app_data_dir};

const UPDATE_JOURNAL_FILE: &str = "update-transaction.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppUpdatePhase {
    Checked,
    Downloaded,
    CleanupStarted,
    CleanupSucceeded,
    InstallerRequested,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppUpdateJournal {
    pub from_version: String,
    pub target_version: String,
    pub phase: AppUpdatePhase,
    pub updated_at: String,
}

fn normalize_version(version: &str) -> &str {
    version.trim_start_matches('v')
}

fn journal_path() -> Result<std::path::PathBuf> {
    Ok(app_data_dir()?.join(UPDATE_JOURNAL_FILE))
}

fn read_from(path: &Path) -> Result<Option<AppUpdateJournal>> {
    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs_err::read(path)
        .with_context(|| format!("failed to read update journal {}", path.display()))?;
    Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
        format!("failed to parse update journal {}", path.display())
    })?))
}

fn write_to(path: &Path, journal: &AppUpdateJournal) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(journal).context("failed to serialize update journal")?;
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(&bytes))
        .with_context(|| format!("failed to persist update journal {}", path.display()))?;
    Ok(())
}

fn record_at(path: &Path, target_version: &str, phase: AppUpdatePhase) -> Result<AppUpdateJournal> {
    let target_version = normalize_version(target_version).to_owned();
    let current = read_from(path)?;
    let from_version = current
        .filter(|journal| journal.target_version == target_version)
        .map(|journal| journal.from_version)
        .unwrap_or_else(|| normalize_version(APP_VERSION).to_owned());
    let journal = AppUpdateJournal {
        from_version,
        target_version,
        phase,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    write_to(path, &journal)?;
    Ok(journal)
}

pub fn record(target_version: &str, phase: AppUpdatePhase) -> Result<AppUpdateJournal> {
    record_at(&journal_path()?, target_version, phase)
}

fn reconcile_at(path: &Path, current_version: &str) -> Result<Option<AppUpdateJournal>> {
    let Some(mut journal) = read_from(path)? else {
        return Ok(None);
    };

    let current_version = normalize_version(current_version);
    if current_version == normalize_version(&journal.target_version) {
        if journal.phase != AppUpdatePhase::Completed {
            journal.phase = AppUpdatePhase::Completed;
            journal.updated_at = chrono::Utc::now().to_rfc3339();
            write_to(path, &journal)?;
        }
        log::info!(
            target: "app",
            "update transaction completed: {} -> {}",
            journal.from_version,
            journal.target_version
        );
    } else if journal.phase == AppUpdatePhase::InstallerRequested {
        log::warn!(
            target: "app",
            "previous installer was requested for {} -> {}, but the running version is still {}",
            journal.from_version,
            journal.target_version,
            current_version
        );
    } else {
        log::info!(
            target: "app",
            "incomplete update transaction remains at phase {:?}: {} -> {}",
            journal.phase,
            journal.from_version,
            journal.target_version
        );
    }

    Ok(Some(journal))
}

pub fn reconcile_startup() -> Result<Option<AppUpdateJournal>> {
    reconcile_at(&journal_path()?, APP_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_preserves_original_source_version_for_one_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPDATE_JOURNAL_FILE);

        let checked = record_at(&path, "v9.9.9", AppUpdatePhase::Checked).unwrap();
        let downloaded = record_at(&path, "9.9.9", AppUpdatePhase::Downloaded).unwrap();

        assert_eq!(checked.from_version, downloaded.from_version);
        assert_eq!(downloaded.target_version, "9.9.9");
        assert_eq!(downloaded.phase, AppUpdatePhase::Downloaded);
    }

    #[test]
    fn new_target_starts_a_new_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPDATE_JOURNAL_FILE);

        let first = record_at(&path, "9.9.9", AppUpdatePhase::Downloaded).unwrap();
        let second = record_at(&path, "10.0.0", AppUpdatePhase::Checked).unwrap();

        assert_eq!(first.from_version, second.from_version);
        assert_eq!(second.target_version, "10.0.0");
        assert_eq!(second.phase, AppUpdatePhase::Checked);
    }

    #[test]
    fn installer_requested_remains_incomplete_when_version_did_not_advance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPDATE_JOURNAL_FILE);
        record_at(&path, "9.9.9", AppUpdatePhase::InstallerRequested).unwrap();

        let journal = reconcile_at(&path, "1.2.3").unwrap().unwrap();

        assert_eq!(journal.phase, AppUpdatePhase::InstallerRequested);
        assert_eq!(read_from(&path).unwrap().unwrap(), journal);
    }

    #[test]
    fn startup_marks_transaction_completed_after_version_advances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPDATE_JOURNAL_FILE);
        record_at(&path, "v9.9.9", AppUpdatePhase::InstallerRequested).unwrap();

        let journal = reconcile_at(&path, "9.9.9").unwrap().unwrap();

        assert_eq!(journal.phase, AppUpdatePhase::Completed);
        assert_eq!(
            read_from(&path).unwrap().unwrap().phase,
            AppUpdatePhase::Completed
        );
    }
}

use std::{
    collections::HashSet,
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};

use crate::config::profile::item::shared::profile_reservation_marker;

const MAX_RESERVATION_MARKER_BYTES: u64 = 256;
pub(crate) const STALE_RESERVATION_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReservationDegradation {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReservationReconcileReport {
    pub removed_reservations: usize,
    pub removed_materializations: usize,
    pub retained: usize,
    pub degradations: Vec<ReservationDegradation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Missing,
    RegularFile,
    Unsafe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservationPlan {
    RemoveReservation,
    RemoveTargetThenReservation,
    RetainFresh,
    Degrade(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReservationFacts {
    committed: bool,
    target: EntryKind,
    stale: Option<bool>,
}

trait ReservationFs {
    fn list(&self, root: &Path) -> Result<Vec<PathBuf>>;
    fn inspect(&self, path: &Path) -> Result<EntryKind>;
    fn read_marker(&self, path: &Path) -> Result<Vec<u8>>;
    fn modified(&self, path: &Path) -> Result<SystemTime>;
    fn remove_file(&self, path: &Path) -> Result<()>;
}

struct StdReservationFs;

impl ReservationFs for StdReservationFs {
    fn list(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let metadata = std::fs::symlink_metadata(root)
            .with_context(|| format!("inspect profiles root {}", root.display()))?;
        if is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
            bail!("profiles root is not a real directory: {}", root.display());
        }
        std::fs::read_dir(root)
            .with_context(|| format!("read profiles root {}", root.display()))?
            .map(|entry| entry.map(|entry| entry.path()).map_err(anyhow::Error::from))
            .collect()
    }

    fn inspect(&self, path: &Path) -> Result<EntryKind> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                    Ok(EntryKind::Unsafe)
                } else {
                    Ok(EntryKind::RegularFile)
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(EntryKind::Missing),
            Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
        }
    }

    fn read_marker(&self, path: &Path) -> Result<Vec<u8>> {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("inspect reservation marker {}", path.display()))?;
        if metadata.len() > MAX_RESERVATION_MARKER_BYTES {
            bail!("reservation marker exceeds size limit");
        }
        let file = std::fs::File::open(path)
            .with_context(|| format!("open reservation marker {}", path.display()))?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_RESERVATION_MARKER_BYTES + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("read reservation marker {}", path.display()))?;
        if bytes.len() as u64 > MAX_RESERVATION_MARKER_BYTES {
            bail!("reservation marker exceeds size limit");
        }
        Ok(bytes)
    }

    fn modified(&self, path: &Path) -> Result<SystemTime> {
        std::fs::symlink_metadata(path)
            .with_context(|| format!("inspect {}", path.display()))?
            .modified()
            .with_context(|| format!("read modified time for {}", path.display()))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
    }
}

pub(crate) fn reconcile_reservations(
    root: &Path,
    committed_files: &HashSet<String>,
) -> Result<ReservationReconcileReport> {
    reconcile_with_fs(
        &StdReservationFs,
        root,
        committed_files,
        SystemTime::now(),
        STALE_RESERVATION_AGE,
    )
}

fn reconcile_with_fs<F: ReservationFs>(
    fs: &F,
    root: &Path,
    committed_files: &HashSet<String>,
    now: SystemTime,
    stale_after: Duration,
) -> Result<ReservationReconcileReport> {
    let mut report = ReservationReconcileReport::default();
    for reservation in fs.list(root)? {
        let Some(name) = reservation.file_name() else {
            continue;
        };
        if !is_reservation_candidate(name) {
            continue;
        }
        let Some(target_name) = parse_canonical_reservation(name) else {
            push_degradation(&mut report, reservation, "invalid reservation name");
            continue;
        };

        match fs.inspect(&reservation) {
            Ok(EntryKind::RegularFile) => {}
            Ok(_) => {
                push_degradation(&mut report, reservation, "reservation is not a real file");
                continue;
            }
            Err(error) => {
                push_degradation(&mut report, reservation, error.to_string());
                continue;
            }
        }

        let expected_marker = profile_reservation_marker(&target_name);
        match fs.read_marker(&reservation) {
            Ok(marker) if marker == expected_marker.as_bytes() => {}
            Ok(_) => {
                push_degradation(
                    &mut report,
                    reservation,
                    "reservation ownership marker is invalid",
                );
                continue;
            }
            Err(error) => {
                push_degradation(&mut report, reservation, error.to_string());
                continue;
            }
        }

        let target = root.join(&target_name);
        let target_kind = match fs.inspect(&target) {
            Ok(kind) => kind,
            Err(error) => {
                push_degradation(&mut report, reservation, error.to_string());
                continue;
            }
        };
        let stale = fs
            .modified(&reservation)
            .ok()
            .map(|modified| now.duration_since(modified).unwrap_or_default() >= stale_after);
        let plan = plan_reservation(ReservationFacts {
            committed: committed_files
                .iter()
                .any(|file| file.eq_ignore_ascii_case(&target_name)),
            target: target_kind,
            stale,
        });

        match plan {
            ReservationPlan::RemoveReservation => {
                remove_reservation(fs, &reservation, &mut report);
            }
            ReservationPlan::RemoveTargetThenReservation => {
                if let Err(error) = fs.remove_file(&target) {
                    push_degradation(&mut report, reservation, error.to_string());
                    continue;
                }
                report.removed_materializations += 1;
                remove_reservation(fs, &reservation, &mut report);
            }
            ReservationPlan::RetainFresh => report.retained += 1,
            ReservationPlan::Degrade(reason) => push_degradation(&mut report, reservation, reason),
        }
    }
    Ok(report)
}

fn plan_reservation(facts: ReservationFacts) -> ReservationPlan {
    if facts.target == EntryKind::Unsafe {
        return ReservationPlan::Degrade("profile target is not a real file");
    }
    if facts.committed {
        return match facts.target {
            EntryKind::RegularFile => ReservationPlan::RemoveReservation,
            EntryKind::Missing => ReservationPlan::Degrade("committed profile target is missing"),
            EntryKind::Unsafe => unreachable!(),
        };
    }
    match facts.stale {
        None => ReservationPlan::Degrade("reservation age is unavailable"),
        Some(false) => ReservationPlan::RetainFresh,
        Some(true) => match facts.target {
            EntryKind::Missing => ReservationPlan::RemoveReservation,
            EntryKind::RegularFile => ReservationPlan::RemoveTargetThenReservation,
            EntryKind::Unsafe => unreachable!(),
        },
    }
}

fn remove_reservation<F: ReservationFs>(
    fs: &F,
    reservation: &Path,
    report: &mut ReservationReconcileReport,
) {
    match fs.remove_file(reservation) {
        Ok(()) => report.removed_reservations += 1,
        Err(error) => push_degradation(report, reservation.to_path_buf(), error.to_string()),
    }
}

fn push_degradation(
    report: &mut ReservationReconcileReport,
    path: PathBuf,
    reason: impl Into<String>,
) {
    report.degradations.push(ReservationDegradation {
        path,
        reason: reason.into(),
    });
}

fn is_reservation_candidate(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') && name.ends_with(".reserve")
}

fn parse_canonical_reservation(name: &OsStr) -> Option<String> {
    let name = name.to_str()?;
    let uid = name.strip_prefix('.')?.strip_suffix(".yaml.reserve")?;
    let mut chars = uid.chars();
    let prefix = chars.next()?;
    if !matches!(prefix, 'l' | 'r') || chars.clone().count() != 11 {
        return None;
    }
    if !chars.all(|character| character.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!("{uid}.yaml"))
}

#[cfg(windows)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
#[path = "reservation_reconcile_tests.rs"]
mod tests;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Result, bail};

use super::*;

#[derive(Default)]
struct FakeFs {
    entries: RefCell<HashSet<PathBuf>>,
    kinds: RefCell<HashMap<PathBuf, EntryKind>>,
    modified: RefCell<HashMap<PathBuf, SystemTime>>,
    markers: RefCell<HashMap<PathBuf, Vec<u8>>>,
    remove_failures: RefCell<HashSet<PathBuf>>,
    removed: RefCell<Vec<PathBuf>>,
}

impl FakeFs {
    fn add(&self, path: impl Into<PathBuf>, kind: EntryKind, modified: SystemTime) {
        let path = path.into();
        self.entries.borrow_mut().insert(path.clone());
        self.kinds.borrow_mut().insert(path.clone(), kind);
        self.modified.borrow_mut().insert(path.clone(), modified);
        if kind == EntryKind::RegularFile
            && let Some(target_name) = path.file_name().and_then(parse_canonical_reservation)
        {
            self.markers
                .borrow_mut()
                .insert(path, profile_reservation_marker(&target_name).into_bytes());
        }
    }

    fn fail_remove(&self, path: impl Into<PathBuf>) {
        self.remove_failures.borrow_mut().insert(path.into());
    }

    fn removed(&self) -> Vec<PathBuf> {
        self.removed.borrow().clone()
    }
}

impl ReservationFs for FakeFs {
    fn list(&self, _root: &Path) -> Result<Vec<PathBuf>> {
        let kinds = self.kinds.borrow();
        let mut entries = self
            .entries
            .borrow()
            .iter()
            .filter(|path| kinds.get(*path) != Some(&EntryKind::Missing))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort();
        Ok(entries)
    }

    fn inspect(&self, path: &Path) -> Result<EntryKind> {
        Ok(self
            .kinds
            .borrow()
            .get(path)
            .copied()
            .unwrap_or(EntryKind::Missing))
    }

    fn read_marker(&self, path: &Path) -> Result<Vec<u8>> {
        self.markers
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing reservation marker"))
    }

    fn modified(&self, path: &Path) -> Result<SystemTime> {
        self.modified
            .borrow()
            .get(path)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("missing modified time"))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        if self.remove_failures.borrow().contains(path) {
            bail!("injected remove failure for {}", path.display());
        }
        self.kinds
            .borrow_mut()
            .insert(path.to_path_buf(), EntryKind::Missing);
        self.removed.borrow_mut().push(path.to_path_buf());
        Ok(())
    }
}

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(100_000)
}

fn stale_time() -> SystemTime {
    now() - Duration::from_secs(10_000)
}

fn fresh_time() -> SystemTime {
    now() - Duration::from_secs(10)
}

fn reservation(root: &Path, uid: &str) -> PathBuf {
    root.join(format!(".{uid}.yaml.reserve"))
}

fn target(root: &Path, uid: &str) -> PathBuf {
    root.join(format!("{uid}.yaml"))
}

#[test]
fn pure_plan_is_fail_closed_and_phase_aware() {
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: true,
            target: EntryKind::RegularFile,
            stale: Some(false),
        }),
        ReservationPlan::RemoveReservation
    );
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: true,
            target: EntryKind::Missing,
            stale: Some(true),
        }),
        ReservationPlan::Degrade("committed profile target is missing")
    );
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: false,
            target: EntryKind::RegularFile,
            stale: Some(true),
        }),
        ReservationPlan::RemoveTargetThenReservation
    );
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: false,
            target: EntryKind::Missing,
            stale: Some(true),
        }),
        ReservationPlan::RemoveReservation
    );
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: false,
            target: EntryKind::RegularFile,
            stale: Some(false),
        }),
        ReservationPlan::RetainFresh
    );
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: false,
            target: EntryKind::Missing,
            stale: None,
        }),
        ReservationPlan::Degrade("reservation age is unavailable")
    );
    assert_eq!(
        plan_reservation(ReservationFacts {
            committed: false,
            target: EntryKind::Unsafe,
            stale: Some(true),
        }),
        ReservationPlan::Degrade("profile target is not a real file")
    );
}

#[test]
fn committed_target_releases_only_the_reservation() {
    let root = PathBuf::from("profiles");
    let uid = "l12345678901";
    let reservation = reservation(&root, uid);
    let target = target(&root, uid);
    let fs = FakeFs::default();
    fs.add(&reservation, EntryKind::RegularFile, fresh_time());
    fs.add(&target, EntryKind::RegularFile, fresh_time());
    let committed = HashSet::from([format!("{uid}.yaml")]);

    let report =
        reconcile_with_fs(&fs, &root, &committed, now(), Duration::from_secs(100)).unwrap();

    assert_eq!(report.removed_reservations, 1);
    assert_eq!(report.removed_materializations, 0);
    assert!(report.degradations.is_empty());
    assert_eq!(fs.removed(), vec![reservation]);
}

#[test]
fn stale_uncommitted_materialization_is_removed_idempotently() {
    let root = PathBuf::from("profiles");
    let uid = "r12345678901";
    let reservation = reservation(&root, uid);
    let target = target(&root, uid);
    let fs = FakeFs::default();
    fs.add(&reservation, EntryKind::RegularFile, stale_time());
    fs.add(&target, EntryKind::RegularFile, stale_time());

    let first =
        reconcile_with_fs(&fs, &root, &HashSet::new(), now(), Duration::from_secs(100)).unwrap();
    let second =
        reconcile_with_fs(&fs, &root, &HashSet::new(), now(), Duration::from_secs(100)).unwrap();

    assert_eq!(first.removed_materializations, 1);
    assert_eq!(first.removed_reservations, 1);
    assert_eq!(fs.removed(), vec![target, reservation]);
    assert_eq!(second, ReservationReconcileReport::default());
}

#[test]
fn fresh_invalid_and_unsafe_entries_are_never_deleted() {
    let root = PathBuf::from("profiles");
    let fresh = reservation(&root, "l12345678901");
    let invalid = root.join(".client-name.yaml.reserve");
    let unsafe_reservation = reservation(&root, "r12345678901");
    let unsafe_target_reservation = reservation(&root, "lABCDEFGHIJK");
    let unsafe_target = target(&root, "lABCDEFGHIJK");
    let fs = FakeFs::default();
    fs.add(&fresh, EntryKind::RegularFile, fresh_time());
    fs.add(&invalid, EntryKind::RegularFile, stale_time());
    fs.add(&unsafe_reservation, EntryKind::Unsafe, stale_time());
    fs.add(
        &unsafe_target_reservation,
        EntryKind::RegularFile,
        stale_time(),
    );
    fs.add(&unsafe_target, EntryKind::Unsafe, stale_time());

    let report =
        reconcile_with_fs(&fs, &root, &HashSet::new(), now(), Duration::from_secs(100)).unwrap();

    assert_eq!(report.retained, 1);
    assert_eq!(report.degradations.len(), 3);
    assert!(fs.removed().is_empty());
}

#[test]
fn target_remove_failure_keeps_reservation_for_retry() {
    let root = PathBuf::from("profiles");
    let uid = "l12345678901";
    let reservation = reservation(&root, uid);
    let target = target(&root, uid);
    let fs = FakeFs::default();
    fs.add(&reservation, EntryKind::RegularFile, stale_time());
    fs.add(&target, EntryKind::RegularFile, stale_time());
    fs.fail_remove(&target);

    let report =
        reconcile_with_fs(&fs, &root, &HashSet::new(), now(), Duration::from_secs(100)).unwrap();

    assert_eq!(report.removed_materializations, 0);
    assert_eq!(report.removed_reservations, 0);
    assert_eq!(report.degradations.len(), 1);
    assert!(
        report.degradations[0]
            .reason
            .contains("injected remove failure")
    );
    assert!(fs.removed().is_empty());
}

#[test]
fn committed_profile_with_missing_target_is_reported_without_cleanup() {
    let root = PathBuf::from("profiles");
    let uid = "r12345678901";
    let reservation = reservation(&root, uid);
    let fs = FakeFs::default();
    fs.add(&reservation, EntryKind::RegularFile, stale_time());
    let committed = HashSet::from([format!("{uid}.yaml")]);

    let report =
        reconcile_with_fs(&fs, &root, &committed, now(), Duration::from_secs(100)).unwrap();

    assert_eq!(report.degradations.len(), 1);
    assert_eq!(
        report.degradations[0].reason,
        "committed profile target is missing"
    );
    assert!(fs.removed().is_empty());
}

#[test]
fn std_filesystem_releases_committed_reservation_without_touching_target() {
    let dir = tempfile::tempdir().unwrap();
    let uid = "l12345678901";
    let reservation = reservation(dir.path(), uid);
    let target = target(dir.path(), uid);
    std::fs::write(
        &reservation,
        profile_reservation_marker(&format!("{uid}.yaml")),
    )
    .unwrap();
    std::fs::write(&target, b"mode: rule\n").unwrap();
    let committed = HashSet::from([format!("{}.YAML", uid.to_ascii_uppercase())]);

    let first = reconcile_reservations(dir.path(), &committed).unwrap();
    let second = reconcile_reservations(dir.path(), &committed).unwrap();

    assert_eq!(first.removed_reservations, 1);
    assert_eq!(first.removed_materializations, 0);
    assert!(first.degradations.is_empty());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "mode: rule\n");
    assert!(!reservation.exists());
    assert_eq!(second, ReservationReconcileReport::default());
}

#[test]
fn std_filesystem_rejects_forged_and_oversized_canonical_markers() {
    let dir = tempfile::tempdir().unwrap();
    let forged_uid = "r12345678901";
    let forged_reservation = reservation(dir.path(), forged_uid);
    let forged_target = target(dir.path(), forged_uid);
    std::fs::write(&forged_reservation, b"").unwrap();
    std::fs::write(&forged_target, b"mode: global\n").unwrap();

    let oversized_uid = "lABCDEFGHIJK";
    let oversized_reservation = reservation(dir.path(), oversized_uid);
    let oversized_target = target(dir.path(), oversized_uid);
    std::fs::write(
        &oversized_reservation,
        vec![b'x'; MAX_RESERVATION_MARKER_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::write(&oversized_target, b"mode: direct\n").unwrap();

    let report = reconcile_reservations(dir.path(), &HashSet::new()).unwrap();

    assert_eq!(report.degradations.len(), 2);
    assert!(forged_reservation.exists());
    assert_eq!(
        std::fs::read_to_string(forged_target).unwrap(),
        "mode: global\n"
    );
    assert!(oversized_reservation.exists());
    assert_eq!(
        std::fs::read_to_string(oversized_target).unwrap(),
        "mode: direct\n"
    );
    assert!(report.degradations.iter().any(|degradation| {
        degradation
            .reason
            .contains("reservation ownership marker is invalid")
    }));
    assert!(
        report
            .degradations
            .iter()
            .any(|degradation| degradation.reason.contains("marker exceeds size limit"))
    );
}

#[test]
fn std_filesystem_retains_invalid_reservation_and_unrelated_files() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = dir.path().join(".client-name.yaml.reserve");
    let unrelated = dir.path().join("notes.txt");
    std::fs::write(&invalid, b"").unwrap();
    std::fs::write(&unrelated, b"keep me").unwrap();

    let report = reconcile_reservations(dir.path(), &HashSet::new()).unwrap();

    assert_eq!(report.degradations.len(), 1);
    assert_eq!(report.degradations[0].path, invalid);
    assert!(invalid.exists());
    assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "keep me");
}

#[cfg(windows)]
#[test]
fn std_filesystem_rejects_a_junction_root_without_touching_contents() {
    let dir = tempfile::tempdir().unwrap();
    let target_root = dir.path().join("target");
    let junction_root = dir.path().join("profiles");
    std::fs::create_dir(&target_root).unwrap();
    let uid = "l12345678901";
    let reservation = reservation(&target_root, uid);
    let target = target(&target_root, uid);
    std::fs::write(
        &reservation,
        profile_reservation_marker(&format!("{uid}.yaml")),
    )
    .unwrap();
    std::fs::write(&target, b"mode: rule\n").unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction_root)
        .arg(&target_root)
        .status()
        .unwrap();
    assert!(status.success());

    let error = reconcile_reservations(&junction_root, &HashSet::new()).unwrap_err();

    assert!(error.to_string().contains("not a real directory"));
    assert!(reservation.exists());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "mode: rule\n");
}

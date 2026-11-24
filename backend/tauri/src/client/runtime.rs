use std::{
    fs::OpenOptions,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ChimeraClient;

// Upstream Runtime Values own these wire-stable values. The ref's four
// runtime states and five phases are supplemented by Chimera's explicit
// pending/recovery and multi-stage commit statuses in the shared crate.
pub use chimera_contracts::runtime::{CommitReceipt, DegradationPhase, RuntimeCommitStatus};

use crate::{client::utf8_path, utils::path::PathResolver};

pub use chimera_core::client::mutation_outcome::{Degradation, MutationOutcome};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG_FILE: &str = "clash-config.yaml";
/// Ref-compatible name for the generated runtime product.
pub const RUNTIME_CONFIG: &str = RUNTIME_CONFIG_FILE;
const CANDIDATE_DIR: &str = ".candidates";

// The shared Core owns the exact runtime receipt, allocator and snapshot store.
pub use chimera_core::client::runtime::{
    RuntimeApplyReceipt, RuntimeLifecycle, RuntimeRebuildGate, RuntimeRevision,
    RuntimeRevisionAllocator, RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransactionSnapshot,
    RuntimeTransformFailure,
};

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    product: Utf8PathBuf,
    candidate_dir: Utf8PathBuf,
}

impl RuntimePaths {
    pub fn from_resolver(paths: &PathResolver) -> anyhow::Result<Self> {
        let runtime_dir = utf8_path(paths.app_config_dir().join(RUNTIME_CONFIG_DIR))?;
        Ok(Self {
            product: runtime_dir.join(RUNTIME_CONFIG),
            candidate_dir: runtime_dir.join(".candidates"),
        })
    }

    #[allow(dead_code)]
    pub fn new(product: Utf8PathBuf, candidate_dir: Utf8PathBuf) -> Self {
        Self {
            product,
            candidate_dir,
        }
    }

    #[cfg(test)]
    pub(super) fn from_config_root(config_root: PathBuf) -> Self {
        let runtime_dir = config_root.join(RUNTIME_CONFIG_DIR);
        Self {
            product: utf8_path(runtime_dir.join(RUNTIME_CONFIG))
                .expect("test runtime path is UTF-8"),
            candidate_dir: utf8_path(runtime_dir.join(".candidates"))
                .expect("test runtime path is UTF-8"),
        }
    }

    pub fn product(&self) -> &Utf8Path {
        &self.product
    }

    #[allow(dead_code)]
    pub fn candidate_dir(&self) -> &Utf8Path {
        &self.candidate_dir
    }

    pub async fn create_candidate(&self, bytes: &[u8]) -> anyhow::Result<CandidateFile> {
        let names = (0..16)
            .map(|_| nanoid::nanoid!(16, &nanoid::alphabet::SAFE))
            .collect();
        self.create_candidate_with_names(bytes, names).await
    }

    async fn create_candidate_with_names(
        &self,
        bytes: &[u8],
        names: Vec<String>,
    ) -> anyhow::Result<CandidateFile> {
        prepare_private_dir(&self.candidate_dir).await?;
        let candidate_dir = self.candidate_dir.clone();
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            for name in names {
                let path = candidate_dir.join(format!("candidate-{name}.yaml"));
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&path) {
                    Ok(mut file) => {
                        file.write_all(&bytes)?;
                        file.sync_all()?;
                        let bytes_sha256 = Sha256::digest(&bytes).into();
                        return Ok(CandidateFile {
                            path: path.into(),
                            bytes_sha256,
                            cleaned: false,
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            anyhow::bail!("failed to allocate a unique runtime candidate after 16 attempts")
        })
        .await?
    }

    pub async fn cleanup_stale_candidates(&self, max_age: Duration) -> anyhow::Result<usize> {
        prepare_private_dir(&self.candidate_dir).await?;
        let now = SystemTime::now();
        let mut removed = 0;
        let mut entries = tokio::fs::read_dir(&self.candidate_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("candidate-") {
                continue;
            }
            let metadata = tokio::fs::symlink_metadata(entry.path()).await?;
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                continue;
            }
            let is_stale = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= max_age);
            if is_stale {
                tokio::fs::remove_file(entry.path()).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[derive(Debug)]
pub struct CandidateFile {
    path: PathBuf,
    bytes_sha256: [u8; 32],
    cleaned: bool,
}

impl CandidateFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes_sha256(&self) -> [u8; 32] {
        self.bytes_sha256
    }

    pub async fn read_verified(&self) -> Result<Vec<u8>> {
        let bytes = tokio::fs::read(&self.path)
            .await
            .with_context(|| format!("read runtime candidate {}", self.path.display()))?;
        let actual: [u8; 32] = Sha256::digest(&bytes).into();
        if actual != self.bytes_sha256 {
            bail!("runtime candidate changed after creation");
        }
        Ok(bytes)
    }

    pub async fn cleanup(mut self) -> Result<()> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => {
                self.cleaned = true;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cleaned = true;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for CandidateFile {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub async fn promote_candidate(
    candidate: &CandidateFile,
    product: impl AsRef<Path>,
) -> Result<Vec<u8>> {
    let product = product.as_ref();
    let bytes = candidate.read_verified().await?;
    restore_product(product, &bytes).await?;
    let promoted = tokio::fs::read(product).await?;
    let promoted_hash: [u8; 32] = Sha256::digest(&promoted).into();
    if promoted_hash != candidate.bytes_sha256() {
        bail!("promoted runtime product hash does not match candidate");
    }
    Ok(promoted)
}

#[derive(Debug, thiserror::Error)]
pub enum CheckedPromotionError {
    #[error("runtime candidate check failed: {0}")]
    Check(#[source] anyhow::Error),
    #[error("runtime candidate changed after check: {0}")]
    Verify(#[source] anyhow::Error),
    #[error("failed to promote checked runtime candidate: {0}")]
    Promote(#[source] anyhow::Error),
}

pub async fn check_and_promote_candidate<F, Fut, P>(
    candidate: &CandidateFile,
    product: P,
    check: F,
) -> std::result::Result<Vec<u8>, CheckedPromotionError>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<()>>,
    P: AsRef<Path>,
{
    check(candidate.path().to_path_buf())
        .await
        .map_err(CheckedPromotionError::Check)?;
    candidate
        .read_verified()
        .await
        .map_err(CheckedPromotionError::Verify)?;
    promote_candidate(candidate, product)
        .await
        .map_err(CheckedPromotionError::Promote)
}

pub async fn restore_product(product: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    let product = product.as_ref().to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = product.parent() {
            std::fs::create_dir_all(parent)?;
        }
        AtomicFile::new(&product, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(&bytes))
            .map_err(|error| {
                anyhow::anyhow!("failed to atomically replace runtime product: {error}")
            })?;
        Ok(())
    })
    .await?
}

pub async fn restore_optional_product(product: &Path, bytes: Option<&[u8]>) -> Result<()> {
    match bytes {
        Some(bytes) => restore_product(product, bytes).await,
        None => match tokio::fs::remove_file(product).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

pub async fn capture_runtime_transaction(
    paths: &RuntimePaths,
    lifecycle: &RuntimeLifecycle,
) -> Result<RuntimeTransactionSnapshot> {
    let product = match tokio::fs::read(paths.product()).await {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    Ok(RuntimeTransactionSnapshot {
        product,
        lifecycle: lifecycle.snapshot(),
    })
}

pub async fn restore_failed_apply<F, Fut>(
    paths: &RuntimePaths,
    lifecycle: &RuntimeLifecycle,
    snapshot: RuntimeTransactionSnapshot,
    recover: F,
) -> Result<()>
where
    F: FnOnce(bool) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let RuntimeTransactionSnapshot {
        product,
        lifecycle: mut previous_lifecycle,
    } = snapshot;
    let current_transform_failure = lifecycle.snapshot().last_transform_failure;
    previous_lifecycle.last_transform_failure = current_transform_failure;
    let had_product = product.is_some();
    restore_optional_product(paths.product.as_std_path(), product.as_deref()).await?;

    match recover(had_product).await {
        Ok(()) => {
            lifecycle.restore(previous_lifecycle);
            Ok(())
        }
        Err(error) => {
            let mut degraded = previous_lifecycle;
            degraded.applied = None;
            degraded.confirmed_available = false;
            lifecycle.restore(degraded);
            Err(error)
        }
    }
}

async fn prepare_private_dir(path: &Utf8Path) -> anyhow::Result<()> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if is_symlink_or_reparse(&metadata) => {
            anyhow::bail!("runtime candidate directory is a symlink or reparse point: {path}");
        }
        Ok(metadata) if !metadata.is_dir() => {
            anyhow::bail!("runtime candidate path is not a directory: {path}");
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    tokio::fs::create_dir_all(path).await?;
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
        anyhow::bail!("runtime candidate path is not a private directory: {path}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

fn ensure_real_directory(path: &Path, metadata: &std::fs::Metadata) -> Result<()> {
    if is_symlink_or_reparse(metadata) {
        bail!(
            "runtime directory is a symlink or reparse point: {}",
            path.display()
        );
    }
    if !metadata.is_dir() {
        bail!("runtime path is not a directory: {}", path.display());
    }
    Ok(())
}

#[cfg(windows)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{chimera::ClashCore, profile::item_type::ScriptType},
        enhance::PostProcessingOutput,
    };
    use serde_yaml::Mapping;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    fn paths(dir: &tempfile::TempDir) -> RuntimePaths {
        RuntimePaths::from_config_root(dir.path().to_path_buf())
    }

    fn transform_output(message: &str) -> PostProcessingOutput {
        serde_json::from_value(serde_json::json!({
            "scopes": {},
            "global": {
                "s-test": [["info", message]],
            },
        }))
        .unwrap()
    }

    #[test]
    fn runtime_revision_allocator_is_monotonic() {
        let allocator = RuntimeRevisionAllocator::default();
        assert_eq!(allocator.allocate().unwrap().get(), 1);
        assert_eq!(allocator.allocate().unwrap().get(), 2);
        assert_eq!(allocator.allocate().unwrap().get(), 3);
        let same_session = allocator.clone();
        assert_eq!(same_session.allocate().unwrap().get(), 4);
        assert_eq!(allocator.allocate().unwrap().get(), 5);
    }

    #[test]
    fn successful_apply_clears_the_last_transform_failure() {
        let lifecycle = RuntimeLifecycle::default();
        let failed_revision = lifecycle.allocate_revision().unwrap();
        lifecycle.publish_transform_failure(RuntimeTransformFailure {
            attempt_revision: failed_revision,
            transform_uid: "sj-failed".into(),
            scope_uid: Some("source-test".into()),
            script_type: Some(ScriptType::JavaScript),
            message: "script exploded".into(),
        });
        assert_eq!(
            lifecycle
                .snapshot()
                .last_transform_failure
                .as_ref()
                .map(|failure| failure.attempt_revision),
            Some(failed_revision)
        );

        let applied = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
        ));
        lifecycle.publish_promoted(applied.clone());
        lifecycle.publish_applied(applied).unwrap();

        assert!(lifecycle.snapshot().last_transform_failure.is_none());
    }

    #[tokio::test]
    async fn candidate_is_hashed_private_and_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let candidate = paths.create_candidate(b"mode: rule\n").await.unwrap();
        let candidate_path = candidate.path().to_path_buf();
        assert_eq!(std::fs::read(&candidate_path).unwrap(), b"mode: rule\n");
        assert_eq!(
            candidate.bytes_sha256(),
            <[u8; 32]>::from(Sha256::digest(b"mode: rule\n"))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(paths.candidate_dir())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        drop(candidate);
        assert!(!candidate_path.exists());
    }

    #[tokio::test]
    async fn candidate_collision_retries_with_exclusive_create() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        prepare_private_dir(paths.candidate_dir()).await.unwrap();
        std::fs::write(
            paths.candidate_dir().join("candidate-taken.yaml"),
            b"occupied",
        )
        .unwrap();
        let candidate = paths
            .create_candidate_with_names(
                b"mode: direct\n",
                vec!["taken".to_string(), "available".to_string()],
            )
            .await
            .unwrap();
        assert!(candidate.path().ends_with("candidate-available.yaml"));
    }

    #[tokio::test]
    async fn candidate_directory_rejects_non_directory() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        std::fs::create_dir_all(paths.candidate_dir().parent().unwrap()).unwrap();
        std::fs::write(paths.candidate_dir(), b"not a directory").unwrap();
        let error = paths.create_candidate(b"mode: rule\n").await.unwrap_err();
        assert!(error.to_string().contains("not a directory"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn candidate_directory_rejects_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let target = dir.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(paths.candidate_dir().parent().unwrap()).unwrap();
        symlink(&target, paths.candidate_dir()).unwrap();
        let error = paths.create_candidate(b"mode: rule\n").await.unwrap_err();
        assert!(error.to_string().contains("symlink or reparse point"));
    }

    #[tokio::test]
    async fn promote_uses_exact_candidate_bytes_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let candidate = paths.create_candidate(b"mode: direct\n").await.unwrap();
        let promoted = promote_candidate(&candidate, paths.product())
            .await
            .unwrap();

        assert_eq!(promoted, b"mode: direct\n");
        assert_eq!(std::fs::read(paths.product()).unwrap(), promoted);
    }

    #[tokio::test]
    async fn check_failure_does_not_replace_product() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        restore_product(paths.product(), b"mode: rule\n")
            .await
            .unwrap();
        let candidate = paths.create_candidate(b"mode: direct\n").await.unwrap();

        let error = check_and_promote_candidate(&candidate, paths.product(), |_| async {
            bail!("candidate rejected")
        })
        .await
        .unwrap_err();

        assert!(matches!(error, CheckedPromotionError::Check(_)));
        assert_eq!(std::fs::read(paths.product()).unwrap(), b"mode: rule\n");
    }

    #[tokio::test]
    async fn candidate_changed_by_check_is_rejected_without_replacing_product() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        restore_product(paths.product(), b"mode: rule\n")
            .await
            .unwrap();
        let candidate = paths.create_candidate(b"mode: direct\n").await.unwrap();

        let error = check_and_promote_candidate(&candidate, paths.product(), |path| async move {
            std::fs::write(path, b"mode: global\n")?;
            Ok(())
        })
        .await
        .unwrap_err();

        assert!(matches!(error, CheckedPromotionError::Verify(_)));
        assert_eq!(std::fs::read(paths.product()).unwrap(), b"mode: rule\n");
    }

    #[tokio::test]
    async fn candidate_tamper_is_rejected_without_replacing_product() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        restore_product(paths.product(), b"mode: rule\n")
            .await
            .unwrap();
        let candidate = paths.create_candidate(b"mode: direct\n").await.unwrap();
        std::fs::write(candidate.path(), b"mode: global\n").unwrap();
        let error = promote_candidate(&candidate, paths.product())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("changed after creation"));
        assert_eq!(std::fs::read(paths.product()).unwrap(), b"mode: rule\n");
    }

    #[tokio::test]
    async fn stale_cleanup_removes_only_old_candidate_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        prepare_private_dir(paths.candidate_dir()).await.unwrap();
        let stale = paths.candidate_dir().join("candidate-stale.yaml");
        let unrelated = paths.candidate_dir().join("keep.txt");
        std::fs::write(&stale, b"stale").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();
        let removed = paths
            .cleanup_stale_candidates(Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert!(!stale.exists());
        assert!(unrelated.exists());
    }

    #[tokio::test]
    async fn transaction_restore_keeps_typed_config_out_of_runtime_product() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let typed_config_path = dir.path().join(RUNTIME_CONFIG);
        let typed_config =
            serde_yaml::to_string(&chimera_config::clash::config::ClashConfig::default()).unwrap();
        std::fs::write(&typed_config_path, &typed_config).unwrap();
        let lifecycle = RuntimeLifecycle::default();
        let transaction = capture_runtime_transaction(&paths, &lifecycle)
            .await
            .unwrap();
        assert!(transaction.product.is_none());

        restore_product(paths.product(), b"failed runtime candidate\n")
            .await
            .unwrap();
        restore_failed_apply(&paths, &lifecycle, transaction, |had_product| async move {
            assert!(!had_product);
            Ok(())
        })
        .await
        .unwrap();

        assert!(!paths.product().exists());
        assert_eq!(
            std::fs::read_to_string(typed_config_path).unwrap(),
            typed_config
        );
    }

    #[tokio::test]
    async fn transaction_restore_recovers_product_and_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let lifecycle = RuntimeLifecycle::default();
        restore_product(paths.product(), b"mode: rule\n")
            .await
            .unwrap();
        let old_output = transform_output("old runtime");
        let old = Arc::new(RuntimeSnapshot::new_with_transform_output(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
            old_output.clone(),
        ));
        lifecycle.publish_promoted(old.clone());
        lifecycle.publish_applied(old.clone()).unwrap();
        lifecycle.publish_transform_failure(RuntimeTransformFailure {
            attempt_revision: lifecycle.allocate_revision().unwrap(),
            transform_uid: "sj-stale".into(),
            scope_uid: None,
            script_type: Some(ScriptType::JavaScript),
            message: "stale transform failure".into(),
        });
        let transaction = capture_runtime_transaction(&paths, &lifecycle)
            .await
            .unwrap();

        restore_product(paths.product(), b"mode: direct\n")
            .await
            .unwrap();
        let replacement = Arc::new(RuntimeSnapshot::new_with_transform_output(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::ClashRs,
            b"mode: direct\n".to_vec(),
            Mapping::new(),
            transform_output("replacement runtime"),
        ));
        lifecycle.publish_promoted(replacement.clone());
        lifecycle.publish_applied(replacement).unwrap();

        restore_failed_apply(&paths, &lifecycle, transaction, |_| async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(std::fs::read(paths.product()).unwrap(), b"mode: rule\n");
        let restored = lifecycle.snapshot();
        assert_eq!(restored.promoted.unwrap().revision, old.revision);
        let applied = restored.applied.unwrap();
        assert_eq!(applied.revision, old.revision);
        assert_eq!(applied.postprocessing_output, old_output);
        assert!(
            restored.last_transform_failure.is_none(),
            "rollback must not resurrect a transform failure cleared by the newer attempt"
        );
    }

    #[tokio::test]
    async fn failed_recovery_restores_product_but_clears_applied_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let lifecycle = RuntimeLifecycle::default();
        restore_product(paths.product(), b"mode: rule\n")
            .await
            .unwrap();
        let old = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
        ));
        lifecycle.publish_promoted(old.clone());
        lifecycle.publish_applied(old.clone()).unwrap();
        let transaction = capture_runtime_transaction(&paths, &lifecycle)
            .await
            .unwrap();

        restore_product(paths.product(), b"mode: direct\n")
            .await
            .unwrap();
        let replacement = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::ClashRs,
            b"mode: direct\n".to_vec(),
            Mapping::new(),
        ));
        lifecycle.publish_promoted(replacement.clone());
        lifecycle.publish_applied(replacement).unwrap();

        let error = restore_failed_apply(&paths, &lifecycle, transaction, |_| async {
            bail!("old core could not restart")
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains("old core could not restart"));
        assert_eq!(std::fs::read(paths.product()).unwrap(), b"mode: rule\n");
        let restored = lifecycle.snapshot();
        assert_eq!(restored.promoted.unwrap().revision, old.revision);
        assert!(restored.applied.is_none());
    }

    #[tokio::test]
    async fn rebuild_gate_serializes_concurrent_transactions() {
        let gate = Arc::new(RuntimeRebuildGate::default());
        let ready = Arc::new(tokio::sync::Barrier::new(3));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..2 {
            let gate = gate.clone();
            let ready = ready.clone();
            let active = active.clone();
            let maximum = maximum.clone();
            tasks.push(tokio::spawn(async move {
                ready.wait().await;
                let _guard = gate.lock().await;
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(25)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        ready.wait().await;
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn runtime_snapshot_preserves_effective_controller_endpoint() {
        let config: Mapping = serde_yaml::from_str(
            "mixed-port: 7890\nexternal-controller: 127.0.0.1:55736\nsecret: chimera\n",
        )
        .unwrap();
        let snapshot = RuntimeSnapshot::new(
            RuntimeRevision::from_raw(1),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            config,
        );

        assert_eq!(snapshot.clash_info().server, "127.0.0.1:55736");
        assert_eq!(snapshot.clash_info().port, 7890);
        assert_eq!(snapshot.clash_info().secret.as_deref(), Some("chimera"));
    }

    #[test]
    fn runtime_snapshot_preserves_executor_applied_fields() {
        let snapshot = RuntimeSnapshot::new_with_transform_output_and_exists_keys(
            RuntimeRevision::from_raw(1),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
            PostProcessingOutput::default(),
            vec!["mode".into(), "rules".into()],
        );

        assert_eq!(snapshot.exists_keys, ["mode", "rules"]);
    }

    #[test]
    fn pending_product_republishes_only_for_the_same_accepted_receipt() {
        let lifecycle = RuntimeLifecycle::default();
        let product = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
        ));
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.revision = product.revision;
        receipt.config_digest = chimera_core_manager::payload_digest(product.product_bytes());
        receipt.artifact = Some(product.clone());
        let receipt = Arc::new(receipt);

        lifecycle.publish_confirmed(receipt.clone());
        let (staged_receipt, staged_product) = lifecycle
            .pending_confirmed_product()
            .expect("an accepted Core receipt retains the exact product before file promotion");
        assert!(Arc::ptr_eq(&staged_receipt, &receipt));
        assert!(Arc::ptr_eq(&staged_product, &product));
        assert!(
            lifecycle
                .republish_confirmed_product(&receipt)
                .unwrap()
                .is_none(),
            "unpromoted bytes must never be marked Applied without repairing the disk file"
        );
        lifecycle.publish_promoted(product.clone());
        let (pending_receipt, pending_product) = lifecycle
            .pending_confirmed_product()
            .expect("confirmed promotion must remain pending before Applied");
        assert!(Arc::ptr_eq(&pending_receipt, &receipt));
        assert!(pending_product.identity_eq(&product));
        assert!(
            lifecycle
                .republish_confirmed_product(&Arc::new((*receipt).clone()))
                .unwrap()
                .is_none(),
            "a copied or superseded receipt cannot republish a product"
        );
        let published = lifecycle
            .republish_confirmed_product(&receipt)
            .unwrap()
            .expect("retry should finish the existing accepted product");
        assert!(published.identity_eq(&product));
        assert!(lifecycle.pending_confirmed_product().is_none());
        assert!(
            lifecycle
                .republish_confirmed_product(&receipt)
                .unwrap()
                .is_none(),
            "an already applied product must not be published twice"
        );

        let newer = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: direct\n".to_vec(),
            Mapping::new(),
        ));
        lifecycle.publish_promoted(newer);
        assert!(
            lifecycle.pending_confirmed_product().is_none(),
            "a promoted product from another revision must not use an old receipt"
        );
        lifecycle.invalidate_confirmed();
        assert!(lifecycle.pending_confirmed_product().is_none());
    }

    #[test]
    fn applied_requires_the_matching_promoted_snapshot() {
        let lifecycle = RuntimeLifecycle::default();
        let first = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
        ));
        assert!(lifecycle.publish_applied(first.clone()).is_err());
        lifecycle.publish_promoted(first.clone());
        lifecycle.publish_applied(first.clone()).unwrap();
        assert_eq!(
            lifecycle.snapshot().applied.unwrap().product_sha256,
            first.product_sha256
        );

        let second = Arc::new(RuntimeSnapshot::new(
            lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: direct\n".to_vec(),
            Mapping::new(),
        ));
        assert!(lifecycle.publish_applied(second).is_err());
    }
}

#[cfg(test)]
mod shared_runtime_values_contract_tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn desktop_receipt_status_and_phase_use_the_shared_ref_types() {
        assert_eq!(
            TypeId::of::<CommitReceipt>(),
            TypeId::of::<chimera_contracts::runtime::CommitReceipt>()
        );
        assert_eq!(
            TypeId::of::<RuntimeCommitStatus>(),
            TypeId::of::<chimera_contracts::runtime::RuntimeCommitStatus>()
        );
        assert_eq!(
            TypeId::of::<DegradationPhase>(),
            TypeId::of::<chimera_contracts::runtime::DegradationPhase>()
        );
        let receipt = CommitReceipt {
            operation_id: Some("core-lease".into()),
            domain: "clash".into(),
            source_version: 8,
            runtime: RuntimeCommitStatus::RecoveryRequired,
        };
        let outcome = MutationOutcome::from_parts((), Vec::new()).with_commit(receipt);
        let wire = serde_json::to_value(outcome).unwrap();
        assert_eq!(wire["status"], "committed");
        assert_eq!(wire["commits"][0]["runtime"], "recovery_required");
        assert_eq!(wire["commits"][0]["source_version"], 8);
    }

    #[test]
    fn typed_degradation_keeps_legacy_flat_ipc_shape() {
        use chimera_contracts::runtime as shared;

        let projected = Degradation::from_shared(shared::Degradation {
            phase: shared::DegradationPhase::ProfileMaterialization,
            reason: shared::DegradationReason::CleanupDeferred,
            message: "fenced cleanup".into(),
            retryable: true,
        });
        let wire = serde_json::to_value(projected).unwrap();
        assert_eq!(wire["phase"], "profile_materialization");
        assert_eq!(wire["code"], "cleanup_deferred");
        assert_eq!(wire["message"], "fenced cleanup");
        assert_eq!(wire["retryable"], true);
        assert!(wire.get("reason").is_none());
    }
}

impl ChimeraClient {
    pub(crate) async fn rebuild_running_config(&self) -> anyhow::Result<()> {
        let result = self.inner.core_lifecycle.reconcile().await;
        if let Err(error) = result {
            self.inner.ui_sink.refresh_runtime_transform_diagnostics();
            return Err(error);
        }
        self.inner.ui_sink.refresh_clash();
        crate::feat::update_proxies_buff(self.clone(), None);
        Ok(())
    }
}

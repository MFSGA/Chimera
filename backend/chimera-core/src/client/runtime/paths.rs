//! Runtime product and candidate file lifecycle shared by Core and Reconciler.
use super::{RuntimeLifecycle, RuntimeTransactionSnapshot};
use anyhow::{Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use std::{
    fs::OpenOptions,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG: &str = "clash-config.yaml";
pub const RUNTIME_CONFIG_FILE: &str = RUNTIME_CONFIG;
const CANDIDATE_DIR: &str = ".candidates";

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    product: Utf8PathBuf,
    candidate_dir: Utf8PathBuf,
}

impl RuntimePaths {
    pub fn from_config_dir(config_dir: &Path) -> anyhow::Result<Self> {
        let runtime_dir = Utf8PathBuf::from_path_buf(config_dir.join(RUNTIME_CONFIG_DIR))
            .map_err(|path| anyhow::anyhow!("runtime path is not UTF-8: {}", path.display()))?;
        Ok(Self {
            product: runtime_dir.join(RUNTIME_CONFIG),
            candidate_dir: runtime_dir.join(CANDIDATE_DIR),
        })
    }

    #[allow(dead_code)]
    pub fn new(product: Utf8PathBuf, candidate_dir: Utf8PathBuf) -> Self {
        Self {
            product,
            candidate_dir,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_config_root(config_root: PathBuf) -> Self {
        Self::from_config_dir(&config_root).expect("test runtime path is UTF-8")
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

    pub async fn cleanup_stale_candidates(&self, max_age: Duration) -> Result<usize, Error> {
        prepare_private_dir(&self.candidate_dir).await?;
        let path = self.candidate_dir.clone();
        chimera_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || cleanup_candidates(&path, max_age)).await,
        )
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
        let bytes = anyhow::Context::with_context(tokio::fs::read(&self.path).await, || {
            format!("read runtime candidate {}", self.path.display())
        })?;
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

// Directory iteration holds mutable iterator state across retries. Keep the
// synchronous scan on a blocking worker, with retries around each filesystem
// operation rather than repeating the whole cleanup.
fn cleanup_candidates(path: &Utf8Path, max_age: Duration) -> Result<usize, Error> {
    let now = SystemTime::now();
    let mut removed = 0;
    let mut entries = chimera_persistence::retry_io(|| std::fs::read_dir(path))
        .context(ReadCandidatesSnafu { path })?;
    while let Some(entry) = chimera_persistence::retry_io(|| entries.next().transpose())
        .context(ReadCandidatesSnafu { path })?
    {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("candidate-") {
            continue;
        }
        let metadata = chimera_persistence::retry_io(|| std::fs::symlink_metadata(entry.path()))
            .context(InspectCandidateSnafu { path: entry.path() })?;
        if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
            continue;
        }
        let modified = chimera_persistence::retry_io(|| metadata.modified())
            .context(InspectCandidateSnafu { path: entry.path() })?;
        let is_stale = now
            .duration_since(modified)
            .ok()
            .is_some_and(|age| age >= max_age);
        if is_stale {
            match chimera_persistence::retry_io(|| std::fs::remove_file(entry.path())) {
                Ok(()) => removed += 1,
                Err(error) => tracing::warn!(path = %entry.path().display(),
                    error = %snafu::Report::from_error(&error), action = "skip_stale_candidate",
                    "could not remove a stale runtime candidate"),
            }
        }
    }
    Ok(removed)
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to create candidate directory {path}"))]
    CreateCandidateDirectory {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("candidate path is not a private directory: {path}"))]
    ValidateCandidateDirectory { path: Utf8PathBuf },
    #[snafu(display("failed to inspect candidate path {}", path.display()))]
    InspectCandidate {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to read candidate directory {path}"))]
    ReadCandidates {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to preserve invalid candidate directory"))]
    PreserveCandidateDirectory { source: chimera_persistence::Error },
    #[cfg(unix)]
    #[snafu(display("failed to secure candidate directory {path}"))]
    SecureCandidateDirectory {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
}

async fn prepare_private_dir(path: &Utf8Path) -> Result<(), Error> {
    prepare_private_dir_with(path, || tokio::fs::create_dir_all(path)).await
}

async fn prepare_private_dir_with<F>(
    path: &Utf8Path,
    create: impl FnMut() -> F,
) -> Result<(), Error>
where
    F: std::future::Future<Output = std::io::Result<()>>,
{
    match chimera_persistence::retry_async(|| tokio::fs::symlink_metadata(path)).await {
        Ok(metadata) if is_symlink_or_reparse(&metadata) || !metadata.is_dir() => {
            let error = ValidateCandidateDirectorySnafu { path }.build();
            let original = path.to_path_buf();
            chimera_owner_support::blocking::join(
                tokio::task::spawn_blocking(move || {
                    chimera_persistence::preserve(original.as_std_path(), &error)
                })
                .await,
            )
            .context(PreserveCandidateDirectorySnafu)?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).context(InspectCandidateSnafu {
                path: path.as_std_path(),
            });
        }
    }
    chimera_persistence::retry_async(create)
        .await
        .context(CreateCandidateDirectorySnafu { path })?;
    let metadata = chimera_persistence::retry_async(|| tokio::fs::symlink_metadata(path))
        .await
        .context(InspectCandidateSnafu {
            path: path.as_std_path(),
        })?;
    if is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
        return ValidateCandidateDirectorySnafu { path }.fail();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        chimera_persistence::retry_async(|| {
            tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        })
        .await
        .context(SecureCandidateDirectorySnafu { path })?;
    }
    Ok(())
}

#[cfg(unix)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use crate::client::runtime::{
        RuntimeRebuildGate, RuntimeRevision, RuntimeRevisionAllocator, RuntimeSnapshot,
        RuntimeTransformFailure, ScriptType,
    };
    use chimera_config::application::ClashCore;
    use chimera_profile_transform::PostProcessingOutput;
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

    fn evidence(path: &Path) -> Vec<PathBuf> {
        let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_string_lossy());
        std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.unwrap();
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&prefix)
                    .then(|| entry.path())
            })
            .collect()
    }

    #[tokio::test]
    async fn candidate_directory_file_is_preserved_then_candidates_are_usable() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        std::fs::create_dir_all(paths.candidate_dir().parent().unwrap()).unwrap();
        std::fs::write(paths.candidate_dir(), b"original node").unwrap();
        assert_eq!(
            paths
                .cleanup_stale_candidates(Duration::ZERO)
                .await
                .unwrap(),
            0
        );
        let evidence = evidence(paths.candidate_dir().as_std_path());
        assert_eq!(evidence.len(), 1);
        assert_eq!(std::fs::read(&evidence[0]).unwrap(), b"original node");
        assert!(paths.candidate_dir().is_dir());
    }

    #[tokio::test(start_paused = true)]
    async fn candidate_directory_creation_retries_at_the_preparation_call_site() {
        for failures in [3, 4] {
            let dir = tempfile::tempdir().unwrap();
            let paths = paths(&dir);
            let mut attempts = 0;
            let before = tokio::time::Instant::now();
            let result = prepare_private_dir_with(paths.candidate_dir(), || {
                attempts += 1;
                let failed = attempts <= failures;
                let path = paths.candidate_dir();
                async move {
                    if failed {
                        Err(std::io::ErrorKind::WouldBlock.into())
                    } else {
                        tokio::fs::create_dir_all(path).await
                    }
                }
            })
            .await;
            assert_eq!(attempts, 4);
            assert_eq!(
                tokio::time::Instant::now() - before,
                Duration::from_millis(350)
            );
            if failures == 3 {
                result.unwrap();
                assert!(paths.candidate_dir().is_dir());
            } else {
                assert!(
                    matches!(result, Err(Error::CreateCandidateDirectory { source, .. })
                    if source.kind() == std::io::ErrorKind::WouldBlock)
                );
                assert!(!paths.candidate_dir().exists());
            }
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn one_stale_candidate_removal_failure_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        prepare_private_dir(paths.candidate_dir()).await.unwrap();
        use std::os::windows::fs::OpenOptionsExt as _;
        let blocked = paths.candidate_dir().join("candidate-held.yaml");
        let removable = paths.candidate_dir().join("candidate-removable.yaml");
        std::fs::write(&blocked, b"kept").unwrap();
        std::fs::write(&removable, b"remove").unwrap();
        // A real sharing violation: allow reads/writes, but refuse deletion.
        const FILE_SHARE_READ_WRITE: u32 = 3;
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ_WRITE)
            .open(&blocked)
            .unwrap();
        let result = paths.cleanup_stale_candidates(Duration::ZERO).await;
        drop(held);
        assert_eq!(result.unwrap(), 1);
        assert_eq!(std::fs::read(&blocked).unwrap(), b"kept");
        assert!(!removable.exists());
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn candidate_directory_preserves_symlink_and_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        let target = dir.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(paths.candidate_dir().parent().unwrap()).unwrap();
        std::fs::write(target.join("kept"), b"target data").unwrap();
        #[cfg(unix)]
        let link = std::os::unix::fs::symlink(&target, paths.candidate_dir());
        #[cfg(windows)]
        let link = std::os::windows::fs::symlink_dir(&target, paths.candidate_dir());
        if let Err(error) = &link
            && error.kind() == std::io::ErrorKind::PermissionDenied
        {
            eprintln!("SKIPPED candidate symlink fixture: symlink privilege unavailable");
            return;
        }
        link.unwrap();
        paths
            .cleanup_stale_candidates(Duration::ZERO)
            .await
            .unwrap();
        assert!(paths.candidate_dir().is_dir());
        let evidence = evidence(paths.candidate_dir().as_std_path());
        assert_eq!(evidence.len(), 1);
        assert!(
            std::fs::symlink_metadata(&evidence[0])
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(target.join("kept")).unwrap(), b"target data");
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

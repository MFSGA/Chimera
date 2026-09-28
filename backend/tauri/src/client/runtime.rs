use std::{
    fs::OpenOptions,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use sha2::{Digest, Sha256};

use super::ChimeraClient;

use crate::{
    client::utf8_path,
    config::{
        chimera::ClashCore,
        clash::{ClashInfo, IClashTemp},
        profile::item_type::{ProfileUid, ScriptType},
    },
    enhance::PostProcessingOutput,
    utils::path::PathResolver,
};

/// Public mutation wire aligned with REF: desired state is committed first;
/// post-commit side-effect failures degrade instead of turning the mutation
/// into an error that would imply the commit was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Committed {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
    },
    CommittedDegraded {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
        degradations: Vec<Degradation>,
    },
}

impl<T> MutationOutcome<T> {
    pub fn from_parts(value: T, degradations: Vec<Degradation>) -> Self {
        if degradations.is_empty() {
            Self::Committed {
                value,
                commits: Vec::new(),
                notifications_pending: true,
            }
        } else {
            Self::CommittedDegraded {
                value,
                commits: Vec::new(),
                notifications_pending: true,
                degradations,
            }
        }
    }

    pub fn with_commit(mut self, receipt: CommitReceipt) -> Self {
        match &mut self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.push(receipt)
            }
        }
        self
    }

    pub fn value(&self) -> &T {
        match self {
            Self::Committed { value, .. } | Self::CommittedDegraded { value, .. } => value,
        }
    }

    pub fn into_value(self) -> T {
        self.into_parts().0
    }

    pub fn append_commit_result(self, result: MutationOutcome<()>) -> Self {
        let commits = match &result {
            MutationOutcome::Committed { commits, .. }
            | MutationOutcome::CommittedDegraded { commits, .. } => commits.clone(),
        };
        let mut outcome = self.extend_degradations(result.into_parts().1);
        for commit in commits {
            outcome = outcome.with_commit(commit);
        }
        outcome
    }

    pub fn degradations(&self) -> &[Degradation] {
        match self {
            Self::Committed { .. } => &[],
            Self::CommittedDegraded { degradations, .. } => degradations,
        }
    }

    pub fn into_parts(self) -> (T, Vec<Degradation>) {
        match self {
            Self::Committed { value, .. } => (value, Vec::new()),
            Self::CommittedDegraded {
                value,
                degradations,
                ..
            } => (value, degradations),
        }
    }

    pub fn extend_degradations(self, extra: Vec<Degradation>) -> Self {
        let commits = match &self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.clone()
            }
        };
        let (value, mut degradations) = self.into_parts();
        degradations.extend(extra);
        commits
            .into_iter()
            .fold(Self::from_parts(value, degradations), |outcome, receipt| {
                outcome.with_commit(receipt)
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Degradation {
    pub phase: DegradationPhase,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DegradationPhase {
    LegacyMirror,
    ProfileMaterialization,
    RuntimeBuild,
    RuntimeCheck,
    RuntimePromote,
    RuntimePublish,
    RuntimeApply,
    CoreRollback,
    SystemEffect,
    UiEffect,
}

/// A source commit and its critical runtime result. Peripheral owners settle separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CommitReceipt {
    pub operation_id: Option<String>,
    pub domain: String,
    pub source_version: u64,
    pub runtime: RuntimeCommitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommitStatus {
    Applied,
    Deferred,
    SavedInactive,
    Unchanged,
    Pending,
    RecoveryRequired,
}

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG_FILE: &str = "clash-config.yaml";
/// Ref-compatible name for the generated runtime product.
pub const RUNTIME_CONFIG: &str = RUNTIME_CONFIG_FILE;
const CANDIDATE_DIR: &str = ".candidates";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeRevision(u64);

impl RuntimeRevision {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Default)]
pub struct RuntimeRebuildGate(tokio::sync::Mutex<()>);

impl RuntimeRebuildGate {
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.0.lock().await
    }

    pub fn is_locked(&self) -> bool {
        self.0.try_lock().is_err()
    }
}

#[derive(Debug, Default)]
pub struct RuntimeRevisionAllocator(AtomicU64);

impl RuntimeRevisionAllocator {
    pub fn allocate(&self) -> Result<RuntimeRevision> {
        let previous = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("runtime revision space exhausted"))?;
        Ok(RuntimeRevision(previous + 1))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSnapshotData {
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
    pub(crate) inspection: Arc<super::runtime_inspection::RuntimeInspectionData>,
}

#[derive(Debug, Clone)]
pub struct RuntimeSnapshot {
    pub(crate) inspection_id: String,
    pub revision: RuntimeRevision,
    pub target_core: ClashCore,
    pub product_sha256: [u8; 32],
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
    product_bytes: Arc<[u8]>,
    pub(crate) inspection: Arc<super::runtime_inspection::RuntimeInspectionData>,
}

impl RuntimeSnapshot {
    pub(crate) fn from_data(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Arc<[u8]>,
        data: RuntimeSnapshotData,
    ) -> Self {
        let product_sha256 = Sha256::digest(&product_bytes).into();
        Self {
            inspection_id: nanoid::nanoid!(),
            revision,
            target_core,
            product_sha256,
            config: data.config,
            exists_keys: data.exists_keys,
            postprocessing_output: data.postprocessing_output,
            product_bytes,
            inspection: data.inspection,
        }
    }

    #[cfg(test)]
    pub fn new(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Vec<u8>,
        config: Mapping,
    ) -> Self {
        Self::new_with_transform_output(
            revision,
            target_core,
            product_bytes,
            config,
            PostProcessingOutput::default(),
        )
    }

    pub fn new_with_transform_output(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Vec<u8>,
        config: Mapping,
        transform_output: PostProcessingOutput,
    ) -> Self {
        Self::new_with_transform_output_and_exists_keys(
            revision,
            target_core,
            product_bytes,
            config,
            transform_output,
            Vec::new(),
        )
    }

    pub(crate) fn new_with_transform_output_and_exists_keys(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Vec<u8>,
        config: Mapping,
        transform_output: PostProcessingOutput,
        exists_keys: Vec<String>,
    ) -> Self {
        Self::from_data(
            revision,
            target_core,
            product_bytes.into(),
            RuntimeSnapshotData {
                config,
                exists_keys,
                postprocessing_output: transform_output,
                inspection: Arc::new(super::runtime_inspection::RuntimeInspectionData::bare()),
            },
        )
    }

    pub(crate) fn new_with_transform_output_and_inspection(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Vec<u8>,
        config: Mapping,
        transform_output: PostProcessingOutput,
        inspection: super::runtime_inspection::RuntimeInspectionData,
    ) -> Self {
        Self::new_with_transform_output_and_inspection_and_exists_keys(
            revision,
            target_core,
            product_bytes,
            config,
            transform_output,
            inspection,
            Vec::new(),
        )
    }

    pub(crate) fn new_with_transform_output_and_inspection_and_exists_keys(
        revision: RuntimeRevision,
        target_core: ClashCore,
        product_bytes: Vec<u8>,
        config: Mapping,
        transform_output: PostProcessingOutput,
        inspection: super::runtime_inspection::RuntimeInspectionData,
        exists_keys: Vec<String>,
    ) -> Self {
        Self::from_data(
            revision,
            target_core,
            product_bytes.into(),
            RuntimeSnapshotData {
                config,
                exists_keys,
                postprocessing_output: transform_output,
                inspection: Arc::new(inspection),
            },
        )
    }

    pub fn product_bytes(&self) -> &[u8] {
        &self.product_bytes
    }

    pub fn clash_info(&self) -> ClashInfo {
        IClashTemp(self.config.clone()).get_client_info()
    }

    fn identity_eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.target_core == other.target_core
            && self.product_sha256 == other.product_sha256
    }

    pub(crate) fn with_revision(&self, revision: RuntimeRevision) -> Self {
        Self::from_data(
            revision,
            self.target_core,
            self.product_bytes.clone(),
            RuntimeSnapshotData {
                config: self.config.clone(),
                exists_keys: self.exists_keys.clone(),
                postprocessing_output: self.postprocessing_output.clone(),
                inspection: self.inspection.clone(),
            },
        )
    }
}

/// What the selected core host confirmed it applied. Keep the exact submitted
/// bytes and host settings here: a Profile transaction may have to restore this
/// runtime even when the previous Profile source now describes something else.
/// `artifact` is Chimera's derived UI snapshot paired with the receipt; it is
/// not evidence that the core accepted the document.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeApplyReceipt {
    pub revision: RuntimeRevision,
    pub config_text: Arc<str>,
    pub config_digest: String,
    pub target_core: ClashCore,
    pub core_spec: chimera_core_manager::CoreSpec,
    pub host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    pub run_intent: super::application_workflow::policy::CoreRunIntent,
    pub local_ipc: chimera_core_manager::LocalIpcSettings,
    pub applied_revision: chimera_ipc::api::status::RevisionIdInfo,
    pub ports: chimera_config::runtime::executor::ResolvedPortBindings,
    pub artifact: Option<Arc<RuntimeSnapshot>>,
}

impl RuntimeApplyReceipt {
    pub(crate) fn confirms(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
        status: &crate::core::actor_v2::control_endpoint::CoreStatusSnapshot,
    ) -> bool {
        matches!(
            status.state,
            Some(chimera_ipc::api::status::CoreStateDetail::Running { .. })
        ) && host == self.host
            && status.source_hash.as_deref() == Some(self.config_digest.as_str())
            && status
                .applied_kind
                .is_none_or(|kind| kind == self.core_spec.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTransformFailure {
    pub attempt_revision: RuntimeRevision,
    pub transform_uid: ProfileUid,
    pub scope_uid: Option<ProfileUid>,
    pub script_type: Option<ScriptType>,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeLifecycleState {
    pub promoted: Option<Arc<RuntimeSnapshot>>,
    pub applied: Option<Arc<RuntimeSnapshot>>,
    pub confirmed: Option<Arc<RuntimeApplyReceipt>>,
    pub last_transform_failure: Option<RuntimeTransformFailure>,
}

#[derive(Debug, Default)]
pub struct RuntimeLifecycle {
    revisions: RuntimeRevisionAllocator,
    state: RwLock<RuntimeLifecycleState>,
}

impl RuntimeLifecycle {
    pub fn allocate_revision(&self) -> Result<RuntimeRevision> {
        self.revisions.allocate()
    }

    pub fn snapshot(&self) -> RuntimeLifecycleState {
        self.state.read().clone()
    }

    pub fn publish_promoted(&self, snapshot: Arc<RuntimeSnapshot>) {
        self.state.write().promoted = Some(snapshot);
    }

    pub fn publish_confirmed(&self, receipt: Arc<RuntimeApplyReceipt>) {
        let mut state = self.state.write();
        state.applied = None;
        state.confirmed = Some(receipt);
    }

    pub fn confirmed_receipt(&self) -> Option<Arc<RuntimeApplyReceipt>> {
        self.state.read().confirmed.clone()
    }

    pub fn publish_applied(&self, snapshot: Arc<RuntimeSnapshot>) -> Result<()> {
        let mut state = self.state.write();
        let promoted = state
            .promoted
            .as_deref()
            .context("cannot publish Applied before Promoted")?;
        if !promoted.identity_eq(&snapshot) {
            bail!("Applied snapshot does not match the promoted runtime product");
        }
        if let Some(receipt) = state.confirmed.as_ref() {
            if receipt.revision != snapshot.revision
                || receipt.config_digest
                    != chimera_core_manager::payload_digest(snapshot.product_bytes())
                || receipt.target_core != snapshot.target_core
            {
                bail!("Applied snapshot does not match the confirmed runtime receipt");
            }
        }
        state.applied = Some(snapshot);
        state.last_transform_failure = None;
        Ok(())
    }

    pub fn publish_transform_failure(&self, failure: RuntimeTransformFailure) {
        self.state.write().last_transform_failure = Some(failure);
    }

    pub fn clear_transform_failure(&self) {
        self.state.write().last_transform_failure = None;
    }

    pub fn restore(&self, state: RuntimeLifecycleState) {
        *self.state.write() = state;
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeTransactionSnapshot {
    pub product: Option<Vec<u8>>,
    pub lifecycle: RuntimeLifecycleState,
}

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
            // legacy_product: product.clone(),
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
            /* legacy_product: utf8_path(config_root.join(RUNTIME_CONFIG))
            .expect("test runtime path is UTF-8"), */
        }
    }

    pub fn product(&self) -> &Utf8Path {
        &self.product
    }

    #[allow(dead_code)]
    pub fn candidate_dir(&self) -> &Utf8Path {
        &self.candidate_dir
    }

    #[cfg(test)]
    fn legacy_product(&self) -> &Utf8Path {
        todo!()
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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            todo!()
        }
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
            lifecycle.restore(degraded);
            Err(error)
        }
    }
}

async fn prepare_private_dir(path: &Utf8Path) -> anyhow::Result<()> {
    if let Ok(metadata) = tokio::fs::symlink_metadata(path).await
        && is_symlink_or_reparse(&metadata)
    {
        anyhow::bail!("runtime candidate directory is a symlink or reparse point: {path}");
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
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn paths(dir: &tempfile::TempDir) -> RuntimePaths {
        todo!() // RuntimePaths::from_config_root(dir.path().to_path_buf())
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
        todo!()
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
        todo!()
        /* restore_product(paths.product(), b"mode: rule\n")
        .await
        .unwrap(); */
        /*  let candidate = paths.create_candidate(b"mode: direct\n").await.unwrap();
        std::fs::write(candidate.path(), b"mode: global\n").unwrap();
        let error = promote_candidate(&candidate, paths.product())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("changed after creation"));
        assert_eq!(std::fs::read(paths.product()).unwrap(), b"mode: rule\n"); */
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
    async fn transaction_capture_falls_back_to_legacy_product() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(&dir);
        /* std::fs::write(paths.legacy_product(), b"mode: rule\n").unwrap();
        let lifecycle = RuntimeLifecycle::default();
        let transaction = capture_runtime_transaction(&paths, &lifecycle)
            .await
            .unwrap();
        assert_eq!(transaction.product.unwrap(), b"mode: rule\n"); */
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
            RuntimeRevision(1),
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
            RuntimeRevision(1),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            Mapping::new(),
            PostProcessingOutput::default(),
            vec!["mode".into(), "rules".into()],
        );

        assert_eq!(snapshot.exists_keys, ["mode", "rules"]);
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

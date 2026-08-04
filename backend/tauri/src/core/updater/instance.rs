use super::shared::{self, CoreTypeMeta};
use crate::{
    config::{chimera::ClashCore, core::Config},
    core::{
        CoreManager,
        download::{DownloadSession, DownloadStatus},
    },
};
use anyhow::anyhow;
use runas::Command as RunasCommand;
use serde::Serialize;
use specta::Type;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tempfile::TempDir;

#[cfg(feature = "e2e")]
const E2E_UPDATER_BLOCKED: &str = "core updater is disabled in E2E mode";
const MAX_CORE_BINARY_SIZE: u64 = 256 * 1024 * 1024;

static CORE_REPLACE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static CORE_REPLACE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn core_transaction_prefix(target: &Path, kind: &str) -> String {
    format!(
        ".{}.chimera-{kind}-",
        target
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("core")
    )
}

fn core_transaction_path(target: &Path, kind: &str) -> PathBuf {
    let sequence = CORE_REPLACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(
        "{}{}-{sequence}",
        core_transaction_prefix(target, kind),
        std::process::id()
    ))
}

fn core_transaction_files(target: &Path, kind: &str) -> anyhow::Result<Vec<PathBuf>> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("core target has no parent directory"))?;
    let prefix = core_transaction_prefix(target, kind);
    let mut paths = std::fs::read_dir(parent)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn recover_interrupted_core_replace(target: &Path) -> anyhow::Result<()> {
    for staged in core_transaction_files(target, "staged")? {
        std::fs::remove_file(staged)?;
    }

    let backups = core_transaction_files(target, "backup")?;
    if target.exists() {
        for backup in backups {
            std::fs::remove_file(backup)?;
        }
        return Ok(());
    }

    match backups.as_slice() {
        [] => Ok(()),
        [backup] => {
            std::fs::rename(backup, target)?;
            Ok(())
        }
        _ => anyhow::bail!(
            "multiple interrupted core backups found for {}",
            target.display()
        ),
    }
}

pub(super) fn recover_interrupted_core_replacements_in_dir(
    core_dir: &Path,
    core_names: &[&str],
) -> anyhow::Result<()> {
    for core_name in core_names {
        let target = core_dir.join(format!("{core_name}{}", std::env::consts::EXE_SUFFIX));
        recover_interrupted_core_replace(&target)?;
    }
    Ok(())
}

fn commit_staged_core_with<F>(
    staged: &Path,
    target: &Path,
    backup: &Path,
    mut rename: F,
) -> anyhow::Result<()>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let had_target = target.exists();
    if had_target {
        rename(target, backup)?;
    }

    if let Err(error) = rename(staged, target) {
        if had_target {
            rename(backup, target).map_err(|restore_error| {
                anyhow!(
                    "failed to install staged core ({error}) and restore previous core ({restore_error})"
                )
            })?;
        }
        return Err(error.into());
    }

    if had_target && backup.exists() {
        std::fs::remove_file(backup)?;
    }
    Ok(())
}

fn replace_core_file_with<F>(source: &Path, target: &Path, rename: F) -> anyhow::Result<u64>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let _guard = CORE_REPLACE_LOCK.lock();
    recover_interrupted_core_replace(target)?;

    let staged = core_transaction_path(target, "staged");
    let backup = core_transaction_path(target, "backup");
    let size = match std::fs::copy(source, &staged) {
        Ok(size) => size,
        Err(error) => {
            let _ = std::fs::remove_file(&staged);
            return Err(error.into());
        }
    };
    let result = commit_staged_core_with(&staged, target, &backup, rename);
    if result.is_err() && staged.exists() {
        let _ = std::fs::remove_file(&staged);
    }
    result.map(|_| size)
}

fn replace_core_file(source: &Path, target: &Path) -> anyhow::Result<u64> {
    replace_core_file_with(source, target, |from, to| std::fs::rename(from, to))
}

#[cfg(target_os = "windows")]
fn powershell_literal(value: &Path) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "''"))
}

#[cfg(target_os = "windows")]
fn build_elevated_replace_script(
    source: &Path,
    target: &Path,
    backup: &Path,
) -> anyhow::Result<String> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("core target has no parent directory"))?;
    let backup_prefix = core_transaction_prefix(target, "backup").replace('\'', "''");
    let staged_prefix = core_transaction_prefix(target, "staged").replace('\'', "''");

    Ok(format!(
        "$ErrorActionPreference = 'Stop'; \
         $source = {}; $target = {}; $backup = {}; $parent = {}; \
         $backupPrefix = '{}'; $stagedPrefix = '{}'; \
         Get-ChildItem -LiteralPath $parent -File | \
           Where-Object {{ $_.Name.StartsWith($stagedPrefix, [System.StringComparison]::Ordinal) }} | \
           Remove-Item -Force; \
         $existingBackups = @(Get-ChildItem -LiteralPath $parent -File | \
           Where-Object {{ $_.Name.StartsWith($backupPrefix, [System.StringComparison]::Ordinal) }}); \
         if (Test-Path -LiteralPath $target) {{ \
           $existingBackups | Remove-Item -Force; \
         }} elseif ($existingBackups.Count -eq 1) {{ \
           Move-Item -LiteralPath $existingBackups[0].FullName -Destination $target -Force; \
         }} elseif ($existingBackups.Count -gt 1) {{ \
           throw 'multiple interrupted core backups found'; \
         }}; \
         $hadTarget = Test-Path -LiteralPath $target; \
         if ($hadTarget) {{ Move-Item -LiteralPath $target -Destination $backup -Force }}; \
         try {{ \
           Copy-Item -LiteralPath $source -Destination $target -Force; \
           if ($hadTarget -and (Test-Path -LiteralPath $backup)) {{ \
             Remove-Item -LiteralPath $backup -Force \
           }} \
         }} catch {{ \
           if (Test-Path -LiteralPath $target) {{ Remove-Item -LiteralPath $target -Force }}; \
           if ($hadTarget -and (Test-Path -LiteralPath $backup)) {{ \
             Move-Item -LiteralPath $backup -Destination $target -Force \
           }}; \
           throw \
         }}",
        powershell_literal(source),
        powershell_literal(target),
        powershell_literal(backup),
        powershell_literal(parent),
        backup_prefix,
        staged_prefix,
    ))
}

fn ensure_updater_allowed() -> anyhow::Result<()> {
    #[cfg(feature = "e2e")]
    {
        anyhow::bail!(E2E_UPDATER_BLOCKED);
    }
    #[cfg(not(feature = "e2e"))]
    {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Default, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdaterState {
    #[default]
    Idle,
    Downloading,
    Decompressing,
    Replacing,
    Restarting,
    Done,
    Failed(String),
}

pub(super) struct Updater {
    id: usize,
    temp_dir: TempDir,
    core_type: ClashCore,
    artifact: String,
    inner: parking_lot::RwLock<UpdaterInner>,
    downloader: Arc<DownloadSession>,
}

struct UpdaterInner {
    state: UpdaterState,
}

#[derive(Debug, Serialize, Type)]
pub struct UpdaterSummary {
    pub id: usize,
    pub state: UpdaterState,
    pub downloader: DownloadStatus,
}

pub(super) struct UpdaterBuilder {
    client: Option<reqwest::Client>,
    core_type: Option<ClashCore>,
    mirror: Option<String>,
    artifact: Option<String>,
    tag: Option<CoreTypeMeta>,
}

impl UpdaterBuilder {
    pub fn new() -> Self {
        Self {
            client: None,
            core_type: None,
            mirror: None,
            artifact: None,
            tag: None,
        }
    }

    pub fn set_client(mut self, client: reqwest::Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn set_core_type(mut self, core_type: ClashCore) -> Self {
        self.core_type = Some(core_type);
        self
    }

    pub fn set_artifact(mut self, artifact: String) -> Self {
        self.artifact = Some(artifact);
        self
    }

    pub fn set_tag(mut self, tag: CoreTypeMeta) -> Self {
        self.tag = Some(tag);
        self
    }

    pub fn set_mirror(mut self, mirror: String) -> Self {
        self.mirror = Some(mirror);
        self
    }

    pub async fn build(self) -> anyhow::Result<Updater> {
        let client = self.client.ok_or(anyhow::anyhow!("client is required"))?;
        let core_type = self
            .core_type
            .ok_or(anyhow::anyhow!("core_type is required"))?;
        let artifact = self
            .artifact
            .ok_or(anyhow::anyhow!("artifact is required"))?;
        let tag = self.tag.ok_or(anyhow::anyhow!("tag is required"))?;
        let mirror = self.mirror.ok_or(anyhow::anyhow!("mirror is required"))?;

        let temp_dir = TempDir::new()?;
        let inner = UpdaterInner {
            state: UpdaterState::Idle,
        };

        // setup downloader
        let download_path = shared::get_download_path(tag, &artifact);
        let mut download_url = url::Url::parse("https://github.com")?;
        download_url.set_path(&download_path);
        let download_url = crate::utils::candy::parse_gh_url(&mirror, download_url.as_str())?;
        let save_path = temp_dir.path().join(&artifact);
        tracing::debug!("downloader url: {}", download_url);
        tracing::debug!("downloader save path: {:?}", save_path);
        let downloader = Arc::new(DownloadSession::new(client, download_url, save_path).await?);
        Ok(Updater {
            id: rand::random::<u32>() as usize,
            temp_dir,
            core_type,
            inner: parking_lot::RwLock::new(inner),
            artifact,
            downloader,
        })
    }
}

fn is_core_archive_entry(file_name: &str) -> bool {
    let Some(base_name) = std::path::Path::new(file_name)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    let normalized = base_name.to_ascii_lowercase();

    #[cfg(target_os = "windows")]
    let stem = normalized.strip_suffix(".exe");
    #[cfg(not(target_os = "windows"))]
    let stem = (!normalized.contains('.')).then_some(normalized.as_str());

    stem.is_some_and(|stem| {
        stem == "mihomo"
            || stem.starts_with("mihomo-")
            || stem == "clash"
            || stem.starts_with("clash-")
            || stem == "chimera"
            || stem.starts_with("chimera-")
    })
}

fn ensure_core_size_allowed(size: u64) -> anyhow::Result<()> {
    if size == 0 {
        anyhow::bail!("core binary is empty");
    }
    if size > MAX_CORE_BINARY_SIZE {
        anyhow::bail!(
            "core binary exceeds maximum allowed size of {} bytes",
            MAX_CORE_BINARY_SIZE
        );
    }
    Ok(())
}

fn validate_core_binary(bytes: &[u8]) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        const PE_OFFSET_FIELD: usize = 0x3c;
        if !bytes.starts_with(b"MZ") || bytes.len() < PE_OFFSET_FIELD + 4 {
            anyhow::bail!("core binary is not a valid Windows PE executable");
        }
        let pe_offset = u32::from_le_bytes(
            bytes[PE_OFFSET_FIELD..PE_OFFSET_FIELD + 4]
                .try_into()
                .expect("PE offset field length is fixed"),
        ) as usize;
        if bytes.get(pe_offset..pe_offset.saturating_add(4)) != Some(b"PE\0\0") {
            anyhow::bail!("core binary is not a valid Windows PE executable");
        }
    }

    #[cfg(target_os = "linux")]
    if !bytes.starts_with(b"\x7fELF") {
        anyhow::bail!("core binary is not a valid Linux ELF executable");
    }

    #[cfg(target_os = "macos")]
    {
        const MACHO_MAGICS: [[u8; 4]; 6] = [
            [0xfe, 0xed, 0xfa, 0xce],
            [0xce, 0xfa, 0xed, 0xfe],
            [0xfe, 0xed, 0xfa, 0xcf],
            [0xcf, 0xfa, 0xed, 0xfe],
            [0xca, 0xfe, 0xba, 0xbe],
            [0xbe, 0xba, 0xfe, 0xca],
        ];
        if !MACHO_MAGICS.iter().any(|magic| bytes.starts_with(magic)) {
            anyhow::bail!("core binary is not a valid macOS Mach-O executable");
        }
    }

    Ok(())
}

fn extract_core_bytes(mut artifact_file: std::fs::File, artifact: &str) -> anyhow::Result<Vec<u8>> {
    let mut buffer = Vec::<u8>::new();
    let normalized_artifact = artifact.to_ascii_lowercase();
    match normalized_artifact.as_str() {
        name if name.ends_with(".gz") => {
            tracing::debug!("decompressing gz file");
            let decoder = flate2::read::GzDecoder::new(&mut artifact_file);
            std::io::copy(&mut decoder.take(MAX_CORE_BINARY_SIZE + 1), &mut buffer)?;
        }
        name if name.ends_with(".zip") => {
            tracing::debug!("decompressing zip file");
            let mut archive = zip::ZipArchive::new(artifact_file)?;
            let mut candidates = Vec::new();
            for index in 0..archive.len() {
                let file = archive.by_index(index)?;
                let file_name = file.name();
                tracing::debug!("Filename: {}", file_name);
                if is_core_archive_entry(file_name) && !file.is_dir() && file.size() > 0 {
                    ensure_core_size_allowed(file.size())?;
                    candidates.push(index);
                }
            }
            let candidate = match candidates.as_slice() {
                [] => anyhow::bail!("failed to find core file in a zip archive"),
                [candidate] => *candidate,
                _ => anyhow::bail!("multiple core files found in a zip archive"),
            };
            let mut file = archive.by_index(candidate)?;
            tracing::debug!("extract file: {}", file.name());
            tracing::debug!("extract file size: {}", file.size());
            std::io::copy(&mut (&mut file).take(MAX_CORE_BINARY_SIZE + 1), &mut buffer)?;
        }
        _ => {
            tracing::debug!("directly copying file");
            ensure_core_size_allowed(artifact_file.metadata()?.len())?;
            std::io::copy(
                &mut (&mut artifact_file).take(MAX_CORE_BINARY_SIZE + 1),
                &mut buffer,
            )?;
        }
    }
    ensure_core_size_allowed(buffer.len() as u64)?;
    validate_core_binary(&buffer)?;
    Ok(buffer)
}

impl Updater {
    fn dispatch_state(&self, state: UpdaterState) {
        tracing::debug!("dispatching updater state: {:?}", state);
        let mut inner = self.inner.write();
        inner.state = state;
    }

    async fn decompress_and_set_permission(&self) -> anyhow::Result<()> {
        self.dispatch_state(UpdaterState::Decompressing);
        let path = self.temp_dir.path().join(&self.artifact);
        tracing::debug!("decompressing file: {:?}", path);
        let tmp_file = std::fs::File::open(path)?;
        tracing::debug!("file size: {}", tmp_file.metadata()?.len());
        let artifact = self.artifact.clone();
        let buff =
            tokio::task::spawn_blocking(move || extract_core_bytes(tmp_file, &artifact)).await??;
        let tmp_core = self.temp_dir.path().join(format!(
            "{}{}",
            self.core_type,
            std::env::consts::EXE_SUFFIX
        ));
        tracing::debug!("writing core to {:?} ({} bytes)", tmp_core, buff.len());
        let mut core_file = tokio::fs::File::create(&tmp_core).await?;
        tokio::io::copy(&mut buff.as_slice(), &mut core_file).await?;
        #[cfg(target_family = "unix")]
        {
            std::fs::set_permissions(&tmp_core, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }

    async fn replace_core(&self) -> anyhow::Result<()> {
        ensure_updater_allowed()?;
        self.dispatch_state(UpdaterState::Replacing);
        let current_core = Config::verge().latest().clash_core.unwrap_or_default();
        tracing::debug!("current core: {}", current_core);
        if current_core == self.core_type {
            tracing::debug!("stopping core to replace");
            CoreManager::global().stop_core().await?;
        }
        #[cfg(target_os = "windows")]
        let target_core = format!("{}.exe", self.core_type);
        #[cfg(not(target_os = "windows"))]
        let target_core = self.core_type.clone().to_string();
        let core_dir = tauri::utils::platform::current_exe()?;
        let core_dir = core_dir.parent().ok_or(anyhow!("failed to get core dir"))?;
        let target_core = core_dir.join(target_core);
        tracing::debug!("copying core to {:?}", target_core);
        let tmp_core_path = self.temp_dir.path().join(format!(
            "{}{}",
            self.core_type,
            std::env::consts::EXE_SUFFIX
        ));
        let copy_source = tmp_core_path.clone();
        let copy_target = target_core.clone();
        let copy_result =
            tokio::task::spawn_blocking(move || replace_core_file(&copy_source, &copy_target))
                .await?;
        match copy_result {
            Ok(size) => {
                tracing::debug!("copied core to {:?} ({} bytes)", target_core, size);
            }
            Err(err) => {
                tracing::warn!(
                    "failed to copy core: {}, trying to use elevated permission to copy and override core",
                    err
                );
                tracing::debug!("tmp core path: {:?}", tmp_core_path);
                tracing::debug!("target core path: {:?}", target_core);
                // 防止 UAC 弹窗堵塞主线程
                let status_code = tokio::task::spawn_blocking(move || {
                    #[cfg(target_os = "windows")]
                    {
                        let backup = core_transaction_path(&target_core, "backup");
                        let script =
                            build_elevated_replace_script(&tmp_core_path, &target_core, &backup)?;
                        RunasCommand::new("powershell")
                            .args(&[
                                "-NoProfile",
                                "-NonInteractive",
                                "-ExecutionPolicy",
                                "Bypass",
                                "-Command",
                                &script,
                            ])
                            .status()
                            .map_err(anyhow::Error::from)
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        RunasCommand::new("cp")
                            .args(&[
                                "-f",
                                tmp_core_path.to_str().unwrap(),
                                target_core.to_str().unwrap(),
                            ])
                            .status()
                            .map_err(anyhow::Error::from)
                    }
                })
                .await??;
                if !status_code.success() {
                    anyhow::bail!("failed to copy core: {}", status_code);
                }
            }
        };

        if current_core == self.core_type {
            self.dispatch_state(UpdaterState::Restarting);
            CoreManager::global().run_core().await?;
        }

        Ok(())
    }

    pub async fn start(&self) {
        if let Err(error) = ensure_updater_allowed() {
            self.dispatch_state(UpdaterState::Failed(error.to_string()));
            return;
        }
        {
            let mut inner = self.inner.write();
            if !matches!(inner.state, UpdaterState::Idle) {
                return;
            }
            inner.state = UpdaterState::Downloading;
        }
        // The download engine reports live progress through `downloader.status()`,
        // which `get_report` surfaces to the frontend while this runs.
        if let Err(e) = self.downloader.start().await {
            tracing::error!("download failed: {}", e);
            self.dispatch_state(UpdaterState::Failed(e.to_string()));
            return;
        }
        tracing::debug!("download finished and start to incoming update logic");
        if let Err(e) = self.decompress_and_set_permission().await {
            tracing::error!("failed to decompress and set permission: {}", e);
            self.dispatch_state(UpdaterState::Failed(e.to_string()));
            return;
        }
        if let Err(e) = self.replace_core().await {
            tracing::error!("failed to replace core: {}", e);
            self.dispatch_state(UpdaterState::Failed(e.to_string()));
            return;
        }
        self.dispatch_state(UpdaterState::Done);
    }

    pub fn get_report(&self) -> UpdaterSummary {
        UpdaterSummary {
            id: self.id,
            state: self.inner.read().state.clone(),
            downloader: self.downloader.status(),
        }
    }

    pub fn get_updater_id(&self) -> usize {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, io::Write};

    #[cfg(target_os = "windows")]
    use std::process::Command;

    #[cfg(feature = "e2e")]
    use super::E2E_UPDATER_BLOCKED;
    #[cfg(target_os = "windows")]
    use super::build_elevated_replace_script;
    use super::{
        MAX_CORE_BINARY_SIZE, commit_staged_core_with, core_transaction_files,
        core_transaction_path, ensure_core_size_allowed, ensure_updater_allowed,
        extract_core_bytes, is_core_archive_entry, recover_interrupted_core_replace,
        recover_interrupted_core_replacements_in_dir, replace_core_file, replace_core_file_with,
        validate_core_binary,
    };

    fn valid_core_bytes() -> Vec<u8> {
        #[cfg(target_os = "windows")]
        {
            let mut bytes = vec![0_u8; 0x44];
            bytes[0..2].copy_from_slice(b"MZ");
            bytes[0x3c..0x40].copy_from_slice(&(0x40_u32).to_le_bytes());
            bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
            bytes
        }
        #[cfg(target_os = "linux")]
        {
            b"\x7fELFtest".to_vec()
        }
        #[cfg(target_os = "macos")]
        {
            vec![0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]
        }
    }

    #[cfg(target_os = "windows")]
    fn run_powershell_script(script: &str) -> std::process::Output {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .output()
            .expect("PowerShell must be available for elevated replacement tests")
    }

    #[test]
    fn corrupted_zip_is_rejected_before_core_file_creation() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("core.zip");
        std::fs::write(&archive_path, b"not a zip archive").unwrap();

        let error = extract_core_bytes(std::fs::File::open(archive_path).unwrap(), "core.zip")
            .expect_err("corrupted ZIP must fail");

        assert!(error.to_string().to_ascii_lowercase().contains("zip"));
        assert!(
            !temp
                .path()
                .join(format!("mihomo{}", std::env::consts::EXE_SUFFIX))
                .exists()
        );
    }

    #[test]
    fn zip_without_core_binary_is_rejected_before_core_file_creation() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("core.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file("README.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"documentation only").unwrap();
        archive.finish().unwrap();

        let error = extract_core_bytes(std::fs::File::open(archive_path).unwrap(), "core.zip")
            .expect_err("ZIP without a core binary must fail");

        assert_eq!(
            error.to_string(),
            "failed to find core file in a zip archive"
        );
        assert!(
            !temp
                .path()
                .join(format!("mihomo{}", std::env::consts::EXE_SUFFIX))
                .exists()
        );
    }

    #[test]
    fn keyword_matching_directory_is_rejected_as_core_binary() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("core.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .add_directory("chimera/", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.finish().unwrap();

        let error = extract_core_bytes(std::fs::File::open(archive_path).unwrap(), "core.zip")
            .expect_err("directory entry must not be treated as a core binary");

        assert_eq!(
            error.to_string(),
            "failed to find core file in a zip archive"
        );
    }

    #[test]
    fn empty_keyword_matching_file_is_rejected_as_core_binary() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("core.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                format!("mihomo{}", std::env::consts::EXE_SUFFIX),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.finish().unwrap();

        let error = extract_core_bytes(std::fs::File::open(archive_path).unwrap(), "core.zip")
            .expect_err("empty file must not be treated as a core binary");

        assert_eq!(
            error.to_string(),
            "failed to find core file in a zip archive"
        );
    }

    #[test]
    fn uppercase_platform_core_name_is_accepted() {
        let platform_name = format!(
            "MIHOMO{}",
            std::env::consts::EXE_SUFFIX.to_ascii_uppercase()
        );
        assert!(is_core_archive_entry(&format!("nested/{platform_name}")));
    }

    #[test]
    fn keyword_document_is_rejected_as_core_binary() {
        assert!(!is_core_archive_entry("not-chimera-document.txt"));
        assert!(!is_core_archive_entry("clash-release-notes.md"));
    }

    #[test]
    fn empty_direct_artifact_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let artifact_path = temp.path().join("mihomo");
        std::fs::File::create(&artifact_path).unwrap();

        let error = extract_core_bytes(std::fs::File::open(artifact_path).unwrap(), "mihomo")
            .expect_err("empty direct artifact must fail");

        assert_eq!(error.to_string(), "core binary is empty");
    }

    #[test]
    fn empty_gzip_artifact_is_rejected_after_decompression() {
        let temp = tempfile::tempdir().unwrap();
        let artifact_path = temp.path().join("mihomo.gz");
        let encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(&artifact_path).unwrap(),
            flate2::Compression::default(),
        );
        encoder.finish().unwrap();

        let error = extract_core_bytes(std::fs::File::open(artifact_path).unwrap(), "mihomo.gz")
            .expect_err("empty gzip payload must fail");

        assert_eq!(error.to_string(), "core binary is empty");
    }

    #[test]
    fn archive_extension_matching_is_case_insensitive() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("CORE.ZIP");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                format!(
                    "MIHOMO{}",
                    std::env::consts::EXE_SUFFIX.to_ascii_uppercase()
                ),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        let expected = valid_core_bytes();
        archive.write_all(&expected).unwrap();
        archive.finish().unwrap();

        let bytes = extract_core_bytes(std::fs::File::open(archive_path).unwrap(), "CORE.ZIP")
            .expect("uppercase ZIP extension must be recognized");

        assert_eq!(bytes, expected);
    }

    #[test]
    fn forged_executable_content_is_rejected() {
        let error = validate_core_binary(b"this is not an executable")
            .expect_err("arbitrary bytes must not be accepted as a core executable");

        assert!(error.to_string().contains("core binary is not a valid"));
    }

    #[test]
    fn platform_executable_header_is_accepted() {
        validate_core_binary(&valid_core_bytes())
            .expect("a structurally valid platform executable header must be accepted");
    }

    #[test]
    fn failed_atomic_commit_restores_previous_core() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("mihomo.exe");
        let staged = temp.path().join("mihomo.staged");
        let backup = core_transaction_path(&target, "backup");
        std::fs::write(&target, b"previous core").unwrap();
        std::fs::write(&staged, b"new core").unwrap();
        let call = Cell::new(0_u8);

        let error = commit_staged_core_with(&staged, &target, &backup, |from, to| {
            let current = call.get();
            call.set(current + 1);
            if current == 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected install failure",
                ));
            }
            std::fs::rename(from, to)
        })
        .expect_err("failed staged install must be reported");

        assert!(error.to_string().contains("injected install failure"));
        assert_eq!(std::fs::read(&target).unwrap(), b"previous core");
        assert!(!backup.exists());
        assert_eq!(std::fs::read(&staged).unwrap(), b"new core");
    }

    #[test]
    fn failed_replace_entry_restores_old_core_and_cleans_staging() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("downloaded-core");
        let target = temp.path().join("mihomo.exe");
        std::fs::write(&source, b"new core").unwrap();
        std::fs::write(&target, b"previous core").unwrap();
        let call = Cell::new(0_u8);

        let error = replace_core_file_with(&source, &target, |from, to| {
            let current = call.get();
            call.set(current + 1);
            if current == 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected promotion failure",
                ));
            }
            std::fs::rename(from, to)
        })
        .expect_err("production replacement entry must report promotion failure");

        assert!(error.to_string().contains("injected promotion failure"));
        assert_eq!(std::fs::read(&target).unwrap(), b"previous core");
        assert_eq!(std::fs::read(&source).unwrap(), b"new core");
        assert!(
            core_transaction_files(&target, "staged")
                .unwrap()
                .is_empty()
        );
        assert!(
            core_transaction_files(&target, "backup")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn successful_atomic_replace_cleans_staging_and_backup_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("downloaded-core");
        let target = temp.path().join("mihomo.exe");
        std::fs::write(&source, b"new core").unwrap();
        std::fs::write(&target, b"previous core").unwrap();

        let size = replace_core_file(&source, &target).expect("atomic replacement must succeed");

        assert_eq!(size, 8);
        assert_eq!(std::fs::read(&target).unwrap(), b"new core");
        assert!(
            core_transaction_files(&target, "backup")
                .unwrap()
                .is_empty()
        );
        assert!(
            core_transaction_files(&target, "staged")
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn elevated_replace_script_replaces_core_and_cleans_transaction_files() {
        let temp = tempfile::Builder::new()
            .prefix("chimera updater 'quoted' ")
            .tempdir()
            .unwrap();
        let source = temp.path().join("downloaded core.exe");
        let target = temp.path().join("mihomo core.exe");
        let stale_staged = core_transaction_path(&target, "staged");
        let stale_backup = core_transaction_path(&target, "backup");
        let backup = core_transaction_path(&target, "backup");
        std::fs::write(&source, b"new core").unwrap();
        std::fs::write(&target, b"previous core").unwrap();
        std::fs::write(&stale_staged, b"partial core").unwrap();
        std::fs::write(&stale_backup, b"stale backup").unwrap();

        let script = build_elevated_replace_script(&source, &target, &backup).unwrap();
        let output = run_powershell_script(&script);

        assert!(
            output.status.success(),
            "elevated replacement script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"new core");
        assert_eq!(std::fs::read(&source).unwrap(), b"new core");
        assert!(
            core_transaction_files(&target, "staged")
                .unwrap()
                .is_empty()
        );
        assert!(
            core_transaction_files(&target, "backup")
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn elevated_replace_script_restores_interrupted_backup_when_copy_fails() {
        let temp = tempfile::tempdir().unwrap();
        let missing_source = temp.path().join("missing core.exe");
        let target = temp.path().join("mihomo.exe");
        let interrupted_backup = core_transaction_path(&target, "backup");
        let transaction_backup = core_transaction_path(&target, "backup");
        std::fs::write(&interrupted_backup, b"previous core").unwrap();

        let script =
            build_elevated_replace_script(&missing_source, &target, &transaction_backup).unwrap();
        let output = run_powershell_script(&script);

        assert!(!output.status.success());
        assert_eq!(std::fs::read(&target).unwrap(), b"previous core");
        assert!(
            core_transaction_files(&target, "backup")
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn elevated_replace_script_rejects_ambiguous_interrupted_backups() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("downloaded-core.exe");
        let target = temp.path().join("mihomo.exe");
        let first_backup = core_transaction_path(&target, "backup");
        let second_backup = core_transaction_path(&target, "backup");
        let transaction_backup = core_transaction_path(&target, "backup");
        std::fs::write(&source, b"new core").unwrap();
        std::fs::write(&first_backup, b"older core").unwrap();
        std::fs::write(&second_backup, b"newer core").unwrap();

        let script = build_elevated_replace_script(&source, &target, &transaction_backup).unwrap();
        let output = run_powershell_script(&script);

        assert!(!output.status.success());
        assert!(!target.exists());
        assert_eq!(std::fs::read(&first_backup).unwrap(), b"older core");
        assert_eq!(std::fs::read(&second_backup).unwrap(), b"newer core");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("multiple interrupted core backups found")
        );
    }

    #[test]
    fn transaction_paths_are_unique_for_the_same_core() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("mihomo.exe");

        let first_staged = core_transaction_path(&target, "staged");
        let second_staged = core_transaction_path(&target, "staged");
        let backup = core_transaction_path(&target, "backup");

        assert_ne!(first_staged, second_staged);
        assert_ne!(first_staged, backup);
        assert_ne!(second_staged, backup);
    }

    #[test]
    fn interrupted_backup_is_restored_and_stale_staging_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("mihomo.exe");
        let backup = core_transaction_path(&target, "backup");
        let staged = core_transaction_path(&target, "staged");
        std::fs::write(&backup, b"previous core").unwrap();
        std::fs::write(&staged, b"partial new core").unwrap();

        recover_interrupted_core_replace(&target)
            .expect("a single interrupted backup must be restored");

        assert_eq!(std::fs::read(&target).unwrap(), b"previous core");
        assert!(!backup.exists());
        assert!(!staged.exists());
    }

    #[test]
    fn launch_recovery_restores_all_known_core_targets() {
        let temp = tempfile::tempdir().unwrap();
        let mihomo_target = temp
            .path()
            .join(format!("mihomo{}", std::env::consts::EXE_SUFFIX));
        let clash_rs_target = temp
            .path()
            .join(format!("clash-rs{}", std::env::consts::EXE_SUFFIX));
        let mihomo_backup = core_transaction_path(&mihomo_target, "backup");
        let clash_rs_backup = core_transaction_path(&clash_rs_target, "backup");
        let stale_stage = core_transaction_path(&mihomo_target, "staged");
        std::fs::write(&mihomo_backup, b"mihomo previous core").unwrap();
        std::fs::write(&clash_rs_backup, b"clash-rs previous core").unwrap();
        std::fs::write(&stale_stage, b"partial core").unwrap();

        recover_interrupted_core_replacements_in_dir(temp.path(), &["mihomo", "clash-rs"])
            .expect("launch recovery must restore each known core independently");

        assert_eq!(
            std::fs::read(&mihomo_target).unwrap(),
            b"mihomo previous core"
        );
        assert_eq!(
            std::fs::read(&clash_rs_target).unwrap(),
            b"clash-rs previous core"
        );
        assert!(!mihomo_backup.exists());
        assert!(!clash_rs_backup.exists());
        assert!(!stale_stage.exists());
    }

    #[test]
    fn multiple_interrupted_backups_are_rejected_without_guessing() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("mihomo.exe");
        let first_backup = core_transaction_path(&target, "backup");
        let second_backup = core_transaction_path(&target, "backup");
        std::fs::write(&first_backup, b"older core").unwrap();
        std::fs::write(&second_backup, b"newer core").unwrap();

        let error = recover_interrupted_core_replace(&target)
            .expect_err("ambiguous interrupted backups must not be guessed");

        assert!(
            error
                .to_string()
                .contains("multiple interrupted core backups")
        );
        assert!(!target.exists());
        assert!(first_backup.exists());
        assert!(second_backup.exists());
    }

    #[test]
    fn zip_with_multiple_core_candidates_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("core.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        for name in [
            format!("mihomo{}", std::env::consts::EXE_SUFFIX),
            format!("clash{}", std::env::consts::EXE_SUFFIX),
        ] {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(b"binary").unwrap();
        }
        archive.finish().unwrap();

        let error = extract_core_bytes(std::fs::File::open(archive_path).unwrap(), "core.zip")
            .expect_err("ambiguous ZIP must be rejected");

        assert_eq!(
            error.to_string(),
            "multiple core files found in a zip archive"
        );
    }

    #[test]
    fn oversized_declared_core_is_rejected_before_allocation() {
        let error = ensure_core_size_allowed(MAX_CORE_BINARY_SIZE + 1)
            .expect_err("oversized core must be rejected");

        assert!(error.to_string().contains("exceeds maximum allowed size"));
        ensure_core_size_allowed(MAX_CORE_BINARY_SIZE)
            .expect("maximum-sized core must remain allowed");
    }

    #[cfg(feature = "e2e")]
    #[test]
    fn e2e_updater_is_rejected_before_download_or_host_mutation() {
        let error = ensure_updater_allowed().expect_err("E2E updater must be disabled");
        assert_eq!(error.to_string(), E2E_UPDATER_BLOCKED);
    }

    #[cfg(not(feature = "e2e"))]
    #[test]
    fn production_updater_guard_allows_normal_execution() {
        ensure_updater_allowed().expect("production updater must remain enabled");
    }
}

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::utils::dirs::app_data_dir;

use super::super::{
    history::{AgentHistoryDocument, MAX_CORRUPT_HISTORY_FILES, normalize_history_document},
    ports::AgentHistoryPersistencePort,
};

const HISTORY_FILE: &str = "agent-history.json";
pub(crate) const MAX_HISTORY_FILE_BYTES: u64 = 1024 * 1024;
const HISTORY_BLOCKING_IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct HistoryBlockingIo {
    gate: Arc<tokio::sync::Semaphore>,
    timeout: Duration,
}

impl HistoryBlockingIo {
    fn new() -> Self {
        Self {
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout: HISTORY_BLOCKING_IO_TIMEOUT,
        }
    }

    async fn run<T, F>(&self, operation: F) -> std::io::Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> std::io::Result<T> + Send + 'static,
    {
        let permit = tokio::time::timeout(self.timeout, self.gate.clone().acquire_owned())
            .await
            .map_err(|_| history_io_timeout("history I/O gate timed out"))?
            .map_err(|_| std::io::Error::other("history I/O gate closed"))?;
        tokio::time::timeout(
            self.timeout,
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                operation()
            }),
        )
        .await
        .map_err(|_| history_io_timeout("history I/O task timed out"))?
        .map_err(|_| std::io::Error::other("history I/O task failed"))?
    }
}

fn history_io_timeout(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, message)
}

pub(crate) struct FsAgentHistoryPersistence {
    path: PathBuf,
    blocking_io: HistoryBlockingIo,
}

impl FsAgentHistoryPersistence {
    pub(crate) fn from_app_data_dir() -> anyhow::Result<Self> {
        Ok(Self {
            path: app_data_dir()?.join(HISTORY_FILE),
            blocking_io: HistoryBlockingIo::new(),
        })
    }
}

#[async_trait::async_trait]
impl AgentHistoryPersistencePort for FsAgentHistoryPersistence {
    async fn load(&self) -> anyhow::Result<AgentHistoryDocument> {
        read_document_from_with_io(&self.path, &self.blocking_io).await
    }

    async fn save(&self, document: &AgentHistoryDocument) -> anyhow::Result<()> {
        write_document_to_with_io(&self.path, document, &self.blocking_io).await
    }
}

async fn read_document_from_with_io(
    path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<AgentHistoryDocument> {
    recover_temporary_document_with_io(path, blocking_io).await?;
    let bytes = match read_private_history_file(path, blocking_io).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error.into()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(mut document) => {
            normalize_history_document(&mut document);
            Ok(document)
        }
        Err(error) => {
            quarantine_corrupt_document(path, blocking_io).await?;
            Err(error.into())
        }
    }
}

async fn recover_temporary_document_with_io(
    path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let temporary = path.with_extension("json.tmp");
    if !path_entry_exists(&temporary).await? {
        return Ok(());
    }
    if path_entry_exists(path).await? {
        discard_temporary_history(&temporary, blocking_io).await?;
        return Ok(());
    }

    let metadata = tokio::fs::symlink_metadata(&temporary).await?;
    if !metadata.file_type().is_file() {
        discard_temporary_history(&temporary, blocking_io).await?;
        return Ok(());
    }

    let bytes = read_private_history_file(&temporary, blocking_io).await?;
    if serde_json::from_slice::<AgentHistoryDocument>(&bytes).is_ok() {
        promote_temporary_history(&temporary, path, blocking_io).await?;
    } else {
        discard_temporary_history(&temporary, blocking_io).await?;
    }
    Ok(())
}

async fn quarantine_corrupt_document(
    path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let quarantined = path.with_file_name(format!("agent-history.corrupt-{timestamp}.json"));
    move_history_file(path, &quarantined, blocking_io).await?;
    prune_corrupt_documents_with_io(path, blocking_io).await
}

async fn prune_corrupt_documents_with_io(
    history_path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let parent = history_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("history path has no parent"))?;
    let mut directory = tokio::fs::read_dir(parent).await?;
    let mut retained = Vec::with_capacity(MAX_CORRUPT_HISTORY_FILES + 1);
    let mut removed = false;
    while let Some(entry) = directory.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("agent-history.corrupt-")
            && name.ends_with(".json")
            && entry.file_type().await?.is_file()
        {
            retained.push((name, entry.path()));
            retained.sort_by(|left, right| left.0.cmp(&right.0));
            if retained.len() > MAX_CORRUPT_HISTORY_FILES {
                let (_, path) = retained.remove(0);
                tokio::fs::remove_file(path).await?;
                removed = true;
            }
        }
    }
    if removed {
        sync_parent_directory(history_path, blocking_io).await?;
    }
    for (_, path) in &retained {
        restrict_history_permissions(path, blocking_io).await?;
    }
    Ok(())
}

async fn write_document_to_with_io(
    path: &Path,
    document: &AgentHistoryDocument,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(document)?;
    if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
        return Err(anyhow::anyhow!("history document exceeds size limit"));
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("history path has no parent"))?;
    tokio::fs::create_dir_all(parent).await?;
    recover_temporary_document_with_io(path, blocking_io).await?;

    let temporary = path.with_extension("json.tmp");
    write_private_history_file_with_io(&temporary, bytes, blocking_io).await?;
    if let Err(error) = replace_history_file(&temporary, path, blocking_io).await {
        let primary_exists = match path_entry_exists(path).await {
            Ok(exists) => exists,
            Err(existence_error) => {
                discard_temporary_history(&temporary, blocking_io).await?;
                return Err(existence_error.into());
            }
        };
        if primary_exists {
            discard_temporary_history(&temporary, blocking_io).await?;
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
async fn replace_history_file(
    source: &Path,
    destination: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    move_history_file_windows(source, destination, true, blocking_io).await
}

#[cfg(not(windows))]
async fn replace_history_file(
    source: &Path,
    destination: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    move_history_file(source, destination, blocking_io).await
}

#[cfg(windows)]
async fn promote_temporary_history(
    source: &Path,
    destination: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    move_history_file_windows(source, destination, false, blocking_io).await
}

#[cfg(not(windows))]
async fn promote_temporary_history(
    source: &Path,
    destination: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    move_history_file(source, destination, blocking_io).await
}

#[cfg(windows)]
async fn move_history_file(
    source: &Path,
    destination: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    move_history_file_windows(source, destination, false, blocking_io).await
}

#[cfg(not(windows))]
async fn move_history_file(
    source: &Path,
    destination: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    tokio::fs::rename(source, destination).await?;
    sync_parent_directory(destination, blocking_io).await
}

#[cfg(windows)]
async fn move_history_file_windows(
    source: &Path,
    destination: &Path,
    replace_existing: bool,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let source = source.to_owned();
    let destination = destination.to_owned();
    blocking_io
        .run(move || -> std::io::Result<()> {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };

            let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
            let destination: Vec<u16> = destination
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            let mut flags = MOVEFILE_WRITE_THROUGH;
            if replace_existing {
                flags |= MOVEFILE_REPLACE_EXISTING;
            }
            let succeeded = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) };
            if succeeded == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        })
        .await?;
    Ok(())
}

async fn write_private_history_file_with_io(
    path: &Path,
    bytes: Vec<u8>,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let path = path.to_owned();
    blocking_io
        .run(move || -> std::io::Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path)?;
            let result = (|| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                }
                file.write_all(&bytes)?;
                file.flush()?;
                file.sync_all()
            })();
            drop(file);
            if result.is_err() {
                let _ = std::fs::remove_file(&path);
            }
            result
        })
        .await?;
    Ok(())
}

async fn read_private_history_file(
    path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> std::io::Result<Vec<u8>> {
    let path = path.to_owned();
    blocking_io
        .run(move || {
            let file = open_private_history_file(&path)?;
            let mut bytes = Vec::new();
            file.take(MAX_HISTORY_FILE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_HISTORY_FILE_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "history file exceeds the allowed size",
                ));
            }
            Ok(bytes)
        })
        .await
}

fn open_private_history_file(path: &Path) -> std::io::Result<std::fs::File> {
    let expected = std::fs::symlink_metadata(path)?;
    if !is_regular_history_metadata(&expected) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "history path is not a regular file",
        ));
    }
    if expected.len() > MAX_HISTORY_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "history file exceeds the allowed size",
        ));
    }

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !is_regular_history_metadata(&opened) || opened.len() > MAX_HISTORY_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "history path changed or exceeds the allowed size",
        ));
    }
    #[cfg(unix)]
    if !same_history_file(&expected, &opened) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "history path changed while opening",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn is_regular_history_metadata(metadata: &std::fs::Metadata) -> bool {
    if !metadata.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
    }
    #[cfg(not(windows))]
    true
}

#[cfg(unix)]
fn same_history_file(expected: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    expected.dev() == opened.dev() && expected.ino() == opened.ino()
}

pub(crate) async fn path_entry_exists(path: &Path) -> std::io::Result<bool> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

async fn discard_temporary_history(
    path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => sync_parent_directory(path, blocking_io).await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn restrict_history_permissions(
    path: &Path,
    blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    let path = path.to_owned();
    blocking_io
        .run(move || open_private_history_file(&path).map(drop))
        .await?;
    Ok(())
}

#[cfg(unix)]
async fn sync_parent_directory(path: &Path, blocking_io: &HistoryBlockingIo) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("history path has no parent"))?
        .to_owned();
    blocking_io
        .run(move || std::fs::File::open(parent)?.sync_all())
        .await?;
    Ok(())
}

#[cfg(not(unix))]
async fn sync_parent_directory(
    _path: &Path,
    _blocking_io: &HistoryBlockingIo,
) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
pub(crate) async fn read_document_from(path: &Path) -> anyhow::Result<AgentHistoryDocument> {
    read_document_from_with_io(path, &HistoryBlockingIo::new()).await
}

#[cfg(test)]
pub(crate) async fn recover_temporary_document(path: &Path) -> anyhow::Result<()> {
    recover_temporary_document_with_io(path, &HistoryBlockingIo::new()).await
}

#[cfg(test)]
pub(crate) async fn prune_corrupt_documents(history_path: &Path) -> anyhow::Result<()> {
    prune_corrupt_documents_with_io(history_path, &HistoryBlockingIo::new()).await
}

#[cfg(test)]
pub(crate) async fn write_document_to(
    path: &Path,
    document: &AgentHistoryDocument,
) -> anyhow::Result<()> {
    write_document_to_with_io(path, document, &HistoryBlockingIo::new()).await
}

#[cfg(test)]
pub(crate) async fn write_private_history_file(path: &Path, bytes: Vec<u8>) -> anyhow::Result<()> {
    write_private_history_file_with_io(path, bytes, &HistoryBlockingIo::new()).await
}

#[cfg(test)]
mod blocking_io_tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::HistoryBlockingIo;

    #[tokio::test]
    async fn timed_out_task_retains_the_single_flight_permit_until_it_finishes() {
        let blocking_io = HistoryBlockingIo {
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout: Duration::from_millis(20),
        };
        let first = blocking_io
            .run(|| {
                std::thread::sleep(Duration::from_millis(120));
                Ok(())
            })
            .await
            .expect_err("first task must time out");
        assert_eq!(first.kind(), std::io::ErrorKind::TimedOut);

        let second_started = Arc::new(AtomicUsize::new(0));
        let second_started_for_task = second_started.clone();
        let second = blocking_io
            .run(move || {
                second_started_for_task.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .expect_err("second task must time out while waiting for the retained permit");
        assert_eq!(second.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(second_started.load(Ordering::SeqCst), 0);

        tokio::time::sleep(Duration::from_millis(140)).await;
        blocking_io
            .run(|| Ok(()))
            .await
            .expect("gate must reopen after the detached task exits");
    }
}

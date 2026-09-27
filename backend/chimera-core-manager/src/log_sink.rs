//! Durable JSONL archive of the manager's core-log stream.
//!
//! One rolling file per writer run in `{runtime_dir}/logs/`, rotated by size and
//! retained by count. The writer is an ordinary [`broadcast`] subscriber, so it
//! never touches the supervision loop that produces the frames
//! (`instance.rs`'s `publish_log_frame`, which runs inline on that loop): a slow
//! disk costs this task its own backlog and nothing else.
//!
//! The on-disk contract, in full:
//!
//! - append-only, line-delimited JSON, one object per line, never rewritten and
//!   never renamed;
//! - every record carries `t` (`"log"` or `"gap"`) and `at`, a unix-millisecond
//!   instant. On a log record it is the parser's observation of the record's
//!   root line; on a gap record it is the writer's own. `at` is the only
//!   sortable clock: a frame whose header did not parse has no timestamp at all,
//!   and clash premium's and clash-rs's are inferred;
//! - a crash can only truncate the **last** line, because each batch is one
//!   `write_all`. A reader splits on `\n`, parses each line, and ignores a
//!   trailing line that does not parse;
//! - there is no `fsync`. This is diagnostic data, deliberately unlike
//!   `RuntimeConfigStore`, which pays the full stage/fsync/replace price because
//!   it holds authoritative configuration.

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use tokio::{
    io::AsyncWriteExt,
    sync::broadcast::{
        Receiver,
        error::{RecvError, TryRecvError},
    },
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Error,
    log::LogFrame,
    // `crate::runtime_store`, not `crate::config::runtime_store`: `lib.rs:19`
    // re-exports it and `manager/mod.rs:26` already spells it this way.
    runtime_store::validate_directory_metadata,
    state::now_ms,
};

/// Subdirectory of the manager's runtime directory. Deliberately not a sibling
/// of the epoch artifacts: `cleanup_epoch` and `artifact_epochs` scan the
/// runtime directory by filename prefix, and a log file's useful life starts
/// exactly where an epoch artifact's ends.
const LOG_DIR_NAME: &str = "logs";

const FILE_PREFIX: &str = "core-";
const FILE_SUFFIX: &str = ".jsonl";

const MAX_BATCH_RECORDS: usize = 256;

/// Rotation knobs, mirrored from `ManagerOptions` so the writer does not carry
/// the whole option bag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SinkOptions {
    pub max_bytes: u64,
    pub max_files: usize,
}

pub(crate) struct SinkHandle {
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl SinkHandle {
    /// An aborted writer can cut the final batch mid-write, the same contract
    /// as a crash; readers already discard a truncated last line.
    pub(crate) async fn shutdown(mut self) {
        self.cancel.cancel();
        match tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await {
            Ok(_) => {}
            Err(_) => {
                tracing::warn!("timed out waiting for the core log sink to shut down");
                self.task.abort();
                let _ = self.task.await;
            }
        }
    }
}

/// A `"log"` line: the frame verbatim, behind the one key that discriminates a
/// record's type.
///
/// The frame is written in place rather than projected, so the disk schema *is*
/// [`LogFrame`]'s field order and every key stays present, nulls included — a
/// reader never has to branch on the schema. `t` is reserved by this envelope:
/// a frame field of that name would produce a duplicate JSON key, which the
/// envelope test below is there to catch.
#[derive(Serialize)]
struct LogRecord<'a> {
    t: &'static str,
    #[serde(flatten)]
    frame: &'a LogFrame,
}

impl<'a> LogRecord<'a> {
    fn new(frame: &'a LogFrame) -> Self {
        Self { t: "log", frame }
    }
}

/// A `"gap"` line: the writer fell behind and the broadcast dropped `dropped`
/// frames on its behalf. Writing it turns a silent hole into a visible one.
/// Unlike a log record's, its `at` is the writer's own observation — there is no
/// frame behind it to have been parsed.
#[derive(Serialize)]
struct GapRecord {
    t: &'static str,
    at: i64,
    dropped: u64,
}

impl GapRecord {
    fn new(dropped: u64, at: i64) -> Self {
        Self {
            t: "gap",
            at,
            dropped,
        }
    }
}

/// Creates and hardens `{parent}/logs`, returning its path.
///
/// The DACL is applied here rather than inherited: the parent grants
/// `OICI` inheritance, but an inherited descriptor does not carry
/// `SE_DACL_PROTECTED`, which is exactly what `verify_windows_directory_acl`
/// requires. A directory this fails on is a construction failure, the same class
/// as a runtime directory that cannot be hardened.
pub(crate) async fn prepare_dir(parent: &Utf8Path) -> Result<Utf8PathBuf, Error> {
    let dir = parent.join(LOG_DIR_NAME);
    match tokio::fs::symlink_metadata(&dir).await {
        Ok(metadata) => validate_directory_metadata(&dir, &metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(&dir).await?;
        }
        Err(error) => return Err(error.into()),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).await?;
    }
    #[cfg(windows)]
    {
        nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&dir)?;
        nyanpasu_utils::io::atomic_fs::verify_windows_directory_acl(&dir)?;
    }

    Ok(dir)
}

/// Opens the first file and hands the writer to a background task.
///
/// Opening before spawning is deliberate: a directory that cannot be written is
/// a `CoreManager::new` failure, not a task that dies quietly three seconds
/// later.
///
/// Dropping the manager without calling `shutdown()` abandons at most the final
/// batch. This is diagnostic data and best-effort by design; `shutdown()` is
/// the graceful path.
pub(crate) async fn spawn(
    dir: Utf8PathBuf,
    options: SinkOptions,
    logs: Receiver<Arc<LogFrame>>,
    cancel: CancellationToken,
) -> Result<SinkHandle, Error> {
    let writer = Writer::open(dir, options).await?;
    let task = tokio::spawn(run(writer, logs, cancel.clone()));
    Ok(SinkHandle { cancel, task })
}

/// Drains the broadcast in batches until the token is cancelled or the last
/// sender is gone. One `recv().await` for the first frame, then `try_recv()`
/// until empty: a start-up burst costs one write instead of one per line.
async fn run(mut writer: Writer, mut logs: Receiver<Arc<LogFrame>>, cancel: CancellationToken) {
    let mut batch = Vec::new();
    loop {
        // Cleared before the select, never after: the cancellation arm writes
        // this same buffer, and a stale batch there would be written twice.
        batch.clear();
        let first = tokio::select! {
            _ = cancel.cancelled() => {
                let closed = drain(&mut logs, &mut batch);
                writer.write(&batch).await;
                if closed || batch.len() < MAX_BATCH_RECORDS {
                    break;
                }
                continue;
            }
            received = logs.recv() => received,
        };
        let closed = match first {
            Ok(frame) => {
                batch.push(Entry::Log(frame));
                false
            }
            Err(RecvError::Lagged(dropped)) => {
                batch.push(Entry::Gap(dropped));
                false
            }
            Err(RecvError::Closed) => true,
        };
        // Short-circuits deliberately: `Closed` from `recv` already means the
        // ring is empty, so there is nothing left for `drain` to find.
        let closed = closed || drain(&mut logs, &mut batch);
        writer.write(&batch).await;
        if closed {
            break;
        }
    }
}

enum Entry {
    Log(Arc<LogFrame>),
    Gap(u64),
}

/// Appends everything already buffered. `true` means the channel is closed and
/// the caller must stop after writing what it has.
fn drain(logs: &mut Receiver<Arc<LogFrame>>, batch: &mut Vec<Entry>) -> bool {
    while batch.len() < MAX_BATCH_RECORDS {
        match logs.try_recv() {
            Ok(frame) => batch.push(Entry::Log(frame)),
            Err(TryRecvError::Lagged(dropped)) => batch.push(Entry::Gap(dropped)),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Closed) => return true,
        }
    }
    false
}

struct Writer {
    dir: Utf8PathBuf,
    options: SinkOptions,
    file: tokio::fs::File,
    written: u64,
    seq: u64,
}

impl Writer {
    async fn open(dir: Utf8PathBuf, options: SinkOptions) -> Result<Self, Error> {
        // A new file per run, never an append: a crash can leave the previous
        // file's last line half-written, and appending would glue it to a fresh
        // record. One file also means one run, which is a useful boundary when
        // somebody hands you a log directory.
        let seq = next_seq(&dir).await?;
        prune_dir(&dir, options.max_files.saturating_sub(1), None).await;
        let file = create(&dir, seq).await?;
        Ok(Self {
            dir,
            options,
            file,
            written: 0,
            seq,
        })
    }

    /// Serializes and appends one batch, rotating whenever the active file has
    /// already crossed the limit. The check runs per record, not per batch, so
    /// a burst that arrives as one batch still cannot produce a file more than
    /// one record past `max_bytes` — a record being bounded by the caps the
    /// manager's parser applied before it ever reached this channel. That holds
    /// for as long as rollover keeps succeeding: `rotate` deliberately keeps
    /// writing into the oversized file when it cannot open a new one, so
    /// repeated failures overshoot by a record each. Errors are logged and
    /// swallowed: losing diagnostic output must never take down a manager that
    /// is otherwise healthy.
    async fn write(&mut self, batch: &[Entry]) {
        if batch.is_empty() {
            return;
        }
        let at = now_ms();
        let mut buffer = Vec::new();
        for entry in batch {
            // Rotate before the record that would land past the limit,
            // counting both what is on disk and what is still buffered for the
            // active file.
            if self.written + buffer.len() as u64 >= self.options.max_bytes {
                self.flush_pending(&mut buffer).await;
                self.rotate().await;
            }
            let mark = buffer.len();
            let result = match entry {
                Entry::Log(frame) => serde_json::to_writer(&mut buffer, &LogRecord::new(frame)),
                Entry::Gap(dropped) => {
                    serde_json::to_writer(&mut buffer, &GapRecord::new(*dropped, at))
                }
            };
            if let Err(error) = result {
                buffer.truncate(mark);
                tracing::error!("failed to serialize a core log record: {error}");
                continue;
            }
            buffer.push(b'\n');
        }
        self.flush_pending(&mut buffer).await;
    }

    /// Appends whatever is buffered to the active file and clears the buffer.
    /// On failure the pending records are dropped (logged), matching the
    /// policy that disk trouble never backs up into the manager.
    async fn flush_pending(&mut self, buffer: &mut Vec<u8>) {
        if buffer.is_empty() {
            return;
        }
        if let Err(error) = self.file.write_all(buffer).await {
            tracing::error!("failed to write core log records: {error}");
            buffer.clear();
            return;
        }
        // tokio's File buffers, so without this the bytes are not on their way
        // to the OS and the file is not tailable.
        if let Err(error) = self.file.flush().await {
            tracing::error!("failed to flush core log records: {error}");
        }
        self.written += buffer.len() as u64;
        buffer.clear();
    }

    /// A single-file budget briefly permits the old and new files to coexist:
    /// the active file is never deleted, and retention converges after the
    /// successful switch closes it.
    async fn rotate(&mut self) {
        let seq = self.seq + 1;
        prune_dir(
            &self.dir,
            self.options.max_files.saturating_sub(1),
            Some(self.seq),
        )
        .await;
        match create(&self.dir, seq).await {
            Ok(file) => {
                self.file = file;
                self.seq = seq;
                self.written = 0;
                prune_dir(&self.dir, self.options.max_files, Some(self.seq)).await;
            }
            // Keep writing into the oversized file rather than losing the
            // stream; the next record's pre-check retries.
            Err(error) => {
                tracing::error!("failed to roll over the core log file: {error}");
            }
        }
    }
}

/// Deletes everything past the newest `keep` files except `protect`. A delete
/// that fails — a file held open by an editor, which Windows refuses to unlink
/// — is logged and retried on the next rollover.
async fn prune_dir(dir: &Utf8Path, keep: usize, protect: Option<u64>) {
    let existing = match read_seqs(dir).await {
        Ok(seqs) => seqs,
        Err(error) => {
            tracing::warn!("failed to list the core log directory: {error}");
            return;
        }
    };
    for seq in prune_targets(existing, keep)
        .into_iter()
        .filter(|seq| Some(*seq) != protect)
    {
        let path = dir.join(file_name(seq));
        if let Err(error) = tokio::fs::remove_file(&path).await {
            tracing::warn!("failed to prune the rotated core log file {path}: {error}");
        }
    }
}

async fn create(dir: &Utf8Path, seq: u64) -> Result<tokio::fs::File, Error> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(dir.join(file_name(seq))).await?)
}

fn file_name(seq: u64) -> String {
    format!("{FILE_PREFIX}{seq:06}{FILE_SUFFIX}")
}

/// `None` for anything this writer did not create, so an unrelated file in the
/// directory is never a rotation candidate.
fn file_seq(name: &str) -> Option<u64> {
    name.strip_prefix(FILE_PREFIX)
        .and_then(|rest| rest.strip_suffix(FILE_SUFFIX))
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

async fn read_seqs(dir: &Utf8Path) -> Result<Vec<u64>, std::io::Error> {
    let mut seqs = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        // Bound before borrowing, the same shape `cleanup_epoch` uses: the
        // `OsString` has to outlive the `&str` taken out of it.
        let name = entry.file_name();
        if let Some(seq) = name.to_str().and_then(file_seq) {
            seqs.push(seq);
        }
    }
    Ok(seqs)
}

async fn next_seq(dir: &Utf8Path) -> Result<u64, Error> {
    let seqs = read_seqs(dir).await?;
    Ok(seqs.into_iter().max().unwrap_or(0) + 1)
}

/// Newest `max_files` survive; the rest are returned oldest-last. Ordering is
/// numeric, not lexicographic: the zero padding is six wide for readability and
/// stops agreeing with string order at a million files.
fn prune_targets(mut seqs: Vec<u64>, max_files: usize) -> Vec<u64> {
    seqs.sort_unstable_by_key(|seq| std::cmp::Reverse(*seq));
    seqs.into_iter().skip(max_files).collect()
}

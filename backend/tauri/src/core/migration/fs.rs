//! Crash-safe filesystem writes for config migrations.

use anyhow::{Context, ensure};
use atomicwrites::{AllowOverwrite, AtomicFile};
use std::{io::Write, path::Path};

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    ensure!(
        path.file_name().is_some(),
        "destination path has no file name: {}",
        path.display()
    );
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir {}", parent.display()))?;
    }
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| file.write_all(contents))
        .with_context(|| format!("failed to atomically write {}", path.display()))?;
    Ok(())
}

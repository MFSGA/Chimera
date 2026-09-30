use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::utils::dirs::app_data_dir;

const TRANSACTION_FILE: &str = "update-transaction.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Downloading,
    Decompressing,
    Replacing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTransaction {
    pub phase: UpdatePhase,
    pub core_type: String,
    pub updated_at: i64,
}

fn path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(TRANSACTION_FILE))
}

pub fn write(transaction: &UpdateTransaction) -> Result<()> {
    let content = serde_json::to_string_pretty(transaction)?;
    std::fs::write(path()?, content)?;
    Ok(())
}

pub fn clear() -> Result<()> {
    let path = path()?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn read() -> Result<Option<UpdateTransaction>> {
    let path = path()?;
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
}

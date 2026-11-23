//! Shared migration state and revision-selection policy, independent of Tauri.
//! The application shell injects paths, backup, and module implementations.

pub mod context;
pub mod document;
pub mod fs;
pub mod module;
pub mod registry;
pub mod runner;
pub mod store;

use semver::Version;
use serde::{Deserialize, Serialize};

pub type MigrationId = &'static str;
pub type ModuleId = &'static str;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    NotStarted,
    InProgress,
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationAdvice {
    Pending,
    Ignored,
    Done,
}

impl std::fmt::Display for MigrationAdvice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Ignored => write!(f, "Ignored"),
            Self::Done => write!(f, "Done"),
        }
    }
}

/// Durable metadata shared by a migration step and the state recorder.
/// Keeping the descriptor free of a Tauri context breaks the crate dependency cycle.
#[derive(Debug, Clone, Copy)]
pub struct StepSpec<'a> {
    pub id: &'a str,
    pub module: &'a str,
    pub revision: u64,
    pub introduced_in: &'a Version,
}

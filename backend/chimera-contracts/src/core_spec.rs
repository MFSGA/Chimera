//! Failures of locating a core binary.

use chimera_config::application::ClashCore;
use serde::Serialize;
use snafu::Snafu;

/// A failure of locating the binary a core is started from.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum CoreSpecError {
    #[snafu(display("could not find the {core} core binary: {source}"))]
    FindCoreBinary {
        #[specta(type = String)]
        core: ClashCore,
        #[serde(skip)]
        source: std::io::Error,
    },
    // Chimera's additional executable choices include user-chosen paths;
    // retain their non-UTF-8 rejection instead of silently replacing the path.
    #[snafu(display("core executable path is not valid UTF-8: {path:?}"))]
    InvalidPath {
        #[serde(skip)]
        path: std::path::PathBuf,
    },
}

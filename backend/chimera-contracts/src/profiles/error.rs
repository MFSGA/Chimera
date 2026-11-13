//! Canonical error path serialization from ref/profiles/error.rs.
//! Only the shared path value is needed until the Profile error contract is migrated.

use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};
use snafu::Snafu;

use crate::BoxedCause;

/// A filesystem path as it appears in an error. The lossy conversion keeps it
/// serializable when the path is not valid UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct ErrorPath(#[specta(type = String)] PathBuf);

impl Serialize for ErrorPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0.display())
    }
}

impl std::fmt::Display for ErrorPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}

impl From<&Path> for ErrorPath {
    fn from(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}

impl From<PathBuf> for ErrorPath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl From<&PathBuf> for ErrorPath {
    fn from(path: &PathBuf) -> Self {
        Self(path.clone())
    }
}

#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubscriptionFetchError {
    #[snafu(display("could not build the http client"))]
    BuildHttpClient {
        #[serde(skip)]
        source: BoxedCause,
    },
    #[snafu(display("could not request the subscription"))]
    RequestSubscription {
        #[serde(skip)]
        source: BoxedCause,
    },
    #[snafu(display("the subscription server answered with HTTP {status}"))]
    SubscriptionHttpStatus { status: u16 },
    #[snafu(display("could not read the subscription response body"))]
    ReadSubscriptionBody {
        #[serde(skip)]
        source: BoxedCause,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_install_errors_serialize_paths_as_display_strings() {
        let path = PathBuf::from("staging/installed-core");
        let error_path = ErrorPath::from(&path);
        assert_eq!(error_path.to_string(), "staging/installed-core");
        assert_eq!(
            serde_json::to_value(error_path).unwrap(),
            "staging/installed-core"
        );
    }
}

// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Local compatibility location for ref nyanpasu-utils::io recovery classifiers.
//! Move to the pinned Chimera runtime utility crate when that dependency can adopt it.

/// How a caller may react to an IO error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum IoFailure {
    /// Contention that usually clears within milliseconds; a bounded retry may succeed.
    Transient,
    /// Access was refused. Neither a retry nor rebuilding the node can help.
    PermissionDenied,
    /// Any other error, including a missing node or invalid data.
    Other,
}

/// Backoff for [`IoFailure::Transient`] errors: three retries after the first
/// attempt, waiting 50, 100 and 200 ms.
///
/// The delays are listed rather than built with backon's exponential builder,
/// whose `f32` factor drifts off whole milliseconds.
pub fn transient_backoff() -> impl backon::BackoffBuilder {
    [50, 100, 200]
        .map(std::time::Duration::from_millis)
        .into_iter()
}

/// Classifies an IO error by how a caller may react to it.
///
/// Windows sharing and lock violations (raw codes 32 and 33) are transient
/// whatever kind the standard library maps them to.
pub fn classify(error: &std::io::Error) -> IoFailure {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return IoFailure::Transient;
    }
    match error.kind() {
        std::io::ErrorKind::Interrupted
        | std::io::ErrorKind::WouldBlock
        | std::io::ErrorKind::TimedOut
        | std::io::ErrorKind::ResourceBusy => IoFailure::Transient,
        std::io::ErrorKind::PermissionDenied => IoFailure::PermissionDenied,
        _ => IoFailure::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Error, ErrorKind},
        time::Duration,
    };

    use backon::BackoffBuilder as _;

    use super::{IoFailure, classify, transient_backoff};

    #[test]
    fn transient_backoff_waits_50_100_and_200_ms() {
        assert_eq!(
            transient_backoff().build().collect::<Vec<_>>(),
            [50, 100, 200].map(Duration::from_millis)
        );
    }

    #[test]
    fn classify_separates_contention_permission_and_other_errors() {
        for kind in [
            ErrorKind::Interrupted,
            ErrorKind::WouldBlock,
            ErrorKind::TimedOut,
            ErrorKind::ResourceBusy,
        ] {
            assert_eq!(
                classify(&Error::from(kind)),
                IoFailure::Transient,
                "{kind:?}"
            );
        }
        assert_eq!(
            classify(&Error::from(ErrorKind::PermissionDenied)),
            IoFailure::PermissionDenied
        );
        for kind in [ErrorKind::NotFound, ErrorKind::InvalidData] {
            assert_eq!(classify(&Error::from(kind)), IoFailure::Other, "{kind:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn classify_treats_windows_sharing_and_lock_violations_as_transient() {
        for code in [32, 33] {
            assert_eq!(
                classify(&Error::from_raw_os_error(code)),
                IoFailure::Transient
            );
        }
        // ERROR_ACCESS_DENIED stays a permission failure.
        assert_eq!(
            classify(&Error::from_raw_os_error(5)),
            IoFailure::PermissionDenied
        );
    }
}

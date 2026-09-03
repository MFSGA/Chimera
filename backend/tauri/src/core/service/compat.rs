//! Service daemon version compatibility gate.
//!
//! Status payloads can remain decodable across incompatible daemon revisions,
//! so service-mode eligibility must not rely on deserialization failures.

use chimera_ipc::types::{ServiceStatus, StatusInfo};

/// Chimera currently ships and speaks the v1 service protocol.
pub const REQUIRED_SERVICE_MAJOR: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ServiceCompat {
    Unknown,
    Compatible {
        server_version: String,
    },
    Incompatible {
        server_version: String,
        required_major: u64,
    },
    Unparsable {
        server_version: String,
    },
}

impl ServiceCompat {
    pub fn classify(info: &StatusInfo<'_>) -> Self {
        if info.status != ServiceStatus::Running {
            return Self::Unknown;
        }

        let Some(server) = info.server.as_ref() else {
            return Self::Unknown;
        };
        let server_version = server.version.to_string();
        let Some(version) = parse_service_version(&server_version) else {
            return Self::Unparsable { server_version };
        };

        if version.major != REQUIRED_SERVICE_MAJOR {
            return Self::Incompatible {
                server_version,
                required_major: REQUIRED_SERVICE_MAJOR,
            };
        }

        Self::Compatible { server_version }
    }

    pub fn allows_service_backend(&self) -> bool {
        matches!(self, Self::Compatible { .. })
    }
}

pub fn parse_service_version(raw: &str) -> Option<semver::Version> {
    semver::Version::parse(raw).ok()
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, path::PathBuf};

    use chimera_ipc::{
        api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody},
        types::{ServiceStatus, StatusInfo},
    };

    use super::{REQUIRED_SERVICE_MAJOR, ServiceCompat, parse_service_version};

    fn status(status: ServiceStatus, server_version: Option<&str>) -> StatusInfo<'static> {
        let server = server_version.map(|version| StatusResBody {
            version: Cow::Owned(version.to_owned()),
            core_infos: CoreInfos {
                r#type: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                config_path: None,
            },
            runtime_infos: RuntimeInfos {
                service_data_dir: Cow::Owned(PathBuf::new()),
                service_config_dir: Cow::Owned(PathBuf::new()),
                nyanpasu_config_dir: Cow::Owned(PathBuf::new()),
                nyanpasu_data_dir: Cow::Owned(PathBuf::new()),
            },
        });

        StatusInfo {
            name: Cow::Borrowed("chimera-service"),
            version: Cow::Borrowed("1.9.0"),
            status,
            server,
        }
    }

    #[test]
    fn v1_daemon_is_compatible() {
        let compat = ServiceCompat::classify(&status(ServiceStatus::Running, Some("1.9.0")));
        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: "1.9.0".to_owned(),
            }
        );
        assert!(compat.allows_service_backend());
    }

    #[test]
    fn future_major_is_fail_closed() {
        let compat = ServiceCompat::classify(&status(ServiceStatus::Running, Some("2.0.0")));
        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "2.0.0".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
            }
        );
        assert!(!compat.allows_service_backend());
    }

    #[test]
    fn unparsable_version_is_fail_closed() {
        let compat = ServiceCompat::classify(&status(ServiceStatus::Running, Some("nightly")));
        assert_eq!(
            compat,
            ServiceCompat::Unparsable {
                server_version: "nightly".to_owned(),
            }
        );
        assert!(!compat.allows_service_backend());
    }

    #[test]
    fn stopped_or_missing_server_is_unknown() {
        assert_eq!(
            ServiceCompat::classify(&status(ServiceStatus::Stopped, Some("1.9.0"))),
            ServiceCompat::Unknown
        );
        assert_eq!(
            ServiceCompat::classify(&status(ServiceStatus::Running, None)),
            ServiceCompat::Unknown
        );
    }

    #[test]
    fn semver_parser_accepts_prereleases() {
        let prerelease = parse_service_version("1.10.0-rc.1").expect("prerelease must parse");
        let stable = parse_service_version("1.9.0").expect("stable must parse");
        assert!(prerelease > stable);
    }
}

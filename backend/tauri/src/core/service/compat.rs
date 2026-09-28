//! Service daemon version compatibility gate.
//!
//! Status payloads can remain decodable across incompatible daemon revisions,
//! so service-mode eligibility must not rely on deserialization failures.

use chimera_ipc::types::{ServiceStatus, StatusInfo};

/// The service release line used by the app.
pub const REQUIRED_SERVICE_MAJOR: u64 = 1;

/// The first service release with the complete CoreControl endpoint set used
/// by this app. Version 1.10.0 lacks `/core/check` and cannot host a runtime.
pub const REQUIRED_SERVICE_MIN: &str = "1.10.1";

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
        required_min: String,
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

        if version.major != REQUIRED_SERVICE_MAJOR || version < required_service_min() {
            return Self::Incompatible {
                server_version,
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
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

fn required_service_min() -> semver::Version {
    semver::Version::parse(REQUIRED_SERVICE_MIN)
        .expect("REQUIRED_SERVICE_MIN must be a valid semver")
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, path::PathBuf};

    use chimera_ipc::{
        api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody},
        types::{ServiceStatus, StatusInfo},
    };

    use super::{
        REQUIRED_SERVICE_MAJOR, REQUIRED_SERVICE_MIN, ServiceCompat, parse_service_version,
        required_service_min,
    };

    fn status(status: ServiceStatus, server_version: Option<&str>) -> StatusInfo<'static> {
        let server = server_version.map(|version| StatusResBody {
            version: Cow::Owned(version.to_owned()),
            core_infos: CoreInfos {
                instance_id: None,
                r#type: None,
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                config_path: None,
                controller: None,
                health: None,
                revision: None,
                detail: None,
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
    fn daemon_at_minimum_is_compatible() {
        let compat =
            ServiceCompat::classify(&status(ServiceStatus::Running, Some(REQUIRED_SERVICE_MIN)));
        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(compat.allows_service_backend());
    }

    #[test]
    fn service_1100_is_incompatible_with_the_current_endpoint_contract() {
        let compat = ServiceCompat::classify(&status(ServiceStatus::Running, Some("1.10.0")));
        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "1.10.0".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!compat.allows_service_backend());
    }

    #[test]
    fn future_major_is_fail_closed() {
        let compat = ServiceCompat::classify(&status(ServiceStatus::Running, Some("2.0.0")));
        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "2.0.0".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
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

    fn parse_package_version(manifest: &str) -> semver::Version {
        let raw = manifest
            .lines()
            .skip_while(|line| line.trim() != "[package]")
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with('['))
            .find_map(|line| {
                let rest = line.trim().strip_prefix("version")?;
                let rest = rest.trim_start().strip_prefix('=')?.trim();
                let rest = rest.strip_prefix('"')?;
                Some(rest[..rest.find('"')?].to_owned())
            })
            .expect("service manifest must declare a package version");
        semver::Version::parse(&raw).expect("service package version must be valid semver")
    }

    #[test]
    fn required_min_never_exceeds_the_bundled_service_version() {
        const MANIFEST: &str =
            include_str!("../../../../chimera-runtime/chimera_service/Cargo.toml");
        let bundled = parse_package_version(MANIFEST);

        assert!(required_service_min() <= bundled);
        assert_eq!(bundled.major, REQUIRED_SERVICE_MAJOR);
    }
}

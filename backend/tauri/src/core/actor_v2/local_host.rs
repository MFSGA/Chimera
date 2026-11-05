//! The in-process core host, adapted from ref `core/actor_v2/local_host.rs`.
//!
//! Chimera keeps its application paths and core selector, while the shared
//! lifecycle transaction is owned by `chimera-core-manager`.

use std::sync::Arc;

use anyhow::Result as AnyResult;
use camino::Utf8PathBuf;
use chimera_core_manager::{
    ControlOptions, CoreControl, CoreKind, CoreManager, CoreSpec, LocalIpcPolicy, ManagerOptions,
    native_store::{FsNativeStore, StoreOwner, legacy_kind},
};
use chimera_utils::process::AtomicStopStrategy;
use snafu::{ResultExt, Snafu};

use crate::{config::chimera::ClashCore, utils::path::PathResolver};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to inspect {}", path.display()))]
    InspectConfig {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to read core kind from {}", path.display()))]
    ReadCoreKind {
        path: std::path::PathBuf,
        source: chimera_core_manager::native_store::StoreError,
    },
    #[snafu(display("failed to build core manager"))]
    BuildManager { source: chimera_core_manager::Error },
    #[snafu(display("path is not valid UTF-8: {}", path.display()))]
    InvalidPath { path: std::path::PathBuf },
}

/// Builds the local core host with the stop strategy shared by its process supervisors.
///
/// # Errors
/// Returns an error if configuration inspection or core manager construction fails.
pub async fn build(
    paths: &PathResolver,
    shutdown_helper: &std::path::Path,
    stop_strategy: Arc<AtomicStopStrategy>,
) -> Result<CoreControl, Error> {
    let runtime_root = paths.app_config_dir().join("runtime");
    let options = ManagerOptions {
        runtime_dir: Some(
            to_utf8(runtime_root.join("control")).map_err(|path| Error::InvalidPath { path })?,
        ),
        local_ipc_policy: LocalIpcPolicy::Disable,
        windows_shutdown_helper: Some(shutdown_helper.to_owned()),
        // Local macOS mode may require the user to authorize DNS changes.
        #[cfg(target_os = "macos")]
        dns_timeout: std::time::Duration::from_secs(60),
        ..ManagerOptions::default()
    };
    let working_dir =
        to_utf8(paths.app_data_dir().to_owned()).map_err(|path| Error::InvalidPath { path })?;
    let native_store = Arc::new(FsNativeStore::new(
        working_dir.clone(),
        StoreOwner::current(),
        configured_core_kind(paths)?,
    ));
    let manager = CoreManager::builder(options)
        .native_store(native_store)
        .stop_strategy(stop_strategy);

    #[cfg(target_os = "macos")]
    let manager = manager.dns_controller(Arc::new(
        chimera_core_manager::dns::macos::MacosDnsController::new(
            "State:/Network/Service/chimera-dns/DNS".into(),
        ),
    ));

    let manager = manager.build().await.context(BuildManagerSnafu)?;
    let source_dir =
        to_utf8(runtime_root.join("staging")).map_err(|path| Error::InvalidPath { path })?;

    Ok(CoreControl::spawn(
        manager,
        ControlOptions::new(source_dir, working_dir),
    ))
}

fn configured_core_kind(paths: &PathResolver) -> Result<Option<CoreKind>, Error> {
    let typed_config = paths.application_config_path();
    let config_path = if typed_config.try_exists().context(InspectConfigSnafu {
        path: typed_config.as_path(),
    })? {
        typed_config
    } else {
        paths.chimera_config_path()
    };
    legacy_kind(config_path.as_path()).context(ReadCoreKindSnafu {
        path: config_path.as_path(),
    })
}

/// Resolve an application core choice to the executable identity understood
/// by the shared process manager. `ChimeraClient` deliberately keeps its own
/// kind even though it accepts the clash-rs command line.
pub(crate) fn core_spec(core: &ClashCore) -> AnyResult<CoreSpec> {
    core_spec_with(core, |core, core_type| {
        if *core == ClashCore::ChimeraClientAlpha {
            let binary_name = format!("chimera-client-alpha{}", std::env::consts::EXE_SUFFIX);
            crate::core::clash::core::find_binary_path_by_name(&binary_name)
        } else {
            crate::core::clash::core::find_binary_path(core_type)
        }
    })
}

fn core_spec_with(
    core: &ClashCore,
    find_binary: impl FnOnce(
        &ClashCore,
        &chimera_utils::core::CoreType,
    ) -> std::io::Result<std::path::PathBuf>,
) -> AnyResult<CoreSpec> {
    let core_type: chimera_utils::core::CoreType = core.into();
    let kind = match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::ChimeraClient => CoreKind::ChimeraClient,
        ClashCore::ChimeraClientAlpha => CoreKind::ChimeraClient,
    };
    let binary_path = to_utf8(find_binary(core, &core_type)?)
        .map_err(|path| anyhow::anyhow!("path is not valid UTF-8: {}", path.to_string_lossy()))?;

    Ok(CoreSpec {
        kind,
        binary_path,
        version: None,
        features: Vec::new(),
    })
}

fn to_utf8(path: std::path::PathBuf) -> Result<Utf8PathBuf, std::path::PathBuf> {
    Utf8PathBuf::from_path_buf(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_control_uses_isolated_runtime_paths() {
        let root = tempfile::TempDir::new().unwrap();
        let paths =
            PathResolver::with_base_dirs(root.path().join("config"), root.path().join("data"));

        let control = build(
            &paths,
            &root.path().join("helper.exe"),
            Arc::new(AtomicStopStrategy::new(
                chimera_utils::process::StopStrategy::default(),
            )),
        )
        .await
        .unwrap();

        assert!(matches!(
            control.status().state,
            chimera_core_manager::CoreState::Stopped { .. }
        ));
        assert!(!control.executor_is_closed());
        control.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn typed_config_takes_precedence_and_legacy_parse_errors_are_preserved() {
        let root = tempfile::TempDir::new().unwrap();
        let paths =
            PathResolver::with_base_dirs(root.path().join("config"), root.path().join("data"));
        std::fs::create_dir_all(paths.app_config_dir()).unwrap();
        std::fs::create_dir_all(paths.app_data_dir()).unwrap();
        std::fs::write(paths.application_config_path(), "core: chimera-client\n").unwrap();
        std::fs::write(paths.chimera_config_path(), "core: mihomo\n").unwrap();
        assert_eq!(
            configured_core_kind(&paths).unwrap(),
            Some(CoreKind::ChimeraClient),
            "typed config must take precedence over the legacy core choice"
        );
        let stop_strategy = Arc::new(AtomicStopStrategy::new(
            chimera_utils::process::StopStrategy::default(),
        ));

        let control = build(
            &paths,
            &root.path().join("helper.exe"),
            Arc::clone(&stop_strategy),
        )
        .await
        .expect("the typed config must be selected before the legacy core choice");
        control.shutdown().await.unwrap();

        std::fs::remove_file(paths.application_config_path()).unwrap();
        assert_eq!(
            configured_core_kind(&paths).unwrap(),
            Some(CoreKind::Mihomo),
            "legacy config must be used when the typed config is absent"
        );
        std::fs::write(paths.chimera_config_path(), "invalid: [\n").unwrap();
        let result = build(&paths, &root.path().join("helper.exe"), stop_strategy).await;
        let Err(error) = result else {
            panic!("the malformed legacy config must be rejected");
        };
        let legacy_path = paths.chimera_config_path();
        assert!(matches!(
            error,
            Error::ReadCoreKind { ref path, .. } if path == &legacy_path
        ));
    }

    #[test]
    fn core_spec_preserves_each_supported_brand_and_executable_mapping() {
        let root = tempfile::TempDir::new().unwrap();
        let variants = [
            (ClashCore::ClashPremium, CoreKind::ClashPremium),
            (ClashCore::ClashRs, CoreKind::ClashRust),
            (ClashCore::ClashRsAlpha, CoreKind::ClashRust),
            (ClashCore::Mihomo, CoreKind::Mihomo),
            (ClashCore::MihomoAlpha, CoreKind::Mihomo),
            (ClashCore::ChimeraClient, CoreKind::ChimeraClient),
            (ClashCore::ChimeraClientAlpha, CoreKind::ChimeraClient),
        ];

        for (core, expected_kind) in variants {
            let spec = core_spec_with(&core, |core, core_type| {
                let name = if *core == ClashCore::ChimeraClientAlpha {
                    format!("chimera-client-alpha{}", std::env::consts::EXE_SUFFIX)
                } else {
                    core_type.get_executable_name().to_owned()
                };
                Ok(root.path().join(name))
            })
            .unwrap();

            assert_eq!(spec.kind, expected_kind, "wrong runtime kind for {core}");
            let expected_binary_name =
                format!("{}{}", core_type_name(&core), std::env::consts::EXE_SUFFIX);
            assert_eq!(
                spec.binary_path.file_name(),
                Some(expected_binary_name.as_str())
            );
            assert!(spec.version.is_none());
            assert!(spec.features.is_empty());
        }
    }

    #[test]
    fn chimera_client_keeps_its_manager_identity() {
        let root = tempfile::TempDir::new().unwrap();
        let spec = core_spec_with(&ClashCore::ChimeraClient, |_, core_type| {
            Ok(root.path().join(core_type.get_executable_name()))
        })
        .unwrap();

        assert_eq!(spec.kind.as_ref(), "chimera-client");
        assert_ne!(spec.kind, CoreKind::ClashRust);
        assert!(spec.binary_path.as_str().contains("chimera-client"));
    }

    #[test]
    fn non_utf8_executable_path_is_rejected_at_the_host_boundary() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let error = core_spec_with(&ClashCore::Mihomo, |_, _| {
                Ok(std::path::PathBuf::from(std::ffi::OsString::from_vec(
                    vec![0xff],
                )))
            })
            .unwrap_err();
            assert!(error.to_string().contains("path is not valid UTF-8"));
        }
    }

    fn core_type_name(core: &ClashCore) -> &'static str {
        match core {
            ClashCore::ClashPremium => "clash",
            ClashCore::ClashRs => "clash-rs",
            ClashCore::ClashRsAlpha => "clash-rs-alpha",
            ClashCore::Mihomo => "mihomo",
            ClashCore::MihomoAlpha => "mihomo-alpha",
            ClashCore::ChimeraClient => "chimera-client",
            ClashCore::ChimeraClientAlpha => "chimera-client-alpha",
        }
    }
}

//! The in-process core host, adapted from ref `core/actor_v2/local_host.rs`.
//!
//! Chimera keeps its application paths and core selector, while the shared
//! lifecycle transaction is owned by `chimera-core-manager`.

#[cfg(target_os = "macos")]
use std::sync::Arc;

use anyhow::Result;
use camino::Utf8PathBuf;
use chimera_core_manager::{
    ControlOptions, CoreControl, CoreKind, CoreManager as RuntimeManager, CoreSpec, LocalIpcPolicy,
    ManagerOptions,
};

use crate::{config::chimera::ClashCore, utils::path::PathResolver};

/// Construct the local host from the application's resolved directories.
///
/// The private manager runtime directory holds epochs and effective configs;
/// staging is a sibling because caller-owned config bytes are materialized
/// only for one control operation. The core working directory remains the
/// user's data directory so existing GeoIP and GeoSite assets stay visible.
pub(crate) async fn build(paths: &PathResolver) -> Result<CoreControl> {
    let runtime_root = paths.app_config_dir().join("runtime");
    let manager_options = ManagerOptions {
        runtime_dir: Some(to_utf8(runtime_root.join("control"))?),
        local_ipc_policy: LocalIpcPolicy::Disable,
        // A local macOS host may need the user to answer a DNS authorization
        // dialog. Service hosts already run with administrator privileges.
        #[cfg(target_os = "macos")]
        dns_timeout: std::time::Duration::from_secs(60),
        ..ManagerOptions::default()
    };

    let manager = RuntimeManager::builder(manager_options);
    #[cfg(target_os = "macos")]
    let manager = manager.dns_controller(Arc::new(
        chimera_core_manager::dns::macos::MacosDnsController::new(
            "State:/Network/Service/chimera-dns/DNS".into(),
        ),
    ));
    let manager = manager.build().await?;
    let source_dir = to_utf8(runtime_root.join("staging"))?;
    let working_dir = to_utf8(paths.app_data_dir().to_owned())?;

    Ok(CoreControl::spawn(
        manager,
        ControlOptions::new(source_dir, working_dir),
    ))
}

/// Resolve an application core choice to the executable identity understood
/// by the shared process manager. `ChimeraClient` deliberately keeps its own
/// kind even though it accepts the clash-rs command line.
pub(crate) fn core_spec(core: &ClashCore) -> Result<CoreSpec> {
    core_spec_with(core, crate::core::clash::core::find_binary_path)
}

fn core_spec_with(
    core: &ClashCore,
    find_binary: impl FnOnce(&chimera_utils::core::CoreType) -> std::io::Result<std::path::PathBuf>,
) -> Result<CoreSpec> {
    let core_type: chimera_utils::core::CoreType = core.into();
    let kind = match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::ChimeraClient => CoreKind::ChimeraClient,
    };
    let binary_path = to_utf8(find_binary(&core_type)?)?;

    Ok(CoreSpec {
        kind,
        binary_path,
        version: None,
        features: Vec::new(),
    })
}

fn to_utf8(path: std::path::PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("path is not valid UTF-8: {}", path.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_control_uses_isolated_runtime_paths() {
        let root = tempfile::TempDir::new().unwrap();
        let paths =
            PathResolver::with_base_dirs(root.path().join("config"), root.path().join("data"));

        let control = build(&paths).await.unwrap();

        assert!(matches!(
            control.status().state,
            chimera_core_manager::CoreState::Stopped { .. }
        ));
        assert!(!control.executor_is_closed());
        control.shutdown().await.unwrap();
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
        ];

        for (core, expected_kind) in variants {
            let spec = core_spec_with(&core, |core_type| {
                Ok(root.path().join(core_type.get_executable_name()))
            })
            .unwrap();

            assert_eq!(spec.kind, expected_kind, "wrong runtime kind for {core}");
            assert!(spec.binary_path.ends_with(core_type_name(&core)));
            assert!(spec.version.is_none());
            assert!(spec.features.is_empty());
        }
    }

    #[test]
    fn chimera_client_keeps_its_manager_identity() {
        let root = tempfile::TempDir::new().unwrap();
        let spec = core_spec_with(&ClashCore::ChimeraClient, |core_type| {
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
            let error = core_spec_with(&ClashCore::Mihomo, |_| {
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
        }
    }
}

//! Ref-aligned core specification resolution for RuntimeBuildPort.
//!
//! The core type and manager kind are derived once from the typed application
//! choice; filesystem discovery is an injected host capability. This is the
//! same separation as ref's `actor_v2/local_host::core_spec_with`, extended for
//! ChimeraClient and retained legacy core choices.

use std::path::PathBuf;

use camino::Utf8PathBuf;
use chimera_config::application::ClashCore;
use chimera_core_manager::{CoreKind, CoreSpec};
use chimera_utils::core::CoreType;

/// The port the future RuntimeBuildService receives from the application shell.
/// It has no mutable Core state and cannot trigger an apply or publication.
pub type CoreSpecResolver = dyn Fn(&ClashCore) -> Result<CoreSpec, CoreSpecError> + Send + Sync;

// The typed source error now lives in the shared ref values layer.
pub use chimera_contracts::core_spec::CoreSpecError;

/// Ref's typed choice to the concrete core process manager identity.
/// `ChimeraClientAlpha` uses the ChimeraClient manager kind with an independent
/// alpha binary, not a ClashRust alias.
pub fn core_kind(core: ClashCore) -> CoreKind {
    match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::ChimeraClient | ClashCore::ChimeraClientAlpha => CoreKind::ChimeraClient,
    }
}

/// Resolve without reading or mutating any runtime state. The caller owns
/// discovery, permissions, platform executable suffix and path policy.
pub fn resolve_with(
    core: &ClashCore,
    find_binary: impl FnOnce(&ClashCore, &CoreType) -> std::io::Result<PathBuf>,
) -> Result<CoreSpec, CoreSpecError> {
    let core_type = CoreType::from(core);
    let kind = core_kind(*core);
    let binary_path =
        find_binary(core, &core_type).map_err(|source| CoreSpecError::FindCoreBinary {
            core: *core,
            source,
        })?;
    let binary_path = Utf8PathBuf::from_path_buf(binary_path)
        .map_err(|path| CoreSpecError::InvalidPath { path })?;
    Ok(CoreSpec {
        kind,
        binary_path,
        version: None,
        features: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_variant_maps_to_its_manager_kind_and_own_binary() {
        for (core, kind) in [
            (ClashCore::ClashPremium, CoreKind::ClashPremium),
            (ClashCore::ClashRs, CoreKind::ClashRust),
            (ClashCore::ClashRsAlpha, CoreKind::ClashRust),
            (ClashCore::Mihomo, CoreKind::Mihomo),
            (ClashCore::MihomoAlpha, CoreKind::Mihomo),
            (ClashCore::ChimeraClient, CoreKind::ChimeraClient),
            (ClashCore::ChimeraClientAlpha, CoreKind::ChimeraClient),
        ] {
            let spec = resolve_with(&core, |requested, core_type| {
                assert_eq!(*requested, core);
                Ok(PathBuf::from(format!(
                    "/binaries/{}",
                    core_type.get_executable_name()
                )))
            })
            .unwrap();
            assert_eq!(spec.kind, kind);
            // CoreType::get_executable_name includes the host executable
            // suffix, while ClashCore::binary_name is extension-free.
            let expected = format!("{}{}", core.binary_name(), std::env::consts::EXE_SUFFIX);
            assert_eq!(spec.binary_path.file_name(), Some(expected.as_str()));
            assert!(spec.version.is_none());
            assert!(spec.features.is_empty());
        }
    }

    #[test]
    fn lookup_errors_carry_selected_core_identity() {
        let err = resolve_with(&ClashCore::ChimeraClientAlpha, |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing custom alpha binary",
            ))
        })
        .unwrap_err();
        assert!(matches!(
            err,
            CoreSpecError::FindCoreBinary {
                core: ClashCore::ChimeraClientAlpha,
                ..
            }
        ));
        assert!(err.to_string().contains("missing custom alpha binary"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_rejected_before_core_is_selected() {
        use std::os::unix::ffi::OsStringExt;
        let err = resolve_with(&ClashCore::Mihomo, |_, _| {
            Ok(PathBuf::from(std::ffi::OsString::from_vec(vec![0xff])))
        })
        .unwrap_err();
        assert!(matches!(err, CoreSpecError::InvalidPath { .. }));
    }
}

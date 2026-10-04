//! Core execution permissions required by host-level runtime features.

/// Grant the selected core the macOS privileges needed to create a TUN and
/// manage host routes. The caller must invoke this only from an explicit
/// TUN-enable preflight so macOS can ask the user for administrator approval.
#[cfg(target_os = "macos")]
pub fn grant_permission(core: &chimera_utils::core::CoreType) -> anyhow::Result<()> {
    use anyhow::Context as _;

    let path = crate::core::clash::core::find_binary_path(core)
        .context("clash core not found")?
        .canonicalize()
        .context("failed to resolve the selected core binary")?;
    let metadata = std::fs::metadata(&path).context("failed to inspect the selected core")?;
    if macos_tun_permission_granted(&metadata)? {
        return Ok(());
    }

    let escaped_path = shell_single_quote(&path.to_string_lossy());
    let shell_command =
        format!("/usr/sbin/chown root:admin {escaped_path} && /bin/chmod u+s {escaped_path}");
    let applescript = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&shell_command)
    );
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &applescript])
        .status()
        .context("failed to request macOS administrator authorization for TUN")?;
    anyhow::ensure!(
        status.success(),
        "macOS administrator authorization for TUN failed"
    );

    let metadata = std::fs::metadata(&path).context("failed to verify the selected core")?;
    anyhow::ensure!(
        macos_tun_permission_granted(&metadata)?,
        "macOS did not apply the required TUN core permissions"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_tun_permission_granted(metadata: &std::fs::Metadata) -> anyhow::Result<bool> {
    use nix::unistd::{Gid, Group, Uid, User};
    use std::os::unix::fs::MetadataExt as _;

    Ok(
        User::from_uid(Uid::from_raw(metadata.uid()))?.is_some_and(|user| user.name == "root")
            && Group::from_gid(Gid::from_raw(metadata.gid()))?
                .is_some_and(|group| group.name == "admin")
            && metadata.mode() & 0o4000 != 0,
    )
}

#[cfg(target_os = "macos")]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tun_permission_tests {
    use super::{applescript_escape, shell_single_quote};

    #[test]
    fn shell_path_is_quoted_as_one_argument() {
        assert_eq!(
            shell_single_quote("/tmp/Core's Bin"),
            "'/tmp/Core'\\''s Bin'"
        );
    }

    #[test]
    fn applescript_command_escapes_string_delimiters() {
        assert_eq!(applescript_escape("a\\b\"c"), "a\\\\b\\\"c");
    }
}

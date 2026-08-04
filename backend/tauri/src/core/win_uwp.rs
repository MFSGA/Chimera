#![cfg(target_os = "windows")]

#[cfg(not(feature = "e2e"))]
use crate::utils::dirs;
use anyhow::{Result, bail};
#[cfg(not(feature = "e2e"))]
use deelevate::{PrivilegeLevel, Token};
#[cfg(not(feature = "e2e"))]
use runas::Command as RunasCommand;
#[cfg(not(feature = "e2e"))]
use std::process::Command as StdCommand;

#[cfg(feature = "e2e")]
pub async fn invoke_uwptools() -> Result<()> {
    bail!("UWP loopback changes are disabled in E2E mode")
}

#[cfg(not(feature = "e2e"))]
pub async fn invoke_uwptools() -> Result<()> {
    let resource_dir = dirs::app_resources_dir()?;
    let tool_path = resource_dir.join("enableLoopback.exe");

    if !tool_path.exists() {
        bail!("enableLoopback exe not found");
    }

    let token = Token::with_current_process()?;
    let level = token.privilege_level()?;

    match level {
        PrivilegeLevel::NotPrivileged => RunasCommand::new(tool_path).status()?,
        _ => StdCommand::new(tool_path).status()?,
    };

    Ok(())
}

#[cfg(all(test, feature = "e2e"))]
mod tests {
    use super::invoke_uwptools;

    #[tokio::test]
    async fn e2e_uwp_loopback_change_is_rejected_before_host_access() {
        let error = invoke_uwptools()
            .await
            .expect_err("E2E must not execute enableLoopback.exe");

        assert_eq!(
            error.to_string(),
            "UWP loopback changes are disabled in E2E mode"
        );
    }
}

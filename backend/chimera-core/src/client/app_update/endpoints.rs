//! Updater feed calculation from an explicit release channel and optional SourceForge project.

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("invalid SourceForge project slug: {project}"))]
    ValidateSourceforgeProject { project: String },
}
use chimera_config::application::ReleaseChannel;

pub fn update_endpoints(channel: ReleaseChannel, project: Option<&str>) -> Vec<String> {
    update_endpoints_for_target(channel, project, false)
}

pub fn update_endpoints_for_target(
    channel: ReleaseChannel,
    project: Option<&str>,
    fixed_webview: bool,
) -> Vec<String> {
    match update_endpoints_for_target_checked(channel, project, fixed_webview) {
        Ok(endpoints) => endpoints,
        Err(error) => {
            tracing::warn!(%error, "ignoring invalid SourceForge updater manifest project");
            update_endpoints_for_target_checked(channel, None, fixed_webview)
                .expect("the updater endpoints without SourceForge are valid")
        }
    }
}

pub fn update_endpoints_for_project(
    channel: ReleaseChannel,
    project: Option<&str>,
) -> Result<Vec<String>, Error> {
    update_endpoints_for_target_checked(channel, project, false)
}

pub fn update_endpoints_for_target_checked(
    channel: ReleaseChannel,
    project: Option<&str>,
    fixed_webview: bool,
) -> Result<Vec<String>, Error> {
    let channel_suffix = match channel {
        ReleaseChannel::Stable => "",
        ReleaseChannel::Beta => "-beta",
        ReleaseChannel::Nightly => "-nightly",
    };
    let fixed_suffix = if fixed_webview { "-fixed-webview" } else { "" };
    let mut suffixes = vec![channel_suffix];
    // Chimera's release tooling currently publishes stable and nightly
    // manifests, but not a dedicated beta manifest. Keep the beta feed first
    // for a future publisher and fall back to the existing stable feed.
    if channel == ReleaseChannel::Beta {
        suffixes.push("");
    }

    let mut endpoints = Vec::with_capacity(suffixes.len() * 4 + 1);
    for suffix in suffixes {
        let manifest = format!("update{suffix}{fixed_suffix}");
        let github =
            format!("https://github.com/MFSGA/Chimera/releases/download/updater/{manifest}.json");
        endpoints.extend([
            format!(
                "https://gh-proxy.com/https://github.com/MFSGA/Chimera/releases/download/updater/{manifest}-proxy.json"
            ),
            format!(
                "https://github.com/MFSGA/Chimera/releases/download/updater/{manifest}-proxy.json"
            ),
            github.clone(),
            format!("https://ghfast.top/{github}"),
        ]);
        if let Some(project) = project.filter(|project| !project.trim().is_empty()) {
            if !is_valid_sourceforge_project(project) {
                return ValidateSourceforgeProjectSnafu {
                    project: project.to_owned(),
                }
                .fail();
            }
            endpoints.push(format!(
                "https://{project}.sourceforge.io/updater/{manifest}.json"
            ));
        }
    }
    Ok(endpoints)
}

pub fn is_valid_sourceforge_project(project: &str) -> bool {
    !project.is_empty()
        && project
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && project.as_bytes()[0].is_ascii_alphanumeric()
        && project
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

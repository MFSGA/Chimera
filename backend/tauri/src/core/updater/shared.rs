pub(super) fn get_arch() -> anyhow::Result<&'static str> {
    let env = (std::env::consts::ARCH, std::env::consts::OS);

    match env {
        ("x86_64", "macos") => Ok("darwin-x64"),
        ("x86_64", "linux") => Ok("linux-amd64"),
        ("x86_64", "windows") => Ok("windows-x86_64"),
        ("aarch64", "macos") => Ok("darwin-arm64"),
        ("aarch64", "linux") => Ok("linux-aarch64"),
        ("aarch64", "windows") => Ok("windows-arm64"),
        _ => anyhow::bail!("unsupported platform"),
    }
}

pub(super) enum CoreTypeMeta {
    ClashPremium(String),
    Mihomo(String),
    MihomoAlpha,
    ClashRs(String),
    ChimeraClient(String),
    ChimeraClientAlpha,
    ClashRsAlpha,
}

pub(super) fn get_download_path(core_type: CoreTypeMeta, artifact: &str) -> String {
    match core_type {
        CoreTypeMeta::Mihomo(tag) => {
            format!("MetaCubeX/mihomo/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::MihomoAlpha => {
            format!("MetaCubeX/mihomo/releases/download/Prerelease-Alpha/{artifact}")
        }
        CoreTypeMeta::ClashRs(tag) => {
            format!("ibigbug/clash-rs/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::ChimeraClient(tag) => {
            format!("MFSGA/Chimera_Client/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::ChimeraClientAlpha => {
            format!("MFSGA/Chimera_Client/releases/download/latest/{artifact}")
        }
        CoreTypeMeta::ClashRsAlpha => {
            format!("ibigbug/clash-rs/releases/download/latest/{artifact}")
        }
        CoreTypeMeta::ClashPremium(tag) => {
            format!("zhongfly/Clash-premium-backup/releases/download/{tag}/{artifact}")
        }
    }
}

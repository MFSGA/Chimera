use anyhow::Result;

use crate::config::{chimera::IVerge, core::Config};

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    // Validate theme_color if it's being updated
    if let Some(ref theme_color) = patch.theme_color {
        if !theme_color.is_empty() && !crate::config::chimera::is_hex_color(theme_color) {
            anyhow::bail!("Invalid theme color: {}", theme_color);
        }
    }
    Config::verge().draft().patch_config(patch.clone());

    todo!()
}

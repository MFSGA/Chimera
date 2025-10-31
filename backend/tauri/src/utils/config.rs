use anyhow::Result;
use sysproxy::Sysproxy;

use crate::config::core::Config;

pub fn get_self_proxy() -> Result<String> {
    let port = Config::verge()
        .latest()
        .verge_mixed_port
        .unwrap_or(Config::clash().data().get_mixed_port());

    let proxy_scheme = format!("http://127.0.0.1:{port}");
    Ok(proxy_scheme)
}

pub fn get_system_proxy() -> Result<Option<String>> {
    let p = Sysproxy::get_system_proxy()?;
    if p.enable {
        let proxy_scheme = format!("http://{}:{}", p.host, p.port);
        return Ok(Some(proxy_scheme));
    }

    Ok(None)
}

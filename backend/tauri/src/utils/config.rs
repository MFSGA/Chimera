use anyhow::Result;
use chimera_contracts::net::SystemProxySource as _;

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
    // The Legacy client also uses the upstream OS owner rather than probing
    // sysproxy independently inside the GUI layer.
    chimera_os::system_proxy_source::OsSystemProxySource.system_proxy()
}

pub trait NyanpasuReqwestProxyExt {
    fn swift_set_proxy(self, url: &str) -> Self;

    fn swift_set_nyanpasu_proxy(self) -> Self;
}

impl NyanpasuReqwestProxyExt for reqwest::ClientBuilder {
    fn swift_set_proxy(self, url: &str) -> Self {
        let mut builder = self;
        if let Ok(proxy) = reqwest::Proxy::http(url) {
            builder = builder.proxy(proxy);
        }
        if let Ok(proxy) = reqwest::Proxy::https(url) {
            builder = builder.proxy(proxy);
        }
        if let Ok(proxy) = reqwest::Proxy::all(url) {
            builder = builder.proxy(proxy);
        }
        builder
    }

    fn swift_set_nyanpasu_proxy(self) -> Self {
        let mut builder = self;
        if let Ok(proxy) = get_self_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        if let Ok(Some(proxy)) = get_system_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        builder
    }
}

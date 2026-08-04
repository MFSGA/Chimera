use std::net::SocketAddr;

/// Prevents the controller credential from ever being sent outside this host.
pub(super) fn loopback_controller_url(server: &str) -> Result<url::Url, ()> {
    let address = server.parse::<SocketAddr>().map_err(|_| ())?;
    if !address.ip().is_loopback() {
        return Err(());
    }
    url::Url::parse(&format!("http://{server}/configs")).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::loopback_controller_url;

    #[test]
    fn controller_probe_accepts_only_loopback_socket_addresses() {
        assert!(loopback_controller_url("127.0.0.1:9090").is_ok());
        assert!(loopback_controller_url("[::1]:9090").is_ok());
        assert!(loopback_controller_url("192.168.1.2:9090").is_err());
        assert!(loopback_controller_url("example.com:9090").is_err());
    }
}

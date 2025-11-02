use once_cell::sync::Lazy;

pub static SERVER_PORT: Lazy<u16> = Lazy::new(|| port_scanner::request_open_port().unwrap());

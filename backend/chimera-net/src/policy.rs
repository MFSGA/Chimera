//! Proxy URL helpers ported from ref's network policy owner.
//! Further reqwest policy adapters remain on the ref-alignment backlog.

/// The application-owned mixed port, with no OS proxy lookup.
pub fn get_self_proxy(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

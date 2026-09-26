//! Profiles domain state (PR-3). Tauri-free.

pub mod actor;
pub mod ports;
mod scheduler;

pub use actor::*;

/// REF-compatible FNV-1a digest used for staged profile content identity.
///
/// Kept here until the upstream core-manager digest crate is wired into this
/// workspace; the algorithm and formatting match its `payload_digest` helper.
pub(crate) fn payload_digest(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

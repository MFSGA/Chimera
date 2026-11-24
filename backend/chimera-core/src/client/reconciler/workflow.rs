//! Pure host preference shared by startup and committed convergence.
use crate::actor_v2::endpoint::ExecutionHost;

pub fn effective_host(
    enable_service_mode: bool,
    service: Option<bool>,
    current: ExecutionHost,
) -> ExecutionHost {
    match (enable_service_mode, service) {
        (true, Some(true)) => ExecutionHost::Service,
        (true, None) => current,
        _ => ExecutionHost::Local,
    }
}

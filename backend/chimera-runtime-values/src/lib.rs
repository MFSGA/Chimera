//! Definition-only public runtime results and error values.
//!
//! Existing shared contracts retain their owner until all consumers migrate.
//! The remaining upstream definitions require a coordinated owner migration.

pub mod app_update;
pub mod check;
pub mod configuration;
pub mod core_status;
pub mod effects;
pub mod hotkey;
pub mod profiles;
pub mod sync;
pub use chimera_contracts::{
    core_spec, execution_host, mutation, network_automation, ports, publish, runtime,
    runtime_error, storage,
};

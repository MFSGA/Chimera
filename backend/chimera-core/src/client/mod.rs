pub mod clash_api;
pub mod clash_info;
pub mod convergence;

pub mod app_update;

pub mod traffic_frames;

pub mod runtime_inspection;

pub mod artifact_snapshot;

pub mod runtime_build;

pub mod runtime_build_port;

pub mod reconciler;

pub mod core_spec;

pub mod core_lifecycle;

pub mod ports;
pub mod runtime;
pub mod runtime_recovery;

pub mod logs;

pub mod frontend_events;
pub mod session_state;

pub mod core_events;
pub mod event_sink;

pub mod application;
pub mod clash_config;

pub mod mutation_outcome;

pub mod profiles;

pub mod effects;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod system_proxy;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod hotkey;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod network_automation;

pub mod configuration;

pub mod configuration_observation;
pub mod configuration_status;

pub mod app_lifecycle;
pub mod composition;

pub mod direct_egress;
pub mod geo_mode;
pub mod traffic;

pub mod host_adapters;

mod facade;
pub use facade::{
    ChimeraClient, ClientParts, WeakChimeraClient, auto_activation_failure_degradation,
};

pub mod application_operations;

pub mod clash_operations;

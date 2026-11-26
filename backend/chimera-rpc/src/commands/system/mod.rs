//! The core lifecycle, system proxy, TUN, service mode and OS integration.

chimera_rpc::namespace!(expose = [http, local]);

pub mod core;
pub mod hotkeys;
pub mod network;
pub mod os;
pub mod proxy;
pub mod service;
pub mod status;

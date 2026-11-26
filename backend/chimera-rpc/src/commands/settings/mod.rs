//! Ordinary application and Clash preferences, configuration sync and the frontend key-value
//! storage.

chimera_rpc::namespace!(expose = [http, local]);

pub mod clash;
pub mod network;

pub mod app;
pub mod storage;

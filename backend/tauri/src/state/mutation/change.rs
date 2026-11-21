//! Candidate documents shared by source owners and runtime consumers.

use std::sync::Arc;

use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};

/// One source's proposed document; the other domains are sampled independently.
#[derive(Debug, Clone)]
pub enum DomainChange {
    Application { candidate: Arc<ChimeraAppConfig> },
    Clash { candidate: Arc<ClashConfig> },
    Profiles { candidate: Arc<Profiles> },
}

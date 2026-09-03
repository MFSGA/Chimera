use crate::core::clash::api;
use anyhow::Result;
use chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ConnectionInfo {
    pub id: String,
    pub chains: Vec<String>,
}

/// Connection interruption service that handles closing connections based on configuration settings
pub struct ConnectionInterruptionService;

impl ConnectionInterruptionService {
    /// Interrupt connections when proxy changes
    pub async fn on_proxy_change(break_when: ProxyChangeBreakMode) -> Result<()> {
        match break_when {
            ProxyChangeBreakMode::Off => {
                // Do nothing
                Ok(())
            }
            ProxyChangeBreakMode::ProxyGroup => {
                // TODO: Implement chain-based connection interruption
                // This would require tracking which connections use which proxy chains
                // For now, we'll fall back to closing all connections
                api::delete_connections(None).await
            }
            ProxyChangeBreakMode::All => api::delete_connections(None).await,
        }
    }

    /// Interrupt connections when profile changes
    pub async fn on_profile_change(break_when: bool) -> Result<()> {
        if break_when {
            api::delete_connections(None).await
        } else {
            // Do nothing
            Ok(())
        }
    }

    /// Interrupt connections when mode changes
    pub async fn on_mode_change(break_when: bool) -> Result<()> {
        if break_when {
            api::delete_connections(None).await
        } else {
            // Do nothing
            Ok(())
        }
    }

    /// Interrupt all connections
    pub async fn interrupt_all() -> Result<()> {
        api::delete_connections(None).await
    }

    /// Interrupt connections based on proxy chain (not yet implemented)
    pub async fn interrupt_by_chain(_chain: &[String]) -> Result<()> {
        // TODO: Implement chain-based connection interruption
        // This would require:
        // 1. Getting the current connections from the Clash API
        // 2. Filtering connections that use the specified proxy chain
        // 3. Closing only those connections
        // For now, we'll close all connections as a fallback
        api::delete_connections(None).await
    }
}

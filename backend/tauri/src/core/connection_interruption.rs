//! Chimera policy adapter; connection selection and close requests live in
//! `chimera_core::connections` beside the core-owned Clash API capability.

use anyhow::Result;
use chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode;
use chimera_core::connections::{ConnectionScope, interrupt_by_chains, interrupt_connections};

use crate::core::actor_v2::api::ApiClient;

/// Map the persisted app strategy to ref's core-owned connection scope.
fn proxy_change_scope(strategy: ProxyChangeBreakMode, group: &str) -> Option<ConnectionScope> {
    ConnectionScope::for_proxy_change(strategy, group.to_owned())
}

/// Application compatibility surface for existing Tauri IPC and lifecycle calls.
pub struct ConnectionInterruptionService;

impl ConnectionInterruptionService {
    pub async fn on_proxy_change(
        api: &ApiClient,
        break_when: ProxyChangeBreakMode,
        group: &str,
    ) -> Result<()> {
        match proxy_change_scope(break_when, group) {
            Some(scope) => interrupt_connections(api, &scope).await.map_err(Into::into),
            None => Ok(()),
        }
    }

    pub async fn on_profile_change(api: &ApiClient, break_when: bool) -> Result<()> {
        if break_when {
            interrupt_connections(api, &ConnectionScope::All)
                .await
                .map_err(Into::into)
        } else {
            Ok(())
        }
    }

    pub async fn on_mode_change(api: &ApiClient, break_when: bool) -> Result<()> {
        if break_when {
            interrupt_connections(api, &ConnectionScope::All)
                .await
                .map_err(Into::into)
        } else {
            Ok(())
        }
    }

    /// Keep the public compatibility entrypoint for multiple chain names.
    pub async fn interrupt_by_chain(api: &ApiClient, chains: &[&str]) -> Result<()> {
        interrupt_by_chains(api, chains).await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_change_policy_is_mapped_without_touching_the_controller() {
        assert_eq!(proxy_change_scope(ProxyChangeBreakMode::Off, "Group"), None);
        assert_eq!(
            proxy_change_scope(ProxyChangeBreakMode::All, "Group"),
            Some(ConnectionScope::All)
        );
        assert_eq!(
            proxy_change_scope(ProxyChangeBreakMode::ProxyGroup, "Group"),
            Some(ConnectionScope::ProxyGroup {
                name: "Group".to_owned()
            })
        );
    }
}

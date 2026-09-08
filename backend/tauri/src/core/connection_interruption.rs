use anyhow::Result;
use chimera_config::clash::config::clash_strategy::ProxyChangeBreakMode;
use futures_util::future::join_all;

use crate::core::clash::api::{self, ConnectionItem};

/// Connection interruption service that handles closing connections based on configuration settings.
pub struct ConnectionInterruptionService;

impl ConnectionInterruptionService {
    /// Interrupt connections when proxy changes.
    pub async fn on_proxy_change(break_when: ProxyChangeBreakMode, group: &str) -> Result<()> {
        match break_when {
            ProxyChangeBreakMode::Off => Ok(()),
            ProxyChangeBreakMode::ProxyGroup => Self::interrupt_by_chain(&[group]).await,
            ProxyChangeBreakMode::All => api::delete_connections(None).await,
        }
    }

    /// Interrupt connections when profile changes.
    pub async fn on_profile_change(break_when: bool) -> Result<()> {
        if break_when {
            api::delete_connections(None).await
        } else {
            Ok(())
        }
    }

    /// Interrupt connections when mode changes.
    pub async fn on_mode_change(break_when: bool) -> Result<()> {
        if break_when {
            api::delete_connections(None).await
        } else {
            Ok(())
        }
    }

    /// Interrupt only connections whose active proxy chain contains one of the supplied names.
    pub async fn interrupt_by_chain(chain: &[&str]) -> Result<()> {
        let connections = api::get_connections().await?.connections;
        let ids = connection_ids_for_chain(&connections, chain);
        let results = join_all(ids.iter().map(|id| api::delete_connections(Some(id)))).await;

        for result in results {
            result?;
        }
        Ok(())
    }
}

fn connection_ids_for_chain(connections: &[ConnectionItem], chain: &[&str]) -> Vec<String> {
    connections
        .iter()
        .filter(|connection| {
            connection
                .chains
                .iter()
                .any(|name| chain.iter().any(|expected| name == expected))
        })
        .map(|connection| connection.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::connection_ids_for_chain;
    use crate::core::clash::api::ConnectionItem;

    fn connection(id: &str, chains: &[&str]) -> ConnectionItem {
        ConnectionItem {
            id: id.to_owned(),
            chains: chains.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn chain_filter_only_selects_related_connections() {
        let connections = vec![
            connection("a", &["Node A", "Auto", "GLOBAL"]),
            connection("b", &["Node B", "Fallback", "GLOBAL"]),
            connection("c", &["DIRECT"]),
        ];

        let ids = connection_ids_for_chain(&connections, &["Auto"]);

        assert_eq!(ids, vec!["a"]);
    }

    #[test]
    fn chain_filter_matches_any_requested_chain_name_exactly() {
        let connections = vec![
            connection("a", &["Auto", "GLOBAL"]),
            connection("b", &["Auto Backup", "GLOBAL"]),
            connection("c", &["Fallback", "GLOBAL"]),
        ];

        let ids = connection_ids_for_chain(&connections, &["Auto", "Fallback"]);

        assert_eq!(ids, vec!["a", "c"]);
    }
}

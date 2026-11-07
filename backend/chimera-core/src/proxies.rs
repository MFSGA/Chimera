//! Proxy operations tied to one running Clash API capability.
//!
//! This module is the independent-core home for proxy coordination, parallel
//! to ref/backend/nyanpasu-core/src/proxies.rs. The longer-lived snapshot
//! actor will be migrated separately. No Tauri handles or IPC transport here.

use std::time::Duration;

use anyhow::{Result, bail};
use clash_api::{IndexMap, ProviderName, ProxyName};
use futures_util::{StreamExt, stream};

use crate::{
    actor_v2::api::{ApiClient, ApiError},
    clash::proxies::NativeProxies,
    client::clash_api::delay_query,
};

pub const PINNED_GROUP_DELAY_CONCURRENCY: usize = 8;

/// Test the currently active group without accidentally clearing its pin.
///
/// An external dashboard may have pinned this group while a frontend cache
/// was fresh, so fetch a new graph before deciding the route. All subsequent
/// requests use the SAME instance-bound API capability, even when the host
/// changes profiles during a fan-out. A stale authority aborts the operation;
/// ordinary per-member test failures report zero and don't stop other members.
/// Delays are not retried: a lost HTTP response can represent a completed test.
pub async fn test_group_delay(
    api: &ApiClient,
    group_name: &ProxyName,
    default_url: &str,
    timeout: Duration,
) -> Result<IndexMap<ProxyName, u16>> {
    let graph = NativeProxies::fetch(api).await?;
    let group = graph
        .global
        .as_ref()
        .filter(|group| &group.name == group_name)
        .or_else(|| graph.groups.iter().find(|group| &group.name == group_name))
        .ok_or_else(|| {
            anyhow::anyhow!("proxy group {group_name} is missing from the controller")
        })?;
    let query = delay_query(
        group.test_url.as_deref(),
        group.expected_status.as_deref(),
        default_url,
        timeout,
    )?;

    if group.fixed.is_none() {
        return Ok(api.group_delay(group_name, &query).await?);
    }

    // Take the immutable member and provider selection from that one snapshot.
    // Do not reuse cached data from a different core instance.
    let members: Vec<_> = group
        .all
        .iter()
        .map(|name| {
            let provider = graph.nodes.get(name).and_then(|node| {
                node.provider
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .or_else(|| {
                        node.provider_name
                            .as_deref()
                            .filter(|name| !name.is_empty())
                    })
                    .map(ProviderName::new)
            });
            (name.clone(), provider)
        })
        .collect();

    let tested = stream::iter(members.into_iter().map(|(name, provider)| {
        let api = api.clone();
        let query = query.clone();
        async move {
            let delay = api.proxy_delay(&name, provider.as_ref(), &query).await;
            (name, delay)
        }
    }))
    .buffered(PINNED_GROUP_DELAY_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    // buffered preserves the group's member order despite concurrent HTTP.
    let mut delays = IndexMap::new();
    for (name, result) in tested {
        match result {
            Ok(value) => {
                delays.insert(name, value.delay);
            }
            Err(ApiError::Stale | ApiError::Unavailable(_)) => {
                // Never convert a retired core into an apparently successful
                // group test with zero samples.
                bail!("the Clash API capability changed during group testing");
            }
            Err(ApiError::Timeout | ApiError::Protocol(_)) => {
                delays.insert(name, 0);
            }
        }
    }
    api.check_binding().await?;
    Ok(delays)
}

/// Core-owned generation, freshness and publish state shared by both proxy
/// representations. The Tauri adapter provides UI notification only.
/// A delayed response from an older profile can never publish over a newer
/// profile. The last value remains visible but stale after invalidation.
#[derive(Debug)]
pub struct ProxySnapshotState<T> {
    value: T,
    generation: u64,
    checksum: Option<u32>,
    updated_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPublish {
    Stale,
    Unchanged,
    Published,
}

impl<T> ProxySnapshotState<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            generation: 0,
            checksum: None,
            updated_at: 0,
        }
    }

    /// Restore a cache snapshot from the application's existing guard.
    pub fn from_parts(value: T, generation: u64, checksum: Option<u32>, updated_at: u64) -> Self {
        Self {
            value,
            generation,
            checksum,
            updated_at,
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn checksum(&self) -> Option<u32> {
        self.checksum
    }

    pub fn is_fresh(&self, now: u64, max_age_seconds: u64) -> bool {
        self.checksum.is_some()
            && self.updated_at > 0
            && now >= self.updated_at
            && now - self.updated_at <= max_age_seconds
    }

    pub fn ensure_generation(&self, generation: u64) -> Result<()> {
        anyhow::ensure!(
            self.generation == generation,
            "proxy selection was invalidated by a profile change"
        );
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.checksum = None;
        self.updated_at = 0;
    }

    pub fn replace(&mut self, value: T, checksum: u32, now: u64) {
        self.value = value;
        self.checksum = Some(checksum);
        self.updated_at = now;
    }

    pub fn publish_if_generation(
        &mut self,
        generation: u64,
        value: T,
        checksum: u32,
        now: u64,
    ) -> SnapshotPublish {
        if self.generation != generation {
            return SnapshotPublish::Stale;
        }
        if self.checksum == Some(checksum) {
            // A successful identical refresh still makes this snapshot fresh;
            // no subscriber needs another copy or notification.
            self.updated_at = now;
            return SnapshotPublish::Unchanged;
        }
        self.replace(value, checksum, now);
        SnapshotPublish::Published
    }
}

#[cfg(test)]
#[path = "proxies_tests.rs"]
mod tests;

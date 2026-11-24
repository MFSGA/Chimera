//! Runtime-owned effects, composed after Streams and Core have started.
//! Based on ref CoreRuntimeEffects; presentation and system owners are injected.
use super::{
    plan::{ApplicationEffect, ApplicationEffectPlan, EffectKind, TransparentProxyDesired},
    ports::ApplicationEffectsPort,
    status::{EffectRevision, EffectStatus, degraded, healthy},
};
use crate::{actor_v2::CoreClient, clash::ws::StreamsClient};
use std::sync::Arc;

pub struct CoreRuntimeEffects {
    executor: Arc<dyn ApplicationEffectsPort>,
    streams: StreamsClient,
    core: CoreClient,
}
impl CoreRuntimeEffects {
    pub fn new(
        executor: Arc<dyn ApplicationEffectsPort>,
        streams: StreamsClient,
        core: CoreClient,
    ) -> Self {
        Self {
            executor,
            streams,
            core,
        }
    }
    async fn apply_transparent_proxy(
        &self,
        revision: EffectRevision,
        desired: &TransparentProxyDesired,
    ) -> EffectStatus {
        use chimera_config::clash::config::TransparentProxyMode as ConfigMode;
        use chimera_ipc::api::network::transparent_proxy::{
            NetworkTransparentProxyMode as WireMode, NetworkTransparentProxyRequest,
        };

        let enabled = desired.config.mode != ConfigMode::Disabled;
        if !cfg!(target_os = "linux") && !enabled {
            return healthy(EffectKind::TransparentProxy, revision);
        }
        let port = if enabled {
            match desired.port.filter(|port| *port != 0) {
                Some(port) => port,
                None => {
                    return degraded(
                        EffectKind::TransparentProxy,
                        revision,
                        "transparent_proxy_failed",
                        "transparent proxy listener port is not resolved".into(),
                        true,
                    );
                }
            }
        } else {
            0
        };
        let mode = match desired.config.mode {
            ConfigMode::Disabled => WireMode::Disabled,
            ConfigMode::Redir => WireMode::Redir,
            ConfigMode::Tproxy => WireMode::Tproxy,
        };
        let request = NetworkTransparentProxyRequest {
            mode,
            port,
            local: desired.config.local,
            interfaces: desired.config.interfaces.clone(),
            ipv6: desired.config.ipv6,
            expected_revision: chimera_ipc::api::status::RevisionIdInfo {
                epoch: 0,
                generation: 0,
                effective_hash: String::new(),
            },
        };
        let result = self
            .core
            .reconcile_transparent_proxy(request)
            .await
            .map_err(|error| (error.to_string(), error.retryable));
        let status = match result {
            Ok(status) => status,
            Err((message, retryable)) => {
                return degraded(
                    EffectKind::TransparentProxy,
                    revision,
                    "transparent_proxy_failed",
                    message,
                    retryable,
                );
            }
        };
        if !status.supported && !enabled && !status.active {
            return healthy(EffectKind::TransparentProxy, revision);
        }
        if let Some(error) = status.error {
            return degraded(
                EffectKind::TransparentProxy,
                revision,
                "transparent_proxy_failed",
                error,
                true,
            );
        }
        if !status.supported || status.active != enabled || (enabled && status.mode != Some(mode)) {
            return degraded(
                EffectKind::TransparentProxy,
                revision,
                "transparent_proxy_failed",
                "the Service did not reach the requested transparent proxy state".into(),
                true,
            );
        }
        healthy(EffectKind::TransparentProxy, revision)
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for CoreRuntimeEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        // Preserve the plan's order, including the system-proxy owner's grouped request.
        let mut statuses = Vec::with_capacity(plan.effects().len());
        let mut others = Vec::new();
        for effect in plan.effects() {
            match effect {
                ApplicationEffect::CoreLogStorage(_) | ApplicationEffect::TransparentProxy(_) => {}
                effect => others.push(effect.clone()),
            }
        }
        let owned = if others.is_empty() {
            Vec::new()
        } else {
            self.executor
                .apply(revision, ApplicationEffectPlan::from_effects(others))
                .await
        };
        for effect in plan.effects() {
            let status = match effect {
                ApplicationEffect::CoreLogStorage(settings) => {
                    match self.streams.core_logs().configure(*settings).await {
                        Ok(()) => healthy(EffectKind::CoreLogStorage, revision),
                        Err(error) => degraded(
                            EffectKind::CoreLogStorage,
                            revision,
                            "core_log_storage_failed",
                            error.to_string(),
                            false,
                        ),
                    }
                }
                ApplicationEffect::TransparentProxy(desired) => {
                    self.apply_transparent_proxy(revision, desired).await
                }
                effect => owned
                    .iter()
                    .find(|status| status.kind == effect.kind())
                    .cloned()
                    .unwrap_or_else(|| {
                        degraded(
                            effect.kind(),
                            revision,
                            "effect_owner_silent",
                            format!("the owner of {:?} reported no status", effect.kind()),
                            true,
                        )
                    }),
            };
            statuses.push(status);
        }
        statuses
    }
    async fn shutdown(&self) -> Vec<EffectStatus> {
        self.executor.shutdown().await
    }
}

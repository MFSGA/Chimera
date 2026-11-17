use std::time::Duration;

use chimera_core::actor_v2::router::{CoreStatusProjection, EndpointConnectivity};
use chimera_ipc::api::status::{CoreControllerInfo, CoreStateDetail};

use crate::{client::ChimeraClient, core::clash::api::ClashRuntimeConfig};

use super::model::AgentRoutingMode;

const CORE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedCoreConfig {
    pub routing_mode: Option<AgentRoutingMode>,
    pub tun_enabled: Option<bool>,
    pub tun_device: Option<String>,
    pub tun_auto_route: Option<bool>,
    pub tun_route_addresses: Vec<String>,
}

/// The Agent cannot validate a legacy Facade controller, then call a ref
/// CoreActor API from a different owner. Both readings must share the same
/// host, generation, running instance, controller and applied revision.
pub(super) async fn observed_core_config(client: &ChimeraClient) -> Result<ObservedCoreConfig, ()> {
    let before = client
        .refreshed_routed_core_status()
        .await
        .map_err(|_| ())?;
    let instance = probe_running_instance(&before)?;
    let api = client.clash_api_client().await.map_err(|_| ())?;
    if api.instance_id() != Some(instance) || api.is_revoked() {
        return Err(());
    }

    let config = tokio::time::timeout(CORE_PROBE_TIMEOUT, api.get_configs())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;

    let after = client
        .refreshed_routed_core_status()
        .await
        .map_err(|_| ())?;
    if api.is_revoked() || !same_probe_authority(&before, &after, instance) {
        return Err(());
    }
    Ok(project_core_config(config))
}

/// An unknown Core projection must never become an inferred Stopped state.
fn probe_running_instance(status: &CoreStatusProjection) -> Result<&str, ()> {
    if !matches!(status.connectivity, EndpointConnectivity::Connected) {
        return Err(());
    }
    let snapshot = status.snapshot.as_ref().ok_or(())?;
    if !matches!(snapshot.state, Some(CoreStateDetail::Running { .. })) {
        return Err(());
    }
    if !local_agent_controller(snapshot.controller.as_ref().ok_or(())?) {
        return Err(());
    }
    snapshot
        .instance_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or(())
}

fn same_probe_authority(
    before: &CoreStatusProjection,
    after: &CoreStatusProjection,
    instance: &str,
) -> bool {
    if before.host != after.host || before.generation != after.generation {
        return false;
    }
    let (Some(first), Some(last)) = (&before.snapshot, &after.snapshot) else {
        return false;
    };
    probe_running_instance(before) == Ok(instance)
        && probe_running_instance(after) == Ok(instance)
        && first.controller == last.controller
        && first.revision == last.revision
        && first.source_hash == last.source_hash
}

/// A DNS hostname (even localhost) is not an authority check: DNS could
/// resolve outside loopback. Local IPC transports do not use HTTP/DNS.
fn local_agent_controller(controller: &CoreControllerInfo) -> bool {
    match controller {
        CoreControllerInfo::UnixSocket(_) => true,
        CoreControllerInfo::NamedPipe(path) => path
            .to_str()
            .is_some_and(|name| name.starts_with(r"\\.\pipe\") && name.len() > r"\\.\pipe\".len()),
        CoreControllerInfo::Http(address) => {
            let Ok(url) = url::Url::parse(address) else {
                return false;
            };
            if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
                return false;
            }
            match url.host() {
                Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                _ => false,
            }
        }
    }
}

pub(super) async fn observed_routing_mode(client: &ChimeraClient) -> Result<AgentRoutingMode, ()> {
    observed_core_config(client).await?.routing_mode.ok_or(())
}

fn project_core_config(config: ClashRuntimeConfig) -> ObservedCoreConfig {
    let (tun_enabled, tun_device, tun_auto_route, tun_route_addresses) = config
        .tun
        .map(|tun| {
            let mut routes = tun.route_address;
            routes.extend(tun.inet4_route_address);
            routes.extend(tun.inet6_route_address);
            routes.sort_unstable();
            routes.dedup();
            (
                Some(tun.enable),
                (!tun.device.trim().is_empty()).then_some(tun.device),
                Some(tun.auto_route),
                routes,
            )
        })
        .unwrap_or((None, None, None, Vec::new()));
    ObservedCoreConfig {
        routing_mode: config.mode.as_deref().and_then(AgentRoutingMode::parse),
        tun_enabled,
        tun_device,
        tun_auto_route,
        tun_route_addresses,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        local_agent_controller, probe_running_instance, project_core_config, same_probe_authority,
    };
    use crate::{
        core::clash::api::{ClashRuntimeConfig, RuntimeTun},
        features::agent::model::AgentRoutingMode,
    };
    use chimera_core::actor_v2::{
        endpoint::{CoreStatusSnapshot, ExecutionHost},
        router::{CoreStatusProjection, EndpointConnectivity},
    };
    use chimera_ipc::api::status::{CoreControllerInfo, CoreStateDetail, RevisionIdInfo};

    fn running_projection() -> CoreStatusProjection {
        CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 4,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("agent-core".into()),
                controller: Some(CoreControllerInfo::Http("http://127.0.0.1:9090".into())),
                state: Some(CoreStateDetail::Running { epoch: 12, pid: 5 }),
                state_changed_at: 2,
                revision: Some(RevisionIdInfo {
                    epoch: 12,
                    generation: 3,
                    effective_hash: "applied".into(),
                }),
                source_hash: Some("source".into()),
                healthy: Some(true),
                applied_kind: None,
            }),
        }
    }

    #[test]
    fn controller_probe_accepts_only_local_core_endpoints() {
        assert!(local_agent_controller(&CoreControllerInfo::Http(
            "http://127.0.0.1:9090".into()
        )));
        assert!(local_agent_controller(&CoreControllerInfo::Http(
            "http://[::1]:9090".into()
        )));
        for remote in [
            "http://192.168.1.2:9090",
            "http://example.com:9090",
            "http://localhost:9090",
            "https://127.0.0.1:9090",
            "http://user:secret@127.0.0.1:9090",
            "not an url",
        ] {
            assert!(
                !local_agent_controller(&CoreControllerInfo::Http(remote.into())),
                "reject unsafe controller: {remote}"
            );
        }
        assert!(local_agent_controller(&CoreControllerInfo::UnixSocket(
            "/tmp/chimera.sock".into()
        )));
        assert!(local_agent_controller(&CoreControllerInfo::NamedPipe(
            r"\\.\pipe\chimera".into()
        )));
        assert!(!local_agent_controller(&CoreControllerInfo::NamedPipe(
            r"\\remote-server\pipe\chimera".into()
        )));
        assert!(!local_agent_controller(&CoreControllerInfo::NamedPipe(
            "chimera-pipe".into()
        )));
    }

    #[test]
    fn agent_probe_rejects_stale_host_generation_and_instance() {
        let before = running_projection();
        assert_eq!(probe_running_instance(&before), Ok("agent-core"));
        assert!(same_probe_authority(&before, &before, "agent-core"));

        let mut sampled = before.clone();
        sampled.snapshot.as_mut().unwrap().state_changed_at += 1;
        assert!(same_probe_authority(&before, &sampled, "agent-core"));

        let mut handoff = before.clone();
        handoff.host = ExecutionHost::Service;
        handoff.generation += 1;
        assert!(!same_probe_authority(&before, &handoff, "agent-core"));

        let mut replaced = before.clone();
        replaced.snapshot.as_mut().unwrap().instance_id = Some("new-core".into());
        assert!(!same_probe_authority(&before, &replaced, "agent-core"));

        let mut newer_revision = before.clone();
        newer_revision
            .snapshot
            .as_mut()
            .unwrap()
            .revision
            .as_mut()
            .unwrap()
            .generation += 1;
        assert!(!same_probe_authority(
            &before,
            &newer_revision,
            "agent-core"
        ));

        let mut remote = before.clone();
        remote.snapshot.as_mut().unwrap().controller =
            Some(CoreControllerInfo::Http("http://192.168.1.1:9090".into()));
        assert!(probe_running_instance(&remote).is_err());
        assert!(!same_probe_authority(&before, &remote, "agent-core"));

        let mut starting = before.clone();
        starting.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Starting { epoch: 13 });
        assert!(probe_running_instance(&starting).is_err());

        let mut offline = before;
        offline.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Local,
            reason: "disconnected".into(),
        };
        assert!(probe_running_instance(&offline).is_err());
    }

    #[test]
    fn core_projection_keeps_tun_and_routing_observation_together() {
        let observed = project_core_config(ClashRuntimeConfig {
            mode: Some("rule".into()),
            tun: Some(RuntimeTun {
                enable: true,
                device: "Meta".into(),
                auto_route: true,
                route_address: vec!["0.0.0.0/1".into()],
                inet4_route_address: vec!["128.0.0.0/1".into()],
                inet6_route_address: vec!["::/1".into(), "0.0.0.0/1".into()],
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(observed.routing_mode, Some(AgentRoutingMode::Rule));
        assert_eq!(observed.tun_enabled, Some(true));
        assert_eq!(observed.tun_device.as_deref(), Some("Meta"));
        assert_eq!(observed.tun_auto_route, Some(true));
        assert_eq!(
            observed.tun_route_addresses,
            vec!["0.0.0.0/1", "128.0.0.0/1", "::/1"]
        );
    }

    #[test]
    fn core_projection_preserves_unknown_tun_state() {
        let observed = project_core_config(ClashRuntimeConfig {
            mode: Some("invalid".into()),
            ..Default::default()
        });

        assert_eq!(observed.routing_mode, None);
        assert_eq!(observed.tun_enabled, None);
        assert_eq!(observed.tun_device, None);
        assert_eq!(observed.tun_auto_route, None);
        assert!(observed.tun_route_addresses.is_empty());
    }
}

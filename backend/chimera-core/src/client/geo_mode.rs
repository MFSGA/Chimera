//! Ref-style core-instance geodata mode source with Chimera's lease proof.
use crate::{
    actor_v2::{
        CoreClient,
        router::{CoreStatusProjection, EndpointConnectivity},
    },
    geo::{CoreGeodataBinding, CoreGeodataModeSource, GeodataMode},
};
use anyhow::Context as _;
use async_trait::async_trait;
use chimera_ipc::api::status::CoreStateDetail;

/// Geodata selection must follow the same ref CoreActor instance as
/// connection traffic. Never use a second Facade status or a stale controller.
pub struct ActiveCoreGeodataMode(pub CoreClient);

fn running_actor_geodata_instance(projection: &CoreStatusProjection) -> anyhow::Result<&str> {
    anyhow::ensure!(
        matches!(projection.connectivity, EndpointConnectivity::Connected),
        "the CoreActor host is not connected for geodata inspection"
    );
    let snapshot = projection
        .snapshot
        .as_ref()
        .context("the CoreActor has no authoritative geodata status")?;
    anyhow::ensure!(
        matches!(snapshot.state, Some(CoreStateDetail::Running { .. })),
        "the CoreActor is not running for geodata inspection"
    );
    snapshot
        .instance_id
        .as_deref()
        .filter(|instance| !instance.is_empty())
        .context("the CoreActor has no confirmed geodata instance")
}

fn actor_geodata_lease_still_owned(
    before: &CoreStatusProjection,
    after: &CoreStatusProjection,
    api_instance: &str,
) -> bool {
    if before.host != after.host || before.generation != after.generation {
        return false;
    }
    let (Ok(first), Ok(last)) = (
        running_actor_geodata_instance(before),
        running_actor_geodata_instance(after),
    ) else {
        return false;
    };
    first == api_instance
        && last == api_instance
        && before.snapshot.as_ref().and_then(|s| s.revision.as_ref())
            == after.snapshot.as_ref().and_then(|s| s.revision.as_ref())
}

#[async_trait]
impl CoreGeodataModeSource for ActiveCoreGeodataMode {
    async fn mode(&self) -> anyhow::Result<CoreGeodataBinding> {
        let before = self.0.refresh_status().await?;
        let running_instance = running_actor_geodata_instance(&before)?;
        let api = self.0.api_client().await?;
        anyhow::ensure!(
            api.instance_id() == Some(running_instance) && !api.is_revoked(),
            "the CoreActor changed while acquiring the geodata API lease"
        );
        let config = api.get_configs().await?;
        let after = self.0.refresh_status().await?;
        anyhow::ensure!(
            !api.is_revoked()
                && actor_geodata_lease_still_owned(
                    &before,
                    &after,
                    api.instance_id().unwrap_or_default(),
                ),
            "the CoreActor changed host, instance, or applied revision while reading geodata mode"
        );
        Ok(CoreGeodataBinding {
            instance_id: running_instance.to_owned(),
            mode: if config.geodata_mode == Some(true) {
                GeodataMode::Dat
            } else {
                GeodataMode::Mmdb
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_v2::endpoint::{CoreStatusSnapshot, ExecutionHost};
    use chimera_ipc::api::status::RevisionIdInfo;

    fn connected_running() -> CoreStatusProjection {
        CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 3,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("core-A".into()),
                controller: None,
                state: Some(CoreStateDetail::Running { epoch: 7, pid: 42 }),
                state_changed_at: 11,
                revision: Some(RevisionIdInfo {
                    epoch: 7,
                    generation: 2,
                    effective_hash: "effective-A".into(),
                }),
                source_hash: Some("source-A".into()),
                healthy: Some(true),
                applied_kind: None,
            }),
        }
    }

    #[test]
    fn geodata_mode_requires_connected_running_ref_core() {
        let running = connected_running();
        assert_eq!(running_actor_geodata_instance(&running).unwrap(), "core-A");

        let mut unknown = running.clone();
        unknown.snapshot = None;
        assert!(running_actor_geodata_instance(&unknown).is_err());

        let mut stopped = running.clone();
        stopped.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Stopped { reason: None });
        assert!(running_actor_geodata_instance(&stopped).is_err());

        let mut switching = running;
        switching.connectivity = EndpointConnectivity::HandingOff {
            from: ExecutionHost::Local,
            to: ExecutionHost::Service,
        };
        assert!(running_actor_geodata_instance(&switching).is_err());
    }

    #[test]
    fn geodata_mode_lease_is_fenced_by_host_generation_instance_and_revision() {
        let before = connected_running();
        let mut sampled = before.clone();
        sampled.snapshot.as_mut().unwrap().state_changed_at += 1;
        assert!(actor_geodata_lease_still_owned(&before, &sampled, "core-A"));

        let mut other_host = before.clone();
        other_host.host = ExecutionHost::Service;
        assert!(!actor_geodata_lease_still_owned(
            &before,
            &other_host,
            "core-A"
        ));

        let mut newer_generation = before.clone();
        newer_generation.generation += 1;
        assert!(!actor_geodata_lease_still_owned(
            &before,
            &newer_generation,
            "core-A"
        ));

        let mut new_instance = before.clone();
        new_instance.snapshot.as_mut().unwrap().instance_id = Some("core-B".into());
        assert!(!actor_geodata_lease_still_owned(
            &before,
            &new_instance,
            "core-A"
        ));

        let mut revised = before.clone();
        revised
            .snapshot
            .as_mut()
            .unwrap()
            .revision
            .as_mut()
            .unwrap()
            .generation += 1;
        assert!(!actor_geodata_lease_still_owned(
            &before, &revised, "core-A"
        ));

        let mut degraded = before.clone();
        degraded.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Local,
            reason: "endpoint unavailable".into(),
        };
        assert!(!actor_geodata_lease_still_owned(
            &before, &degraded, "core-A"
        ));
    }
}

//! Verifies whether an observed running instance matches a confirmed application.
use chimera_core_manager::CoreKind;
use chimera_ipc::api::status::CoreStateDetail;

use crate::client::runtime::RuntimeApplyReceipt;
use chimera_core::{
    actor_v2::{
        CoreStatusProjection, EndpointConnectivity,
        endpoint::{CoreStatusSnapshot, ExecutionHost},
    },
    client::core_spec::core_kind,
};
use chimera_ipc::api::status::ConfigRevisionInfo;

/// What the app can observe about the runtime right now. Built from a status
/// read, or from the report of the operation that claimed to restore it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObservedRuntime {
    pub host: ExecutionHost,
    pub instance_id: Option<String>,
    pub binding: Option<ConfigRevisionInfo>,
    /// A direct compatibility endpoint has no router generation.
    pub generation: Option<u64>,
    pub applied_kind: Option<CoreKind>,
    /// `None` means the host published no state this app can trust. It is not
    /// `Stopped` and it is not `Running`; it is an absent fact.
    pub state: Option<CoreStateDetail>,
    /// The source hash of the revision the host reports as applied. This is
    /// the document identity that survives a restart; the effective hash of
    /// the same document does not, because it covers the epoch-specific
    /// controller endpoint the runtime writes into it.
    pub source_hash: Option<String>,
}

impl ObservedRuntime {
    pub fn from_status(status: &CoreStatusProjection) -> Self {
        // Disconnected projections may still carry the last cached snapshot.
        let snapshot = matches!(status.connectivity, EndpointConnectivity::Connected)
            .then_some(status.snapshot.as_ref())
            .flatten();
        Self::from_snapshot(status.host, Some(status.generation), snapshot)
    }

    pub fn from_snapshot(
        host: ExecutionHost,
        generation: Option<u64>,
        snapshot: Option<&CoreStatusSnapshot>,
    ) -> Self {
        Self {
            host,
            instance_id: snapshot.and_then(|s| s.instance_id.clone()),
            binding: snapshot
                .and_then(|s| s.revision.clone())
                .map(|revision| ConfigRevisionInfo {
                    epoch: revision.epoch,
                    generation: revision.generation,
                    effective_hash: revision.effective_hash,
                    source_hash: snapshot
                        .and_then(|s| s.source_hash.clone())
                        .unwrap_or_default(),
                }),
            generation,
            applied_kind: snapshot.and_then(|s| s.applied_kind),
            state: snapshot.and_then(|s| s.state.clone()),
            source_hash: snapshot.and_then(|s| s.source_hash.clone()),
        }
    }
}

/// Whether the running instance still runs the confirmed application.
/// A different connection must prove that the very same process survived.
pub(crate) fn matches_confirmed_apply(
    receipt: &RuntimeApplyReceipt,
    observed: &ObservedRuntime,
) -> bool {
    observed.host == receipt.host
        && observed.binding.as_ref() == Some(&receipt.applied_revision)
        && (receipt
            .controller_generation
            .zip(observed.generation)
            .is_some_and(|(recorded, current)| recorded == current)
            || receipt
                .instance_id
                .as_ref()
                .is_some_and(|id| observed.instance_id.as_ref() == Some(id)))
        && observed.source_hash.as_deref() == Some(receipt.applied_revision.source_hash.as_str())
        && matches!(observed.state, Some(CoreStateDetail::Running { epoch, .. })
            if epoch == receipt.applied_revision.epoch)
        && observed
            .applied_kind
            .is_none_or(|kind| kind == receipt.core_spec.kind)
}

/// The core kind a confirmed apply proves when the status does not publish one
/// (`TODO(service-applied-kind)` keeps a service host silent): the receipt must
/// still match the observed runtime, so its target names the binary in use.
pub(crate) fn confirmed_apply_kind(
    receipt: &RuntimeApplyReceipt,
    observed: &ObservedRuntime,
) -> Option<CoreKind> {
    matches_confirmed_apply(receipt, observed).then(|| core_kind(receipt.target_core))
}

/// The legacy direct port cannot supply a controller generation. Only an
/// entirely unbound compatibility receipt may use the old revision-only proof.
pub(crate) fn matches_endpoint_apply(
    receipt: &RuntimeApplyReceipt,
    host: ExecutionHost,
    status: &CoreStatusSnapshot,
) -> bool {
    let observed = ObservedRuntime::from_snapshot(host, None, Some(status));
    if receipt.controller_generation.is_none() && receipt.instance_id.is_none() {
        return observed.host == receipt.host
            && observed.binding.as_ref() == Some(&receipt.applied_revision)
            && observed.source_hash.as_deref()
                == Some(receipt.applied_revision.source_hash.as_str())
            && matches!(observed.state, Some(CoreStateDetail::Running { epoch, .. })
                if epoch == receipt.applied_revision.epoch)
            && observed
                .applied_kind
                .is_none_or(|kind| kind == receipt.core_spec.kind);
    }
    matches_confirmed_apply(receipt, &observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn receipt() -> RuntimeApplyReceipt {
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.applied_revision = ConfigRevisionInfo {
            epoch: 4,
            generation: 11,
            source_hash: "source-a".into(),
            effective_hash: "effective-a".into(),
        };
        receipt.controller_generation = Some(3);
        receipt
    }
}
#[cfg(test)]
mod confirmed_apply_tests {
    use super::*;

    #[test]
    fn process_identity_is_required_only_across_connections() {
        for (generation, recorded, observed_id, expected) in [
            (3, Some("a"), Some("b"), true),
            (4, Some("a"), Some("a"), true),
            (4, Some("a"), Some("b"), false),
            (4, None, Some("a"), false),
        ] {
            let mut receipt = super::tests::receipt();
            receipt.instance_id = recorded.map(str::to_owned);
            let binding = receipt.applied_revision.clone();
            let observed = ObservedRuntime {
                host: receipt.host,
                instance_id: observed_id.map(str::to_owned),
                binding: Some(binding),
                generation: Some(generation),
                applied_kind: None,
                state: Some(CoreStateDetail::Running {
                    epoch: receipt.applied_revision.epoch,
                    pid: 42,
                }),
                source_hash: Some(receipt.applied_revision.source_hash.clone()),
            };
            assert_eq!(
                matches_confirmed_apply(&receipt, &observed),
                expected,
                "{generation}/{recorded:?}/{observed_id:?}"
            );
        }
    }

    // Guards revision and epoch matching across reconnection to the same process.
    #[test]
    fn confirmed_apply_matches_only_the_same_running_revision() {
        let mut receipt = super::tests::receipt();
        receipt.instance_id = Some("same-process".into());
        let mut observed = ObservedRuntime {
            host: receipt.host,
            instance_id: Some("same-process".into()),
            binding: Some(receipt.applied_revision.clone()),
            generation: receipt.controller_generation,
            applied_kind: None,
            state: Some(CoreStateDetail::Running {
                epoch: receipt.applied_revision.epoch,
                pid: 42,
            }),
            source_hash: Some(receipt.applied_revision.source_hash.clone()),
        };
        observed.generation = Some(4);
        assert!(matches_confirmed_apply(&receipt, &observed));
        for field in [
            "host",
            "epoch",
            "generation",
            "effective_hash",
            "source_hash",
            "running_epoch",
            "stopped",
            "no_binding",
            "no_source",
        ] {
            let mut mismatch = observed.clone();
            match field {
                "host" => mismatch.host = ExecutionHost::Service,
                "epoch" => mismatch.binding.as_mut().unwrap().epoch += 1,
                "generation" => mismatch.binding.as_mut().unwrap().generation += 1,
                "effective_hash" => mismatch.binding.as_mut().unwrap().effective_hash.push('x'),
                "source_hash" => mismatch.source_hash = Some("different".into()),
                "running_epoch" => {
                    mismatch.state = Some(CoreStateDetail::Running { epoch: 99, pid: 42 })
                }
                "stopped" => mismatch.state = Some(CoreStateDetail::Stopped { reason: None }),
                "no_binding" => mismatch.binding = None,
                "no_source" => mismatch.source_hash = None,
                _ => unreachable!(),
            }
            assert!(!matches_confirmed_apply(&receipt, &mismatch), "{field}");
        }
    }

    #[test]
    fn disconnected_projection_cannot_reuse_cached_running_evidence() {
        use chimera_ipc::api::status::RevisionIdInfo;

        let receipt = super::tests::receipt();
        let mut status = CoreStatusProjection {
            host: receipt.host,
            generation: receipt.controller_generation.unwrap(),
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("same-process".into()),
                controller: None,
                state: Some(CoreStateDetail::Running {
                    epoch: receipt.applied_revision.epoch,
                    pid: 42,
                }),
                state_changed_at: 1,
                revision: Some(RevisionIdInfo {
                    epoch: receipt.applied_revision.epoch,
                    generation: receipt.applied_revision.generation,
                    effective_hash: receipt.applied_revision.effective_hash.clone(),
                }),
                source_hash: Some(receipt.applied_revision.source_hash.clone()),
                healthy: Some(true),
                applied_kind: Some(CoreKind::Mihomo),
            }),
        };
        assert!(matches_confirmed_apply(
            &receipt,
            &ObservedRuntime::from_status(&status)
        ));
        status.connectivity = EndpointConnectivity::Degraded {
            desired: receipt.host,
            reason: "lost connection".into(),
        };
        assert!(!matches_confirmed_apply(
            &receipt,
            &ObservedRuntime::from_status(&status)
        ));
        status.connectivity = EndpointConnectivity::Connected;
        status.snapshot.as_mut().unwrap().applied_kind = Some(CoreKind::ChimeraClient);
        assert!(!matches_confirmed_apply(
            &receipt,
            &ObservedRuntime::from_status(&status)
        ));
        status.snapshot.as_mut().unwrap().applied_kind = None;
        let mut unbound = receipt.clone();
        unbound.controller_generation = None;
        assert!(!matches_confirmed_apply(
            &unbound,
            &ObservedRuntime::from_status(&status)
        ));
        // Direct ports cannot claim that the router generation was observed.
        assert!(!matches_endpoint_apply(
            &receipt,
            receipt.host,
            status.snapshot.as_ref().unwrap()
        ));
        unbound.instance_id = Some("same-process".into());
        assert!(matches_confirmed_apply(
            &unbound,
            &ObservedRuntime::from_status(&status)
        ));
        assert!(matches_endpoint_apply(
            &unbound,
            receipt.host,
            status.snapshot.as_ref().unwrap()
        ));
    }

    /// A matching receipt recovers the confirmed apply's core kind for hosts
    /// that do not publish the applied kind (service mode).
    #[test]
    fn a_confirmed_apply_names_its_core_kind_only_while_it_still_runs() {
        let receipt = super::tests::receipt();
        let running = ObservedRuntime {
            host: receipt.host,
            instance_id: None,
            binding: Some(receipt.applied_revision.clone()),
            generation: receipt.controller_generation,
            applied_kind: None,
            state: Some(CoreStateDetail::Running {
                epoch: receipt.applied_revision.epoch,
                pid: 42,
            }),
            source_hash: Some(receipt.applied_revision.source_hash.clone()),
        };
        assert_eq!(
            confirmed_apply_kind(&receipt, &running),
            Some(chimera_core_manager::CoreKind::Mihomo)
        );

        let mut replaced = running.clone();
        replaced.source_hash = Some("different".into());
        assert_eq!(confirmed_apply_kind(&receipt, &replaced), None);

        let mut stopped = running;
        stopped.state = Some(CoreStateDetail::Stopped { reason: None });
        assert_eq!(confirmed_apply_kind(&receipt, &stopped), None);
    }
}

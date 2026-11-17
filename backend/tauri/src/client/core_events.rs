//! Ref CoreActor projection -> Chimera UI and runtime Effects bridge.
//!
//! The router is the sole owner of host/generation state. Its status pump may
//! publish an unchanged frame every two seconds; these notifications must NOT
//! cause periodic OS proxy mutations or gratuitous UI refreshes.

use chimera_core::actor_v2::{endpoint::CoreStatusSnapshot, router::CoreStatusProjection};

use super::ChimeraClient;

/// Exclude periodic sample timestamps. Health-only changes update the UI,
/// but must NOT reapply OS networking side effects (which use a separate
/// runtime-identity predicate below).
fn runtime_materially_changed(old: &CoreStatusProjection, new: &CoreStatusProjection) -> bool {
    runtime_binding_changed(old, new)
        || old.snapshot.as_ref().and_then(|snapshot| snapshot.healthy)
            != new.snapshot.as_ref().and_then(|snapshot| snapshot.healthy)
}

/// A new host, instance or revision may need reapplication even when the
/// listener port is unchanged. A health-only report does not change the
/// expected revision and must not cause a fresh transparent-proxy OS write.
fn runtime_binding_changed(old: &CoreStatusProjection, new: &CoreStatusProjection) -> bool {
    if old.host != new.host
        || old.generation != new.generation
        || old.connectivity != new.connectivity
    {
        return true;
    }
    fn changed(a: Option<&CoreStatusSnapshot>, b: Option<&CoreStatusSnapshot>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => {
                a.instance_id != b.instance_id
                    || a.state != b.state
                    || a.revision != b.revision
                    || a.source_hash != b.source_hash
                    || a.applied_kind != b.applied_kind
            }
            (None, None) => false,
            _ => true,
        }
    }
    changed(old.snapshot.as_ref(), new.snapshot.as_ref())
}

impl ChimeraClient {
    /// Start one tracked CoreActor watch after the Client is fully composed.
    /// Call from inside the Tauri runtime: TaskTracker::spawn needs a reactor.
    /// The observer never owns or re-selects an endpoint and exits when the
    /// parent Client's shutdown token is canceled.
    pub(crate) fn start_core_status_observer(&self) -> bool {
        let Some(core) = &self.inner.core_api else {
            // Test-only legacy clients do not synthesize an extra actor.
            return true;
        };
        let mut updates = core.subscribe();
        let shutdown = self.shutdown_child_token();
        let client = self.clone();
        self.spawn_tracked(async move {
            let mut previous = updates.borrow_and_update().clone();
            loop {
                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break,
                    changed = updates.changed() => {
                        if changed.is_err() { break; }
                        let current = updates.borrow_and_update().clone();
                        let ui_changed = runtime_materially_changed(&previous, &current);
                        let binding_changed = runtime_binding_changed(&previous, &current);
                        previous = current;
                        if shutdown.is_cancelled() {
                            break;
                        }
                        if ui_changed {
                            // UI state is a notification, not an independent
                            // poll or a cached authority. The typed Actor
                            // projection remains the authoritative state.
                            client.inner.ui_sink.refresh_clash();
                        }
                        if binding_changed {
                            #[cfg(not(any(target_os = "android", target_os = "ios")))]
                            client.publish_runtime_port_binding().await;
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::actor_v2::{endpoint::ExecutionHost, router::EndpointConnectivity};
    use chimera_ipc::api::status::{CoreStateDetail, RevisionIdInfo};

    fn projection() -> CoreStatusProjection {
        CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 0,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("instance-1".to_owned()),
                controller: None,
                state: Some(CoreStateDetail::Running { epoch: 3, pid: 12 }),
                state_changed_at: 100,
                revision: Some(RevisionIdInfo {
                    epoch: 3,
                    generation: 1,
                    effective_hash: "config-A".into(),
                }),
                source_hash: Some("source-A".into()),
                healthy: Some(true),
                applied_kind: None,
            }),
        }
    }

    #[test]
    fn repeated_ref_pump_samples_do_not_reapply_system_effects() {
        let before = projection();
        let mut sample = before.clone();
        sample.snapshot.as_mut().unwrap().state_changed_at = 101;
        assert!(!runtime_materially_changed(&before, &sample));
        assert!(!runtime_binding_changed(&before, &sample));

        // The entire channel can coalesce several identical status frames.
        assert!(!runtime_materially_changed(&sample, &sample));
        assert!(!runtime_binding_changed(&sample, &sample));
    }

    #[test]
    fn host_instance_revision_and_health_changes_are_observable() {
        let before = projection();

        let mut switched = before.clone();
        switched.host = ExecutionHost::Service;
        switched.generation += 1;
        assert!(runtime_materially_changed(&before, &switched));

        let mut replaced = before.clone();
        replaced.snapshot.as_mut().unwrap().instance_id = Some("instance-2".into());
        assert!(runtime_materially_changed(&before, &replaced));

        let mut revised = before.clone();
        revised
            .snapshot
            .as_mut()
            .unwrap()
            .revision
            .as_mut()
            .unwrap()
            .generation += 1;
        assert!(runtime_materially_changed(&before, &revised));

        let mut stopped = before.clone();
        stopped.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Stopped { reason: None });
        assert!(runtime_materially_changed(&before, &stopped));

        let mut unhealthy = before.clone();
        unhealthy.snapshot.as_mut().unwrap().healthy = Some(false);
        assert!(runtime_materially_changed(&before, &unhealthy));
        assert!(
            !runtime_binding_changed(&before, &unhealthy),
            "health-only changes must not repeat OS network mutations"
        );

        let mut degraded = before.clone();
        degraded.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Local,
            reason: "service became unreachable".into(),
        };
        assert!(runtime_materially_changed(&before, &degraded));
        assert!(runtime_binding_changed(&before, &degraded));
        assert!(runtime_binding_changed(&before, &switched));
        assert!(runtime_binding_changed(&before, &replaced));
        assert!(runtime_binding_changed(&before, &revised));
        assert!(runtime_binding_changed(&before, &stopped));
    }
}

//! Tauri adapter for ref-aligned, shared runtime inspection projections.
//! Core actor binding verification remains here; graph projection lives in chimera-core.

use super::{ChimeraClient, runtime::RuntimeSnapshot};
use chimera_ipc::api::{core::v2::CoreEffectiveConfig, status::ConfigRevisionInfo};
#[cfg(test)]
use std::sync::Arc;

pub(crate) use chimera_core::client::runtime_inspection::RuntimeInspectionData;
pub use chimera_core::client::runtime_inspection::{
    RuntimeInspection, RuntimeInspectionContent, RuntimeInspectionDiff, RuntimeInspectionNode,
};

/// Ref-first effective-config evidence: two actor-serialized reads must
/// describe the same endpoint generation, running instance and applied
/// revision. A host that changes away and back cannot reuse an old answer.
fn actor_effective_config_matches_receipt(
    receipt: &crate::client::runtime::RuntimeApplyReceipt,
    before: &chimera_core::actor_v2::router::CoreStatusProjection,
    after: &chimera_core::actor_v2::router::CoreStatusProjection,
    effective: &CoreEffectiveConfig,
) -> bool {
    use chimera_core::actor_v2::router::EndpointConnectivity;
    if !matches!(before.connectivity, EndpointConnectivity::Connected)
        || !matches!(after.connectivity, EndpointConnectivity::Connected)
        || before.host != after.host
        || before.generation != after.generation
        || before.snapshot != after.snapshot
    {
        return false;
    }
    let Some(before_status) = &before.snapshot else {
        return false;
    };
    receipt.confirms_projection(before)
        && receipt.confirms_projection(after)
        && before_status.instance_id.as_deref() == Some(effective.instance_id.as_str())
        && effective.revision == receipt.applied_revision
}

impl ChimeraClient {
    /// The ref CoreActor owns effective-config reads on production clients.
    /// The legacy port is needed only by injected unit-test clients; those
    /// have no CoreActor and must never fabricate a separate routing owner.
    async fn active_effective_inspection_config(
        &self,
        receipt: &crate::client::runtime::RuntimeApplyReceipt,
    ) -> anyhow::Result<Option<CoreEffectiveConfig>> {
        let Some(core) = &self.inner.core_api else {
            return self.inner.core.active_effective_config().await;
        };
        let before = core.refresh_status().await?;
        if !matches!(
            before.connectivity,
            chimera_core::actor_v2::router::EndpointConnectivity::Connected
        ) || !receipt.confirms_projection(&before)
        {
            return Ok(None);
        }
        let Some(effective) = core.effective_config().await? else {
            return Ok(None);
        };
        let after = core.refresh_status().await?;
        Ok(
            actor_effective_config_matches_receipt(receipt, &before, &after, &effective)
                .then_some(effective),
        )
    }

    pub(crate) async fn runtime_exists(&self) -> Vec<String> {
        self.inner
            .core
            .promoted_runtime_snapshot()
            .map(|snapshot| snapshot.exists_keys.clone())
            .unwrap_or_default()
    }

    /// Read-only effective recovery; an older host's reply cannot be attached
    /// to a newer build because its full applied revision must match the receipt.
    async fn inspect_snapshot_view(
        &self,
        snapshot: &RuntimeSnapshot,
    ) -> (RuntimeSnapshot, Option<ConfigRevisionInfo>) {
        let active = self
            .inner
            .core
            .active_runtime_receipt()
            .await
            .ok()
            .flatten();
        let Some(receipt) = active.filter(|receipt| {
            receipt.revision == snapshot.revision
                && receipt.target_core == snapshot.target_core
                && receipt.config_digest
                    == chimera_core_manager::payload_digest(snapshot.product_bytes())
        }) else {
            return (snapshot.clone(), None);
        };
        let revision = receipt.applied_revision.clone();
        let effective = self.active_effective_inspection_config(&receipt).await;
        let Ok(Some(effective)) = effective else {
            if let Err(error) = effective {
                tracing::warn!(%error, "failed to read effective inspection config");
            }
            return (snapshot.clone(), Some(revision));
        };
        if effective.revision != revision {
            return (snapshot.clone(), Some(revision));
        }
        match snapshot.with_effective_config(effective) {
            Ok(view) => (view, Some(revision)),
            Err(error) => {
                tracing::warn!(%error, "failed to decode effective inspection config");
                (snapshot.clone(), Some(revision))
            }
        }
    }

    pub(crate) async fn inspect_runtime(&self) -> Option<RuntimeInspection> {
        let snapshot = self.inner.core.promoted_runtime_snapshot()?;
        let (view, binding) = self.inspect_snapshot_view(&snapshot).await;
        Some(view.inspection_summary(binding))
    }

    /// Return the last confirmed build's inspection even if the core has since
    /// stopped; `applied` is true only while the receipt is still active.
    pub(crate) async fn inspect_applied_runtime(&self) -> Option<RuntimeInspection> {
        let receipt = self.inner.core.last_confirmed_runtime_receipt()?;
        let snapshot = receipt.artifact.as_ref()?;
        let (view, binding) = self.inspect_snapshot_view(snapshot).await;
        Some(view.inspection_summary(binding))
    }

    pub(crate) async fn inspect_runtime_node(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        let snapshots = [
            self.inner.core.promoted_runtime_snapshot(),
            self.inner
                .core
                .last_confirmed_runtime_receipt()
                .and_then(|receipt| receipt.artifact.clone()),
        ];
        for snapshot in snapshots.into_iter().flatten() {
            if snapshot.inspection_id == snapshot_id {
                let snapshot_id = snapshot_id.to_owned();
                return tokio::task::spawn_blocking(move || {
                    snapshot.inspection_content(&snapshot_id, node_id)
                })
                .await?;
            }
            // Only an actively verified effective view can satisfy a derived
            // inspection id; stale or replaced reports are rejected.
            if snapshot_id.starts_with(&format!("{}:effective:", snapshot.inspection_id)) {
                let (view, _) = self.inspect_snapshot_view(&snapshot).await;
                if view.inspection_id == snapshot_id {
                    let snapshot_id = snapshot_id.to_owned();
                    return tokio::task::spawn_blocking(move || {
                        view.inspection_content(&snapshot_id, node_id)
                    })
                    .await?;
                }
            }
        }
        anyhow::bail!("runtime snapshot changed; refresh the inspection")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        profile::ProfileId,
        runtime::{
            executor::{StepLog, StepLogEntry, StepLogLevel},
            snapshot::{BuiltinStepKind, ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag},
            value::ConfigValue,
        },
    };

    use crate::{
        client::runtime::RuntimeRevisionAllocator, config::chimera::ClashCore,
        enhance::PostProcessingOutput,
    };

    fn inspection_data() -> RuntimeInspectionData {
        let graph = ConfigSnapshotsBuilder::new_root(
            Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "rule"})).unwrap()),
            OperatorTag::BareRoot,
        )
        .build_stored()
        .unwrap();
        let key = graph.nodes[0].tag.node_key();
        RuntimeInspectionData {
            final_node: graph.root_id,
            graph,
            step_logs: vec![StepLog {
                key,
                entries: vec![StepLogEntry::new(StepLogLevel::Info, "built")],
            }],
        }
    }

    fn snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot::new_with_transform_output_and_inspection(
            RuntimeRevisionAllocator::default().allocate().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            serde_yaml::Mapping::new(),
            PostProcessingOutput::default(),
            inspection_data(),
        )
    }

    #[test]
    fn actor_effective_inspection_requires_matching_host_generation_and_revision() {
        use chimera_core::actor_v2::{
            endpoint::{CoreStatusSnapshot, ExecutionHost},
            router::{CoreStatusProjection, EndpointConnectivity},
        };
        use chimera_ipc::api::status::{CoreStateDetail, RevisionIdInfo};

        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.controller_generation = Some(7);
        let projection = CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 7,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("running-instance".into()),
                controller: None,
                state: Some(CoreStateDetail::Running {
                    epoch: receipt.applied_revision.epoch,
                    pid: 42,
                }),
                state_changed_at: 12,
                revision: Some(RevisionIdInfo {
                    epoch: receipt.applied_revision.epoch,
                    generation: receipt.applied_revision.generation,
                    effective_hash: receipt.applied_revision.effective_hash.clone(),
                }),
                source_hash: Some(receipt.applied_revision.source_hash.clone()),
                healthy: Some(true),
                applied_kind: None,
            }),
        };
        let effective = CoreEffectiveConfig {
            instance_id: "running-instance".into(),
            revision: receipt.applied_revision.clone(),
            config: "mode: rule\n".into(),
        };
        assert!(actor_effective_config_matches_receipt(
            &receipt,
            &projection,
            &projection,
            &effective
        ));

        let mut after_handoff = projection.clone();
        after_handoff.generation += 1;
        assert!(!actor_effective_config_matches_receipt(
            &receipt,
            &projection,
            &after_handoff,
            &effective
        ));
        let mut after_reconcile = projection.clone();
        after_reconcile.snapshot.as_mut().unwrap().state_changed_at += 1;
        assert!(!actor_effective_config_matches_receipt(
            &receipt,
            &projection,
            &after_reconcile,
            &effective
        ));
        let mut wrong_revision = effective.clone();
        wrong_revision.revision.generation += 1;
        assert!(!actor_effective_config_matches_receipt(
            &receipt,
            &projection,
            &projection,
            &wrong_revision
        ));
        let mut unrelated_instance = effective;
        unrelated_instance.instance_id = "stale-instance".into();
        assert!(!actor_effective_config_matches_receipt(
            &receipt,
            &projection,
            &projection,
            &unrelated_instance
        ));
    }

    #[test]
    fn bare_data_has_a_stable_root() {
        let data = RuntimeInspectionData::bare();
        assert_eq!(data.graph.root_id, 0);
        assert!(matches!(data.graph.nodes[0].tag, OperatorTag::BareRoot));
        assert!(data.step_logs.is_empty());
    }

    #[test]
    fn inspection_projects_metadata_and_selected_content() {
        let snapshot = snapshot();
        let summary = snapshot.inspection_summary(None);
        assert_eq!(summary.root_id, 0);
        assert_eq!(summary.final_id, 0);
        assert!(!summary.applied);
        assert_eq!(summary.revision, "1");
        assert_eq!(summary.nodes.len(), 1);
        assert_eq!(summary.nodes[0].tag, OperatorTag::BareRoot);
        assert_eq!(summary.nodes[0].changed_fields, None);
        assert!(summary.nodes[0].has_logs);
        assert_eq!(summary.nodes[0].max_log_level, Some(StepLogLevel::Info));

        let content = snapshot
            .inspection_content(&summary.snapshot_id, 0)
            .unwrap();
        assert_eq!(
            serde_yaml::from_str::<serde_json::Value>(&content.yaml).unwrap(),
            serde_json::json!({"mode": "rule"})
        );
        assert_eq!(content.logs[0].message, "built");
        assert!(content.diff.is_none());
    }

    #[test]
    fn inspection_reports_the_highest_severity_without_changing_step_logs() {
        let mut snapshot = snapshot();
        Arc::make_mut(&mut snapshot.inspection).step_logs[0]
            .entries
            .extend([
                StepLogEntry::new(StepLogLevel::Log, "trace"),
                StepLogEntry::new(StepLogLevel::Warn, "warning"),
                StepLogEntry::new(StepLogLevel::Error, "failure"),
            ]);
        let summary = snapshot.inspection_summary(Some(ConfigRevisionInfo {
            epoch: 1,
            generation: 1,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        }));
        assert!(!summary.applied);
        assert!(summary.effective_pending);
        assert_eq!(summary.nodes[0].max_log_level, Some(StepLogLevel::Error));
        assert_eq!(
            snapshot
                .inspection_content(&summary.snapshot_id, 0)
                .unwrap()
                .logs
                .len(),
            4
        );
    }

    #[test]
    fn effective_core_view_is_a_separate_redacted_node_and_rejects_stale_revisions() {
        let mut snapshot = snapshot();
        let parent_config = Arc::new(
            ConfigValue::try_from(serde_json::json!({"mode": "rule", "secret": "old"})).unwrap(),
        );
        let graph = ConfigSnapshotsBuilder::new_root(parent_config, OperatorTag::BareRoot)
            .build_stored()
            .unwrap();
        snapshot.inspection = Arc::new(RuntimeInspectionData {
            final_node: graph.root_id,
            graph,
            step_logs: Vec::new(),
        });
        let effective = CoreEffectiveConfig {
            instance_id: "instance-1".into(),
            revision: ConfigRevisionInfo {
                epoch: 12,
                generation: 7,
                source_hash: "source".into(),
                effective_hash: "effective-v7".into(),
            },
            config: "mode: global\nmmdb: Country.mmdb\nsecret: private\n".into(),
        };
        let verified = snapshot.with_effective_config(effective.clone()).unwrap();
        let again = snapshot.with_effective_config(effective.clone()).unwrap();
        assert_eq!(verified.inspection_id, again.inspection_id);
        assert_ne!(verified.inspection_id, snapshot.inspection_id);

        let summary = verified.inspection_summary(Some(effective.revision.clone()));
        assert!(summary.applied);
        assert!(!summary.effective_pending);
        assert_eq!(summary.effective_revision, Some(effective.revision.clone()));
        assert_eq!(summary.final_id, 0);
        assert_eq!(summary.nodes.len(), 2);
        assert_eq!(summary.nodes[0].next, [1]);
        assert_eq!(summary.nodes[1].max_log_level, None);
        assert!(matches!(
            summary.nodes[1].tag,
            OperatorTag::BuiltinStep {
                step: BuiltinStepKind::CoreController,
                ..
            }
        ));
        assert!(
            summary.nodes[1]
                .changed_fields
                .as_ref()
                .unwrap()
                .contains(&"mmdb".to_string())
        );
        assert!(
            summary.nodes[1]
                .changed_fields
                .as_ref()
                .unwrap()
                .contains(&"secret".to_string())
        );

        let content = verified
            .inspection_content(&summary.snapshot_id, 1)
            .unwrap();
        let parsed: serde_json::Value = serde_yaml::from_str(&content.yaml).unwrap();
        assert_eq!(parsed["mode"], "global");
        assert_eq!(parsed["mmdb"], "Country.mmdb");
        assert_eq!(parsed["secret"], "<redacted>");
        assert!(!content.yaml.contains("private"));
        assert!(!content.diff.as_ref().unwrap().hunks.is_empty());
        assert!(
            content
                .diff
                .as_ref()
                .unwrap()
                .hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .all(|line| !line.contains("private"))
        );

        let mut changed = effective;
        changed.revision.generation += 1;
        changed.revision.effective_hash = "effective-v8".into();
        let newer = snapshot.with_effective_config(changed).unwrap();
        assert_ne!(newer.inspection_id, verified.inspection_id);
        assert!(
            newer
                .inspection_content(&verified.inspection_id, 1)
                .is_err()
        );
        assert!(
            snapshot
                .inspection_content(&summary.snapshot_id, 1)
                .is_err()
        );
    }

    #[test]
    fn inspection_preserves_branch_links_and_change_baselines() {
        let value =
            |mode| Arc::new(ConfigValue::try_from(serde_json::json!({"mode": mode})).unwrap());
        let mut builder = ConfigSnapshotsBuilder::new_root(value("rule"), OperatorTag::BareRoot);
        builder
            .attach_independent_branch(
                builder.root_node_id(),
                ConfigSnapshotsBuilder::new_root(
                    value("direct"),
                    OperatorTag::FileConfigRoot {
                        profile_id: ProfileId("source".into()),
                        role: ConfigExecutionRole::CompositionContributor {
                            composition_id: ProfileId("combined".into()),
                            contributor_index: 0,
                        },
                    },
                ),
            )
            .unwrap();
        builder
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                value("global"),
            )
            .unwrap();

        let mut snapshot = snapshot();
        let (graph, final_node) = builder.build_stored_with_current().unwrap();
        snapshot.inspection = Arc::new(RuntimeInspectionData {
            graph,
            final_node,
            step_logs: Vec::new(),
        });
        let summary = snapshot.inspection_summary(None);
        let root = &summary.nodes[summary.root_id as usize];
        assert_eq!(summary.final_id, final_node);
        assert_eq!(
            summary.nodes[summary.final_id as usize].tag,
            OperatorTag::BuiltinStep {
                selected_profile_id: None,
                step: BuiltinStepKind::Finalizing,
            }
        );
        assert_eq!(root.next.len(), 2);
        for child in &root.next {
            let node = &summary.nodes[*child as usize];
            let content = snapshot
                .inspection_content(&summary.snapshot_id, *child)
                .unwrap();
            let config: serde_json::Value = serde_yaml::from_str(&content.yaml).unwrap();
            assert!(content.logs.is_empty());
            assert!(!node.has_logs);
            match &node.tag {
                OperatorTag::FileConfigRoot { .. } => {
                    assert_eq!(node.changed_fields, None);
                    assert_eq!(config["mode"], "direct");
                    assert!(content.diff.is_none());
                }
                OperatorTag::BuiltinStep { .. } => {
                    assert_eq!(node.changed_fields, Some(vec!["mode".to_string()]));
                    assert_eq!(config["mode"], "global");
                    let diff = content.diff.unwrap();
                    assert_eq!(diff.parent_id, summary.root_id);
                    assert_eq!(diff.hunks[0].lines, ["-mode: rule", "+mode: global"]);
                }
                _ => panic!("unexpected child"),
            }
        }
    }

    #[test]
    fn inspection_rejects_replaced_snapshot_and_missing_node() {
        let first = snapshot();
        let second = snapshot();
        assert!(second.inspection_content(&first.inspection_id, 0).is_err());
        assert!(
            first
                .inspection_content(&first.inspection_id, u32::MAX)
                .is_err()
        );
    }
}

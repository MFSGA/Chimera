//! Read-only projections of one immutable, promoted runtime build.

use super::{core_lifecycle::ports::CoreLifecyclePort, runtime::RuntimeSnapshot};
use crate::actor_v2::CoreClient;
use std::sync::Arc;

use chimera_config::{
    redact::redact_config,
    runtime::{
        executor::{StepLog, StepLogEntry, StepLogLevel},
        snapshot::{
            BuiltinStepKind, ConfigSnapshot, ConfigSnapshotsBuilder, OperatorTag, SnapshotDiffHunk,
            StoredConfigSnapshotsGraph, changed_fields_between,
        },
        value::ConfigValue,
    },
};
use chimera_ipc::api::{core::v2::CoreEffectiveConfig, status::ConfigRevisionInfo};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct RuntimeInspectionData {
    pub graph: StoredConfigSnapshotsGraph,
    /// The node that owns the built YAML, not an arbitrary terminal branch.
    pub final_node: u32,
    pub step_logs: Vec<StepLog>,
}

impl RuntimeInspectionData {
    /// Empty root used by explicit compatibility constructors and tests that
    /// do not carry a RuntimeExecutor graph.
    pub fn bare() -> Self {
        let config = ConfigValue::try_from(serde_json::json!({}))
            .expect("empty JSON object is a valid runtime config value");
        let graph = ConfigSnapshotsBuilder::new_root(Arc::new(config), OperatorTag::BareRoot)
            .build_stored()
            .expect("bare runtime inspection graph must build");
        Self {
            final_node: graph.root_id,
            graph,
            step_logs: Vec::new(),
        }
    }
}

/// The core's actual reported configuration, attached as a virtual child of
/// the final build node without changing the immutable pipeline graph.
#[derive(Debug, Clone)]
pub struct EffectiveInspection {
    pub config: serde_json::Value,
    pub revision: ConfigRevisionInfo,
    pub parent: u32,
    pub tag: OperatorTag,
    pub changed_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspection {
    pub snapshot_id: String,
    /// Whether the selected build matches the currently active core receipt.
    pub applied: bool,
    /// An active matching receipt exists but the host cannot yet supply its document.
    pub effective_pending: bool,
    pub effective_revision: Option<ConfigRevisionInfo>,
    pub revision: String,
    pub target_core: String,
    pub root_id: u32,
    pub final_id: u32,
    pub nodes: Vec<RuntimeInspectionNode>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionNode {
    pub id: u32,
    pub tag: chimera_config::runtime::snapshot::OperatorTag,
    pub next: Vec<u32>,
    pub has_logs: bool,
    /// Highest severity of the recorded step logs, matching the upstream DTO.
    pub max_log_level: Option<StepLogLevel>,
    /// None means unchanged or no comparison baseline (including independent roots).
    pub changed_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionContent {
    pub yaml: String,
    pub diff: Option<RuntimeInspectionDiff>,
    pub logs: Vec<StepLogEntry>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionDiff {
    pub parent_id: u32,
    pub hunks: Vec<SnapshotDiffHunk>,
}

impl RuntimeInspectionData {
    /// Build the core-reported view against the immutable promoted graph.
    /// The caller authenticates host, generation, and applied revision before
    /// invoking this pure projection; no I/O or actor state lives here.
    pub fn effective_config(
        &self,
        effective: CoreEffectiveConfig,
    ) -> anyhow::Result<EffectiveInspection> {
        let yaml: serde_yaml::Value = serde_yaml::from_str(&effective.config)?;
        let config = serde_json::to_value(yaml)?;
        let parent = self.final_node;
        let parent_node = self
            .graph
            .nodes
            .get(parent as usize)
            .ok_or_else(|| anyhow::anyhow!("runtime final node is missing"))?;
        let selected_profile_id = match &parent_node.tag {
            OperatorTag::BuiltinStep {
                selected_profile_id,
                ..
            }
            | OperatorTag::BuiltinTransform {
                selected_profile_id,
                ..
            } => selected_profile_id.clone(),
            _ => None,
        };
        // Compare unredacted values so paths changed only under a secret are
        // retained, while the final API response always redacts values.
        let parent_config = self.graph.restore(parent)?;
        let changed_fields = changed_fields_between(&parent_config, &config)
            .map(|fields| fields.into_iter().collect());
        Ok(EffectiveInspection {
            config,
            revision: effective.revision,
            parent,
            tag: OperatorTag::BuiltinStep {
                selected_profile_id,
                step: BuiltinStepKind::CoreController,
            },
            changed_fields,
        })
    }

    fn max_log_level(&self, tag: &OperatorTag) -> Option<StepLogLevel> {
        let key = tag.node_key();
        self.step_logs
            .iter()
            .filter(|log| log.key == key)
            .flat_map(|log| &log.entries)
            .map(|entry| entry.level)
            .max()
    }

    pub fn inspection_summary(
        &self,
        inspection_id: &str,
        revision: &str,
        target_core: &str,
        effective: Option<&EffectiveInspection>,
        bound_revision: Option<ConfigRevisionInfo>,
    ) -> RuntimeInspection {
        let mut nodes: Vec<_> = self
            .graph
            .nodes
            .iter()
            .enumerate()
            .map(|(id, node)| {
                let max_log_level = self.max_log_level(&node.tag);
                RuntimeInspectionNode {
                    id: id as u32,
                    has_logs: max_log_level.is_some(),
                    max_log_level,
                    tag: node.tag.clone(),
                    next: node.next.clone().unwrap_or_default(),
                    changed_fields: node
                        .changed_fields
                        .as_ref()
                        .map(|fields| fields.iter().cloned().collect()),
                }
            })
            .collect();
        if let Some(effective) = effective {
            let id = nodes.len() as u32;
            if let Some(final_node) = nodes.get_mut(effective.parent as usize) {
                final_node.next.push(id);
            }
            nodes.push(RuntimeInspectionNode {
                id,
                tag: effective.tag.clone(),
                next: Vec::new(),
                has_logs: false,
                max_log_level: None,
                changed_fields: effective.changed_fields.clone(),
            });
        }
        RuntimeInspection {
            snapshot_id: inspection_id.to_owned(),
            applied: bound_revision.is_some() && effective.is_some(),
            effective_pending: bound_revision.is_some() && effective.is_none(),
            effective_revision: effective
                .map(|effective| effective.revision.clone())
                .or(bound_revision),
            revision: revision.to_owned(),
            target_core: target_core.to_owned(),
            root_id: self.graph.root_id,
            final_id: self.final_node,
            nodes,
        }
    }

    pub fn inspection_content(
        &self,
        inspection_id: &str,
        effective: Option<&EffectiveInspection>,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        anyhow::ensure!(
            inspection_id == snapshot_id,
            chimera_contracts::runtime_error::RuntimeError::RuntimeSnapshotChanged
        );
        let (tag, config, parent) = match effective {
            Some(effective) if node_id as usize == self.graph.nodes.len() => (
                &effective.tag,
                effective.config.clone(),
                Some((effective.parent, self.graph.restore(effective.parent)?)),
            ),
            _ => {
                let node = self.graph.nodes.get(node_id as usize).ok_or(
                    chimera_contracts::runtime_error::RuntimeError::RuntimeNodeNotFound { node_id },
                )?;
                let (config, parent) = self.graph.restore_with_parent(node_id)?;
                (&node.tag, config, parent)
            }
        };
        // Match ref: restore the selected node only when requested. Redact
        // both sides before producing YAML and a human-readable diff.
        let yaml = serde_yaml::to_string(&redact_config(config))?;
        let key = tag.node_key();
        Ok(RuntimeInspectionContent {
            diff: parent
                .map(|(parent_id, parent)| -> anyhow::Result<_> {
                    Ok(RuntimeInspectionDiff {
                        parent_id,
                        hunks: ConfigSnapshot::new_unchanged(redact_config(parent))
                            .diff_yaml_to(&yaml)?,
                    })
                })
                .transpose()?,
            yaml,
            logs: self
                .step_logs
                .iter()
                .filter(|log| log.key == key)
                .flat_map(|log| log.entries.iter().cloned())
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_graph_reports_one_root_and_rejects_stale_snapshot_ids() {
        let data = RuntimeInspectionData::bare();
        let summary = data.inspection_summary("snapshot-1", "12", "mihomo", None, None);
        assert_eq!(summary.snapshot_id, "snapshot-1");
        assert_eq!(summary.root_id, summary.final_id);
        assert_eq!(summary.nodes.len(), 1);
        assert!(!summary.applied);
        assert!(!summary.effective_pending);
        assert_eq!(summary.nodes[0].tag, OperatorTag::BareRoot);

        let node = data
            .inspection_content("snapshot-1", None, "snapshot-1", 0)
            .unwrap();
        assert!(node.logs.is_empty());
        assert!(node.diff.is_none());
        let replaced = data
            .inspection_content("snapshot-1", None, "replaced", 0)
            .unwrap_err();
        assert!(matches!(
            replaced.downcast_ref::<chimera_contracts::runtime_error::RuntimeError>(),
            Some(chimera_contracts::runtime_error::RuntimeError::RuntimeSnapshotChanged)
        ));
        let missing = data
            .inspection_content("snapshot-1", None, "snapshot-1", 42)
            .unwrap_err();
        assert!(matches!(
            missing.downcast_ref::<chimera_contracts::runtime_error::RuntimeError>(),
            Some(
                chimera_contracts::runtime_error::RuntimeError::RuntimeNodeNotFound { node_id: 42 }
            )
        ));
    }

    /// Source configs are independent of the inspector. This checks that a
    /// stored delta is replayed only on request, that the summary reports its
    /// unredacted changed paths, and that both YAML and diff redact secrets.
    #[test]
    fn stored_delta_inspection_replays_and_redacts_config_and_parent_diff() {
        use chimera_config::runtime::snapshot::{KeyframePolicy, SnapshotPayload};

        let input = serde_json::json!({
            "mode": "rule",
            "secret": "old-secret",
            "proxies": ["a", "b"],
            "rules": ["MATCH,DIRECT"],
        });
        let expected = serde_json::json!({
            "mode": "global",
            "secret": "new-secret",
            "proxies": ["a", "b"],
            "rules": ["MATCH,DIRECT"],
        });
        let mut builder = ConfigSnapshotsBuilder::new_root_with_keyframe_policy(
            Arc::new(ConfigValue::try_from(input).unwrap()),
            OperatorTag::BareRoot,
            KeyframePolicy {
                delta_to_full_ratio: 10.0,
            },
        );
        let tag = OperatorTag::BuiltinStep {
            selected_profile_id: None,
            step: BuiltinStepKind::Finalizing,
        };
        builder
            .push(
                tag.clone(),
                Arc::new(ConfigValue::try_from(expected).unwrap()),
            )
            .unwrap();
        let (graph, final_node) = builder.build_stored_with_current().unwrap();
        assert!(matches!(
            graph.nodes[final_node as usize].snapshot.payload,
            SnapshotPayload::Delta(_)
        ));

        let data = RuntimeInspectionData {
            graph,
            final_node,
            step_logs: vec![StepLog {
                key: tag.node_key(),
                entries: vec![StepLogEntry::new(StepLogLevel::Info, "generated")],
            }],
        };
        let summary = data.inspection_summary("version-3", "3", "mihomo", None, None);
        assert_eq!(summary.final_id, final_node);
        let changed = summary.nodes[final_node as usize]
            .changed_fields
            .as_ref()
            .unwrap();
        assert!(changed.contains(&"secret".to_owned()));
        assert!(changed.contains(&"mode".to_owned()));

        let content = data
            .inspection_content("version-3", None, "version-3", final_node)
            .unwrap();
        let parsed: serde_json::Value = serde_yaml::from_str(&content.yaml).unwrap();
        assert_eq!(parsed["mode"], "global");
        assert_eq!(parsed["secret"], "<redacted>");
        assert_eq!(content.logs[0].message, "generated");
        assert_eq!(content.diff.as_ref().unwrap().parent_id, 0);
        assert!(!content.diff.as_ref().unwrap().hunks.is_empty());
        assert!(!format!("{content:?}").contains("old-secret"));
        assert!(!format!("{content:?}").contains("new-secret"));
    }

    /// A stored graph may be corrupted by a persistence or replay failure.
    /// The inspection API must return an error instead of partial YAML.
    #[test]
    fn invalid_stored_delta_is_not_exposed_as_partial_inspection_yaml() {
        let root = Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "rule"})).unwrap());
        let mut builder = ConfigSnapshotsBuilder::new_root(root.clone(), OperatorTag::BareRoot);
        let invalid_patch = serde_json::from_value(serde_json::json!([
            {"op": "replace", "path": "/mode", "value": "global"},
            {"op": "remove", "path": "/missing"}
        ]))
        .unwrap();
        builder
            .push_delta(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                root,
                invalid_patch,
            )
            .unwrap();
        let (graph, final_node) = builder.build_stored_with_current().unwrap();
        let data = RuntimeInspectionData {
            graph,
            final_node,
            step_logs: Vec::new(),
        };
        assert!(
            data.inspection_content("version-3", None, "version-3", final_node)
                .is_err()
        );
    }

    #[test]
    fn effective_view_keeps_sensitive_changed_paths_but_redacts_values() {
        let root = Arc::new(
            ConfigValue::try_from(serde_json::json!({"mode": "rule", "secret": "old"})).unwrap(),
        );
        let graph = ConfigSnapshotsBuilder::new_root(root, OperatorTag::BareRoot)
            .build_stored()
            .unwrap();
        let data = RuntimeInspectionData {
            final_node: graph.root_id,
            graph,
            step_logs: Vec::new(),
        };
        let effective = data
            .effective_config(CoreEffectiveConfig {
                instance_id: "core-a".into(),
                revision: ConfigRevisionInfo {
                    epoch: 7,
                    generation: 2,
                    source_hash: "source".into(),
                    effective_hash: "effective".into(),
                },
                config: "mode: direct\nsecret: private\n".into(),
            })
            .unwrap();
        let summary = data.inspection_summary(
            "effective-id",
            "12",
            "mihomo",
            Some(&effective),
            Some(effective.revision.clone()),
        );
        assert!(summary.applied);
        assert_eq!(summary.final_id, 0);
        assert_eq!(summary.nodes[0].next, [1]);
        assert_eq!(summary.nodes[1].changed_fields.as_ref().unwrap().len(), 2);

        let content = data
            .inspection_content("effective-id", Some(&effective), "effective-id", 1)
            .unwrap();
        let json: serde_json::Value = serde_yaml::from_str(&content.yaml).unwrap();
        assert_eq!(json["mode"], "direct");
        assert_eq!(json["secret"], "<redacted>");
        assert!(!content.yaml.contains("private"));
        assert!(
            content
                .diff
                .unwrap()
                .hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .all(|line| !line.contains("private") && !line.contains("old"))
        );
    }
}

impl RuntimeSnapshot {
    /// Restore the graph's final node, retaining the executor's JSON semantics.
    pub fn config_json(&self) -> anyhow::Result<serde_json::Value> {
        Ok(self.inspection.graph.restore(self.inspection.final_node)?)
    }

    /// Attach an authenticated effective document to a temporary inspection view.
    pub fn with_effective_config(&self, effective: CoreEffectiveConfig) -> anyhow::Result<Self> {
        let view = self.inspection.effective_config(effective)?;
        let mut snapshot = self.clone();
        snapshot.inspection_id = format!(
            "{}:effective:{}:{}:{}",
            self.inspection_id,
            view.revision.epoch,
            view.revision.generation,
            view.revision.effective_hash,
        );
        snapshot.effective = Some(Arc::new(view));
        Ok(snapshot)
    }

    pub fn inspection_summary(
        &self,
        bound_revision: Option<ConfigRevisionInfo>,
    ) -> RuntimeInspection {
        self.inspection.inspection_summary(
            &self.inspection_id,
            &self.revision.get().to_string(),
            &self.target_core.to_string(),
            self.effective.as_deref(),
            bound_revision,
        )
    }

    pub fn inspection_content(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        self.inspection.inspection_content(
            &self.inspection_id,
            self.effective.as_deref(),
            snapshot_id,
            node_id,
        )
    }
}

/// Ref's read-only client inspection workflow, using the existing CoreActor
/// and lifecycle receipt owner. No desktop state or second snapshot store.
pub struct RuntimeInspectionReader<'a> {
    core: &'a dyn CoreLifecyclePort,
    core_api: Option<&'a CoreClient>,
}

impl<'a> RuntimeInspectionReader<'a> {
    pub fn new(core: &'a dyn CoreLifecyclePort, core_api: Option<&'a CoreClient>) -> Self {
        Self { core, core_api }
    }
}

/// Ref-first effective-config evidence: two actor-serialized reads must
/// describe the same endpoint generation, running instance and applied
/// revision. A host that changes away and back cannot reuse an old answer.
fn actor_effective_config_matches_receipt(
    receipt: &crate::client::runtime::RuntimeApplyReceipt,
    before: &crate::actor_v2::router::CoreStatusProjection,
    after: &crate::actor_v2::router::CoreStatusProjection,
    effective: &CoreEffectiveConfig,
) -> bool {
    use crate::actor_v2::router::EndpointConnectivity;
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

impl RuntimeInspectionReader<'_> {
    /// The ref CoreActor owns effective-config reads on production clients.
    /// The legacy port is needed only by injected unit-test clients; those
    /// have no CoreActor and must never fabricate a separate routing owner.
    async fn active_effective_inspection_config(
        &self,
        receipt: &crate::client::runtime::RuntimeApplyReceipt,
    ) -> anyhow::Result<Option<CoreEffectiveConfig>> {
        let Some(core) = self.core_api else {
            return self.core.active_effective_config().await;
        };
        let before = core.refresh_status().await?;
        if !matches!(
            before.connectivity,
            crate::actor_v2::router::EndpointConnectivity::Connected
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

    pub async fn runtime_exists(&self) -> Vec<String> {
        self.core
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
        let active = self.core.active_runtime_receipt().await.ok().flatten();
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

    pub async fn inspect_runtime(&self) -> Option<RuntimeInspection> {
        let snapshot = self.core.promoted_runtime_snapshot()?;
        let (view, binding) = self.inspect_snapshot_view(&snapshot).await;
        Some(view.inspection_summary(binding))
    }

    /// Return the last confirmed build's inspection even if the core has since
    /// stopped; `applied` is true only while the receipt is still active.
    pub async fn inspect_applied_runtime(&self) -> Option<RuntimeInspection> {
        let receipt = self.core.last_confirmed_runtime_receipt()?;
        let snapshot = receipt.artifact.as_ref()?;
        let (view, binding) = self.inspect_snapshot_view(snapshot).await;
        Some(view.inspection_summary(binding))
    }

    pub async fn inspect_runtime_node(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        let snapshots = [
            self.core.promoted_runtime_snapshot(),
            self.core
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
        Err(chimera_contracts::runtime_error::RuntimeError::RuntimeSnapshotChanged.into())
    }
}

#[cfg(test)]
mod reader_tests {
    use super::*;
    use chimera_config::application::ClashCore;
    use chimera_config::{
        profile::ProfileId,
        runtime::{
            executor::{StepLog, StepLogEntry, StepLogLevel},
            snapshot::{BuiltinStepKind, ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag},
            value::ConfigValue,
        },
    };
    use chimera_profile_transform::PostProcessingOutput;

    use crate::client::runtime::RuntimeRevisionAllocator;

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
        use crate::actor_v2::{
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

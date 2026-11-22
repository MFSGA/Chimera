//! Read-only projections of one immutable, promoted runtime build.

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
            "runtime snapshot changed; refresh the inspection"
        );
        let (tag, config, parent) = match effective {
            Some(effective) if node_id as usize == self.graph.nodes.len() => (
                &effective.tag,
                effective.config.clone(),
                Some((effective.parent, self.graph.restore(effective.parent)?)),
            ),
            _ => {
                let node = self
                    .graph
                    .nodes
                    .get(node_id as usize)
                    .ok_or_else(|| anyhow::anyhow!("runtime snapshot node does not exist"))?;
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
        assert!(
            data.inspection_content("snapshot-1", None, "replaced", 0)
                .is_err()
        );
        assert!(
            data.inspection_content("snapshot-1", None, "snapshot-1", 42)
                .is_err()
        );
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

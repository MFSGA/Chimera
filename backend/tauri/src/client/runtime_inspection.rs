//! Read-only projections of one immutable, promoted runtime build.

use std::sync::Arc;

use chimera_config::runtime::{
    executor::{StepLog, StepLogEntry},
    snapshot::{ConfigSnapshotsBuilder, ConfigSnapshotsGraph, OperatorTag, SnapshotDiffHunk},
    value::ConfigValue,
};
use serde::Serialize;

use super::{ChimeraClient, runtime::RuntimeSnapshot};

#[derive(Debug, Clone)]
pub(crate) struct RuntimeInspectionData {
    pub graph: ConfigSnapshotsGraph,
    pub step_logs: Vec<StepLog>,
}

impl RuntimeInspectionData {
    /// Compatibility root used by legacy/fallback builds that do not expose
    /// the ref executor graph yet.
    pub(crate) fn bare() -> Self {
        let config = ConfigValue::try_from(serde_json::json!({}))
            .expect("empty JSON object is a valid runtime config value");
        let graph = ConfigSnapshotsBuilder::new_root(Arc::new(config), OperatorTag::BareRoot)
            .build()
            .expect("bare runtime inspection graph must build");
        Self {
            graph,
            step_logs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspection {
    pub snapshot_id: String,
    pub revision: String,
    pub target_core: String,
    pub root_id: u32,
    pub nodes: Vec<RuntimeInspectionNode>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeInspectionNode {
    pub id: u32,
    pub tag: chimera_config::runtime::snapshot::OperatorTag,
    pub next: Vec<u32>,
    pub has_logs: bool,
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

impl RuntimeSnapshot {
    fn inspection_summary(&self) -> RuntimeInspection {
        RuntimeInspection {
            snapshot_id: self.inspection_id.clone(),
            revision: self.revision.get().to_string(),
            target_core: self.target_core.to_string(),
            root_id: self.inspection.graph.root_id,
            nodes: self
                .inspection
                .graph
                .nodes
                .iter()
                .enumerate()
                .map(|(id, node)| RuntimeInspectionNode {
                    id: id as u32,
                    has_logs: self
                        .inspection
                        .step_logs
                        .iter()
                        .any(|log| log.key == node.key && !log.entries.is_empty()),
                    tag: node.tag.clone(),
                    next: node.next.clone().unwrap_or_default(),
                    changed_fields: node
                        .snapshot
                        .changed_fields
                        .as_ref()
                        .map(|fields| fields.iter().cloned().collect()),
                })
                .collect(),
        }
    }

    fn inspection_content(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        anyhow::ensure!(
            self.inspection_id == snapshot_id,
            "runtime snapshot changed; refresh the inspection"
        );
        let node = self
            .inspection
            .graph
            .nodes
            .get(node_id as usize)
            .ok_or_else(|| anyhow::anyhow!("runtime snapshot node does not exist"))?;
        let yaml = serde_yaml::to_string(&node.snapshot.config)?;
        Ok(RuntimeInspectionContent {
            diff: self
                .inspection
                .graph
                .comparison_parent(node_id)
                .map(|parent_id| -> anyhow::Result<_> {
                    Ok(RuntimeInspectionDiff {
                        parent_id,
                        hunks: self.inspection.graph.nodes[parent_id as usize]
                            .snapshot
                            .diff_yaml_to(&yaml)?,
                    })
                })
                .transpose()?,
            yaml,
            logs: self
                .inspection
                .step_logs
                .iter()
                .filter(|log| log.key == node.key)
                .flat_map(|log| log.entries.iter().cloned())
                .collect(),
        })
    }
}

impl ChimeraClient {
    pub(crate) async fn runtime_exists(&self) -> Vec<String> {
        self.inner
            .core
            .promoted_runtime_snapshot()
            .map(|snapshot| snapshot.exists_keys.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn inspect_runtime(&self) -> Option<RuntimeInspection> {
        self.inner
            .core
            .promoted_runtime_snapshot()
            .map(|snapshot| snapshot.inspection_summary())
    }

    pub(crate) async fn inspect_runtime_node(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        let snapshot = self.inner.core.promoted_runtime_snapshot().ok_or_else(|| {
            anyhow::anyhow!("no promoted runtime snapshot; refresh the inspection")
        })?;
        let snapshot_id = snapshot_id.to_owned();
        tokio::task::spawn_blocking(move || snapshot.inspection_content(&snapshot_id, node_id))
            .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        profile::ProfileId,
        runtime::{
            executor::StepLogLevel,
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
        .build()
        .unwrap();
        let key = graph.nodes[0].key.clone();
        RuntimeInspectionData {
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
    fn bare_data_has_a_stable_root() {
        let data = RuntimeInspectionData::bare();
        assert_eq!(data.graph.root_id, 0);
        assert!(matches!(data.graph.nodes[0].tag, OperatorTag::BareRoot));
        assert!(data.step_logs.is_empty());
    }

    #[test]
    fn inspection_projects_metadata_and_selected_content() {
        let snapshot = snapshot();
        let summary = snapshot.inspection_summary();
        assert_eq!(summary.root_id, 0);
        assert_eq!(summary.revision, "1");
        assert_eq!(summary.nodes.len(), 1);
        assert_eq!(summary.nodes[0].tag, OperatorTag::BareRoot);
        assert_eq!(summary.nodes[0].changed_fields, None);
        assert!(summary.nodes[0].has_logs);

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
        snapshot.inspection = Arc::new(RuntimeInspectionData {
            graph: builder.build().unwrap(),
            step_logs: Vec::new(),
        });
        let summary = snapshot.inspection_summary();
        let root = &summary.nodes[summary.root_id as usize];
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

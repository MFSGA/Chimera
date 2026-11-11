//! Read-only projections of one immutable, promoted runtime build.

use std::sync::Arc;

use chimera_config::{
    redact::redact_config,
    runtime::{
        executor::{StepLog, StepLogEntry, StepLogLevel},
        snapshot::{
            BuiltinStepKind, ConfigSnapshot, ConfigSnapshotsBuilder, ConfigSnapshotsGraph,
            OperatorTag, SnapshotDiffHunk, changed_fields_between,
        },
        value::ConfigValue,
    },
};
use chimera_ipc::api::{core::v2::CoreEffectiveConfig, status::ConfigRevisionInfo};
use serde::Serialize;

use super::{ChimeraClient, runtime::RuntimeSnapshot};

#[derive(Debug, Clone)]
pub(crate) struct RuntimeInspectionData {
    pub graph: ConfigSnapshotsGraph,
    /// The node that owns the built YAML, not an arbitrary terminal branch.
    pub final_node: u32,
    pub step_logs: Vec<StepLog>,
}

impl RuntimeInspectionData {
    /// Empty root used by explicit compatibility constructors and tests that
    /// do not carry a RuntimeExecutor graph.
    pub(crate) fn bare() -> Self {
        let config = ConfigValue::try_from(serde_json::json!({}))
            .expect("empty JSON object is a valid runtime config value");
        let graph = ConfigSnapshotsBuilder::new_root(Arc::new(config), OperatorTag::BareRoot)
            .build()
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
pub(crate) struct EffectiveInspection {
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

impl RuntimeSnapshot {
    /// Attach a verified, core-reported view without mutating the committed build.
    /// The derived identifier is stable across refreshes for the same effective
    /// revision and changes when the core reports a different document.
    fn with_effective_config(&self, effective: CoreEffectiveConfig) -> anyhow::Result<Self> {
        let yaml: serde_yaml::Value = serde_yaml::from_str(&effective.config)?;
        let config = serde_json::to_value(yaml)?;
        let parent = self.inspection.final_node;
        let parent_node = self
            .inspection
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
        // Compare raw values to retain changed field paths even when both
        // secret values redact to the same placeholder. Only paths escape.
        let changed_fields = changed_fields_between(&parent_node.snapshot.config, &config)
            .map(|fields| fields.into_iter().collect());
        let mut snapshot = self.clone();
        snapshot.inspection_id = format!(
            "{}:effective:{}:{}:{}",
            self.inspection_id,
            effective.revision.epoch,
            effective.revision.generation,
            effective.revision.effective_hash,
        );
        snapshot.effective = Some(Arc::new(EffectiveInspection {
            config,
            revision: effective.revision,
            parent,
            tag: OperatorTag::BuiltinStep {
                selected_profile_id,
                step: BuiltinStepKind::CoreController,
            },
            changed_fields,
        }));
        Ok(snapshot)
    }

    fn max_log_level(&self, tag: &OperatorTag) -> Option<StepLogLevel> {
        let key = tag.node_key();
        self.inspection
            .step_logs
            .iter()
            .filter(|log| log.key == key)
            .flat_map(|log| &log.entries)
            .map(|entry| entry.level)
            .max()
    }

    fn inspection_summary(&self, bound_revision: Option<ConfigRevisionInfo>) -> RuntimeInspection {
        let mut nodes: Vec<_> = self
            .inspection
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
                        .snapshot
                        .changed_fields
                        .as_ref()
                        .map(|fields| fields.iter().cloned().collect()),
                }
            })
            .collect();
        if let Some(effective) = &self.effective {
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
            snapshot_id: self.inspection_id.clone(),
            applied: bound_revision.is_some() && self.effective.is_some(),
            effective_pending: bound_revision.is_some() && self.effective.is_none(),
            effective_revision: self
                .effective
                .as_ref()
                .map(|effective| effective.revision.clone())
                .or(bound_revision),
            revision: self.revision.get().to_string(),
            target_core: self.target_core.to_string(),
            root_id: self.inspection.graph.root_id,
            final_id: self.inspection.final_node,
            nodes,
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
        if let Some(effective) = self
            .effective
            .as_ref()
            .filter(|_| node_id as usize == self.inspection.graph.nodes.len())
        {
            let yaml = serde_yaml::to_string(&redact_config(effective.config.clone()))?;
            let parent = &self.inspection.graph.nodes[effective.parent as usize];
            return Ok(RuntimeInspectionContent {
                diff: Some(RuntimeInspectionDiff {
                    parent_id: effective.parent,
                    hunks: ConfigSnapshot::new_unchanged(redact_config(
                        parent.snapshot.config.clone(),
                    ))
                    .diff_yaml_to(&yaml)?,
                }),
                yaml,
                logs: Vec::new(),
            });
        }
        let node = self
            .inspection
            .graph
            .nodes
            .get(node_id as usize)
            .ok_or_else(|| anyhow::anyhow!("runtime snapshot node does not exist"))?;
        let yaml = serde_yaml::to_string(&redact_config(node.snapshot.config.clone()))?;
        Ok(RuntimeInspectionContent {
            diff: self
                .inspection
                .graph
                .comparison_parent(node_id)
                .map(|parent_id| -> anyhow::Result<_> {
                    Ok(RuntimeInspectionDiff {
                        parent_id,
                        hunks: ConfigSnapshot::new_unchanged(redact_config(
                            self.inspection.graph.nodes[parent_id as usize]
                                .snapshot
                                .config
                                .clone(),
                        ))
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
        let effective = self.inner.core.active_effective_config().await;
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
            .build()
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
        let graph = builder.build().unwrap();
        let final_node = graph.nodes.len() as u32 - 1;
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

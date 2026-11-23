use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;

/// User-defined presentation folder for proxy groups; members are Clash group
/// names, not Profile ids or runtime-config mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyGroupFolder {
    pub id: FolderId,
    pub name: String,
    pub members: Vec<String>,
}

/// A Config Profile's proxy-group arrangement. A layout can mention groups
/// absent from the current core snapshot: reconciliation preserves those names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyGroupLayout {
    pub folders: Vec<ProxyGroupFolder>,
    pub ungrouped: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedProxyGroupLayout {
    pub folders: Vec<ProxyGroupFolder>,
    pub ungrouped: Vec<String>,
}

impl ResolvedProxyGroupLayout {
    pub fn flat_order(&self) -> Vec<String> {
        self.ungrouped
            .iter()
            .chain(self.folders.iter().flat_map(|folder| &folder.members))
            .cloned()
            .collect()
    }
}

/// Repair report for older persisted documents. Does not rewrite live runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyGroupRepairReport {
    pub dropped_layouts: Vec<ProfileId>,
    pub dropped_folders: Vec<(ProfileId, FolderId)>,
    pub dropped_members: Vec<(ProfileId, String)>,
    pub renamed: Vec<(ProfileId, FolderId)>,
}

impl ProxyGroupRepairReport {
    pub fn is_empty(&self) -> bool {
        self.dropped_layouts.is_empty()
            && self.dropped_folders.is_empty()
            && self.dropped_members.is_empty()
            && self.renamed.is_empty()
    }
}

const REPAIRED_FOLDER_NAME: &str = "Folder";

impl ProxyGroupLayout {
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.ungrouped.is_empty()
    }

    /// Ref algorithm: retain stored order for currently live names, drop
    /// duplicates in the resolved view and append new live names in core order.
    /// Missing names stay in the stored document for a future reappearance.
    pub fn resolve(&self, live: &[String]) -> ResolvedProxyGroupLayout {
        let live_names: IndexSet<&str> = live.iter().map(String::as_str).collect();
        let mut placed = IndexSet::new();
        let mut place =
            |name: &String| live_names.contains(name.as_str()) && placed.insert(name.clone());
        let mut ungrouped: Vec<String> = self
            .ungrouped
            .iter()
            .filter(|name| place(name))
            .cloned()
            .collect();
        let folders = self
            .folders
            .iter()
            .map(|folder| ProxyGroupFolder {
                id: folder.id.clone(),
                name: folder.name.clone(),
                members: folder
                    .members
                    .iter()
                    .filter(|name| place(name))
                    .cloned()
                    .collect(),
            })
            .collect();
        for name in live {
            if place(name) {
                ungrouped.push(name.clone());
            }
        }
        ResolvedProxyGroupLayout { folders, ungrouped }
    }

    fn repair(&mut self, profile: &ProfileId, report: &mut ProxyGroupRepairReport) {
        let mut folder_ids = IndexSet::new();
        let mut placed = IndexSet::new();
        let mut keep = |name: &String, report: &mut ProxyGroupRepairReport| {
            let valid = !name.trim().is_empty() && placed.insert(name.clone());
            if !valid {
                report.dropped_members.push((profile.clone(), name.clone()));
            }
            valid
        };
        self.ungrouped.retain(|name| keep(name, report));
        let mut folders = Vec::with_capacity(self.folders.len());
        for mut folder in std::mem::take(&mut self.folders) {
            if !folder_ids.insert(folder.id.clone()) {
                report.dropped_folders.push((profile.clone(), folder.id));
                continue;
            }
            folder.members.retain(|name| keep(name, report));
            if folder.name.trim().is_empty() {
                folder.name = REPAIRED_FOLDER_NAME.to_owned();
                report.renamed.push((profile.clone(), folder.id.clone()));
            }
            folders.push(folder);
        }
        self.folders = folders;
    }
}

impl Profiles {
    pub fn repair_proxy_group_layouts(&mut self) -> ProxyGroupRepairReport {
        let mut report = ProxyGroupRepairReport::default();
        let mut layouts = std::mem::take(&mut self.proxy_group_layouts);
        layouts.retain(|profile, _| {
            let keep = self
                .items
                .get(profile)
                .is_some_and(|item| item.definition.is_config());
            if !keep {
                report.dropped_layouts.push(profile.clone());
            }
            keep
        });
        for (profile, layout) in &mut layouts {
            layout.repair(profile, &mut report);
        }
        self.proxy_group_layouts = layouts;
        report
    }
}

pub(super) fn validate_proxy_group_layouts(
    items: &IndexMap<ProfileId, ProfileItem>,
    layouts: &IndexMap<ProfileId, ProxyGroupLayout>,
    errors: &mut Vec<ProfileValidationError>,
) {
    for (profile, layout) in layouts {
        if !items
            .get(profile)
            .is_some_and(|item| item.definition.is_config())
        {
            errors.push(ProfileValidationError::ProxyGroupLayoutProfileNotConfig {
                profile: profile.clone(),
            });
        }

        let mut folder_ids = IndexSet::new();
        let mut placed: IndexSet<String> = IndexSet::new();
        let mut check_name = |name: &String, errors: &mut Vec<ProfileValidationError>| {
            if name.trim().is_empty() {
                errors.push(ProfileValidationError::ProxyGroupNameEmpty {
                    profile: profile.clone(),
                });
            }
            if !placed.insert(name.clone()) {
                errors.push(ProfileValidationError::ProxyGroupDuplicate {
                    profile: profile.clone(),
                    name: name.clone(),
                });
            }
        };

        for name in &layout.ungrouped {
            check_name(name, errors);
        }
        for folder in &layout.folders {
            if !folder_ids.insert(&folder.id) {
                errors.push(ProfileValidationError::ProxyGroupFolderDuplicateId {
                    profile: profile.clone(),
                    id: folder.id.clone(),
                });
            }
            if folder.name.trim().is_empty() {
                errors.push(ProfileValidationError::ProxyGroupFolderNameEmpty {
                    profile: profile.clone(),
                    id: folder.id.clone(),
                });
            }
            for name in &folder.members {
                check_name(name, errors);
            }
        }
    }
}

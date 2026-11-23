use crate::profile::*;

fn id(text: &str) -> ProfileId {
    ProfileId(text.to_owned())
}
fn fid(text: &str) -> FolderId {
    FolderId(text.to_owned())
}
fn names(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|name| (*name).to_owned()).collect()
}
fn folder(id: &str, name: &str, members: &[&str]) -> ProxyGroupFolder {
    ProxyGroupFolder {
        id: fid(id),
        name: name.to_owned(),
        members: names(members),
    }
}
fn config(uid: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.to_owned(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

#[test]
fn resolves_live_groups_once_and_preserves_disappeared_entries_in_storage() {
    let layout = ProxyGroupLayout {
        ungrouped: names(&["A", "Gone", "A"]),
        folders: vec![folder("f1", "Streaming", &["D", "B", "A"])],
    };
    let resolved = layout.resolve(&names(&["B", "D", "A", "C", "B"]));
    assert_eq!(resolved.ungrouped, names(&["A", "C"]));
    assert_eq!(resolved.folders[0].members, names(&["D", "B"]));
    assert_eq!(resolved.flat_order(), names(&["A", "C", "D", "B"]));
    assert_eq!(layout.ungrouped, names(&["A", "Gone", "A"]));
}

#[test]
fn serializes_layouts_without_changing_legacy_documents() {
    let mut doc = Profiles::default();
    assert!(
        !serde_yaml_ng::to_string(&doc)
            .unwrap()
            .contains("proxy_group_layouts")
    );
    assert!(doc.append_item(config("cfg")));
    doc.proxy_group_layouts.insert(
        id("cfg"),
        ProxyGroupLayout {
            ungrouped: names(&["Automatic"]),
            folders: vec![folder("f1", "Streaming", &["Netflix"])],
        },
    );
    assert!(doc.validate().is_ok());
    let dumped = serde_yaml_ng::to_string(&doc).unwrap();
    let round_trip: Profiles = serde_yaml_ng::from_str(&dumped).unwrap();
    assert_eq!(round_trip.proxy_group_layouts, doc.proxy_group_layouts);
}

#[test]
fn rejects_invalid_profile_duplicates_and_blank_names() {
    let mut doc = Profiles::default();
    assert!(doc.append_item(config("cfg")));
    doc.proxy_group_layouts.insert(
        id("missing"),
        ProxyGroupLayout {
            ungrouped: names(&["X"]),
            folders: vec![],
        },
    );
    doc.proxy_group_layouts.insert(
        id("cfg"),
        ProxyGroupLayout {
            ungrouped: names(&["A", "A", " "]),
            folders: vec![folder("same", "", &["B"]), folder("same", "Valid", &["B"])],
        },
    );
    let errors = doc.validate().unwrap_err();
    assert!(
        errors.contains(&ProfileValidationError::ProxyGroupLayoutProfileNotConfig {
            profile: id("missing")
        })
    );
    assert!(
        errors.contains(&ProfileValidationError::ProxyGroupDuplicate {
            profile: id("cfg"),
            name: "A".into()
        })
    );
    assert!(
        errors.contains(&ProfileValidationError::ProxyGroupFolderDuplicateId {
            profile: id("cfg"),
            id: fid("same")
        })
    );
    assert!(
        errors.contains(&ProfileValidationError::ProxyGroupFolderNameEmpty {
            profile: id("cfg"),
            id: fid("same")
        })
    );
    assert!(errors.contains(&ProfileValidationError::ProxyGroupNameEmpty { profile: id("cfg") }));
}

#[test]
fn repairs_invalid_layout_metadata_without_rewriting_runtime_group_names() {
    let mut doc = Profiles::default();
    assert!(doc.append_item(config("cfg")));
    doc.proxy_group_layouts.insert(
        id("gone"),
        ProxyGroupLayout {
            ungrouped: names(&["Gone"]),
            folders: vec![],
        },
    );
    doc.proxy_group_layouts.insert(
        id("cfg"),
        ProxyGroupLayout {
            ungrouped: names(&["A", "A"]),
            folders: vec![
                folder("f1", "", &["B", "A"]),
                folder("f1", "Repeat", &["C"]),
            ],
        },
    );
    let report = doc.repair_proxy_group_layouts();
    assert_eq!(report.dropped_layouts, vec![id("gone")]);
    assert_eq!(report.dropped_folders, vec![(id("cfg"), fid("f1"))]);
    assert!(doc.validate().is_ok());
    assert_eq!(doc.proxy_group_layouts[&id("cfg")].ungrouped, names(&["A"]));
    assert_eq!(
        doc.proxy_group_layouts[&id("cfg")].folders[0].name,
        "Folder"
    );
    assert_eq!(
        doc.proxy_group_layouts[&id("cfg")].folders[0].members,
        names(&["B"])
    );
    assert!(doc.repair_proxy_group_layouts().is_empty());
}

#[test]
fn deleting_profile_removes_only_its_proxy_group_layout() {
    let mut doc = Profiles::default();
    for uid in ["a", "b"] {
        assert!(doc.append_item(config(uid)));
        doc.proxy_group_layouts.insert(
            id(uid),
            ProxyGroupLayout {
                ungrouped: names(&["Proxy"]),
                folders: vec![],
            },
        );
    }
    assert!(doc.remove_item_unchecked(&id("a")).is_some());
    assert!(!doc.proxy_group_layouts.contains_key(&id("a")));
    assert!(doc.proxy_group_layouts.contains_key(&id("b")));
    assert!(doc.validate().is_ok());
}

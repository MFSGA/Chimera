//! Ref-aligned frozen content and active Profile closure for candidate builds.
//! This module owns neither runtime state nor the filesystem; platform
//! callers supply a ProfileContentSource and the staged source bytes.

use std::collections::BTreeMap;

use chimera_config::{
    profile::{ConfigDefinition, ManagedProfilePath, ProfileDefinition, ProfileId, Profiles},
    runtime::executor::{PortError, ProfileContentSource},
};
use indexmap::IndexSet;

/// Exactly the active Profile dependency closure used by Profile mutations
/// and runtime content capture. Global transforms are included even when the
/// selected profile has no content; composition source members and their
/// transforms are included in deterministic insertion order.
pub fn current_profile_closure(profiles: &Profiles) -> IndexSet<ProfileId> {
    let mut closure: indexmap::IndexSet<ProfileId> =
        profiles.global_transforms.iter().cloned().collect();
    let Some(current) = &profiles.current else {
        return closure;
    };

    closure.insert(current.clone());
    let mut configs = vec![current.clone()];
    if let Some(item) = profiles.items.get(current)
        && let ProfileDefinition::Config {
            config: ConfigDefinition::Composition(composition),
        } = &item.definition
    {
        if let Some(base) = &composition.base {
            closure.insert(base.clone());
            configs.push(base.clone());
        }
        for member in &composition.extend_proxies_from {
            closure.insert(member.clone());
            configs.push(member.clone());
        }
    }

    for config in configs {
        if let Some(item) = profiles.items.get(&config)
            && let ProfileDefinition::Config { config } = &item.definition
        {
            for transform in config.transforms() {
                closure.insert(transform.clone());
            }
        }
    }

    closure
}

#[derive(Debug, Clone, Default)]
pub struct FrozenProfileContent(pub BTreeMap<String, Result<String, String>>);

impl FrozenProfileContent {
    pub fn capture_content(
        profiles: &Profiles,
        source: &dyn ProfileContentSource,
        staged_content: BTreeMap<String, String>,
    ) -> Self {
        let mut content = Self::default();
        for uid in current_profile_closure(profiles) {
            if let Some(source_path) = profiles
                .items
                .get(&uid)
                .and_then(|item| item.definition.source())
            {
                let path = &source_path.materialized().file;
                content.0.insert(
                    path.to_string(),
                    source.read(path).map_err(|error| error.to_string()),
                );
            }
        }
        for (path, staged) in staged_content {
            if let Some(captured) = content.0.get_mut(&path) {
                *captured = Ok(staged);
            }
        }
        content
    }
}

impl ProfileContentSource for FrozenProfileContent {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        self.0
            .get(&path.to_string())
            .cloned()
            .map(|result| result.map_err(Into::into))
            .ok_or_else(|| -> PortError {
                format!("profile content was not captured: {path}").into()
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::profile::{
        CompositionConfig, ConfigDefinition, FileConfig, LocalBinding, MaterializedFile,
        ProfileItem, ProfileMetadata, ProfileSource,
    };

    struct MemorySource(BTreeMap<String, String>);
    impl ProfileContentSource for MemorySource {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            self.0
                .get(&path.to_string())
                .cloned()
                .ok_or_else(|| format!("missing captured source: {path}").into())
        }
    }

    fn file_profile(id: &str, file: &str, transforms: &[&str]) -> ProfileItem {
        ProfileItem {
            uid: ProfileId(id.into()),
            metadata: ProfileMetadata {
                name: id.into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new(file).unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: transforms
                        .iter()
                        .map(|id| ProfileId((*id).into()))
                        .collect(),
                }),
            },
        }
    }

    #[test]
    fn closure_retains_global_current_and_selected_transforms_in_order() {
        let mut profiles = Profiles::default();
        profiles.global_transforms = vec![ProfileId("global".into())];
        profiles.current = Some(ProfileId("selected".into()));
        profiles.items.insert(
            ProfileId("selected".into()),
            file_profile("selected", "selected.yaml", &["local", "global"]),
        );
        let ids: Vec<_> = current_profile_closure(&profiles)
            .into_iter()
            .map(|id| id.0)
            .collect();
        assert_eq!(ids, ["global", "selected", "local"]);
    }

    #[test]
    fn composition_closure_includes_base_contributors_and_their_scoped_transforms() {
        let mut profiles = Profiles::default();
        profiles.global_transforms = vec![ProfileId("global".into())];
        profiles.current = Some(ProfileId("composed".into()));
        profiles.items.insert(
            ProfileId("composed".into()),
            ProfileItem {
                uid: ProfileId("composed".into()),
                metadata: ProfileMetadata {
                    name: "composed".into(),
                    desc: None,
                    custom_name: true,
                },
                definition: ProfileDefinition::Config {
                    config: ConfigDefinition::Composition(CompositionConfig {
                        base: Some(ProfileId("base".into())),
                        extend_proxies_from: vec![ProfileId("extra".into())],
                        transforms: vec![ProfileId("after".into())],
                    }),
                },
            },
        );
        profiles.items.insert(
            ProfileId("base".into()),
            file_profile("base", "base.yaml", &["base-script"]),
        );
        profiles.items.insert(
            ProfileId("extra".into()),
            file_profile("extra", "extra.yaml", &["extra-script"]),
        );
        let ids: Vec<_> = current_profile_closure(&profiles)
            .into_iter()
            .map(|id| id.0)
            .collect();
        assert_eq!(
            ids,
            [
                "global",
                "composed",
                "base",
                "extra",
                "after",
                "base-script",
                "extra-script"
            ]
        );
    }

    #[test]
    fn capture_keeps_read_errors_until_a_matching_staged_override_replaces_them() {
        let mut profiles = Profiles::default();
        profiles.current = Some(ProfileId("selected".into()));
        profiles.items.insert(
            ProfileId("selected".into()),
            file_profile("selected", "selected.yaml", &[]),
        );
        let missing = MemorySource(BTreeMap::new());
        let frozen = FrozenProfileContent::capture_content(&profiles, &missing, BTreeMap::new());
        let path = ManagedProfilePath::new("selected.yaml").unwrap();
        assert!(frozen.read(&path).is_err());

        let frozen = FrozenProfileContent::capture_content(
            &profiles,
            &missing,
            BTreeMap::from([
                ("selected.yaml".into(), "proxies: []".into()),
                ("unused.yaml".into(), "ignored".into()),
            ]),
        );
        assert_eq!(frozen.read(&path).unwrap(), "proxies: []");
        assert!(!frozen.0.contains_key("unused.yaml"));
        assert!(
            frozen
                .read(&ManagedProfilePath::new("not-captured.yaml").unwrap())
                .is_err()
        );
    }
}

//! Compatibility conversion from Chimera's persisted profile document to the
//! ref-aligned shared Profile domain.
//!
//! The persisted document is intentionally not rewritten in this stage. The
//! adapter gives the RuntimeBuilder the same semantic model as ref while the
//! legacy UI and its storage migration continue to use the existing schema.

use anyhow::Result;
use chimera_config::profile::{
    ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
    OverlayTransform, ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource,
    Profiles as RuntimeProfiles, ScriptRuntime, ScriptTransform, SubscriptionInfo,
    TransformDefinition,
};
use time::OffsetDateTime;

use super::{
    item::{Profile, ProfileMetaGetter, local::LocalProfile, remote::RemoteProfile},
    profiles::Profiles,
};

/// Convert the persisted legacy profile document into the ref-aligned runtime
/// document without mutating or rewriting the user's profile file.
pub(crate) fn to_runtime_profiles(source: &Profiles) -> Result<RuntimeProfiles> {
    let mut runtime = RuntimeProfiles::default();
    runtime.global_transforms = source.chain.iter().cloned().map(ProfileId).collect();
    runtime.valid = source.valid.clone();

    for item in &source.items {
        runtime.append_item(convert_item(item)?);
    }

    match source.current.as_slice() {
        [] => {}
        [uid] => runtime.current = Some(ProfileId(uid.clone())),
        selected => {
            // The legacy UI can select multiple config profiles. ref models
            // this as one composition: the first profile supplies the full
            // config and the remaining profiles contribute proxies in order.
            let base = ProfileId(selected[0].clone());
            let contributors = selected[1..].iter().cloned().map(ProfileId).collect();
            let synthetic = synthetic_current_id(&runtime);
            runtime.append_item(ProfileItem {
                uid: synthetic.clone(),
                metadata: ProfileMetadata {
                    name: "Chimera selected profiles".into(),
                    desc: Some(
                        "Compatibility composition for legacy multi-profile selection".into(),
                    ),
                    custom_name: true,
                },
                definition: ProfileDefinition::Config {
                    config: ConfigDefinition::Composition(
                        chimera_config::profile::CompositionConfig {
                            base: Some(base),
                            extend_proxies_from: contributors,
                            transforms: Vec::new(),
                        },
                    ),
                },
            });
            runtime.current = Some(synthetic);
        }
    }

    runtime.validate().map_err(|errors| {
        anyhow::anyhow!("legacy profile conversion failed validation: {errors:?}")
    })?;
    Ok(runtime)
}

fn synthetic_current_id(profiles: &RuntimeProfiles) -> ProfileId {
    let base = "__chimera_legacy_current__";
    if !profiles.items.contains_key(&ProfileId(base.into())) {
        return ProfileId(base.into());
    }

    for index in 1.. {
        let candidate = ProfileId(format!("{base}-{index}"));
        if !profiles.items.contains_key(&candidate) {
            return candidate;
        }
    }
    unreachable!("synthetic profile id search must find a free id")
}

fn convert_item(item: &Profile) -> Result<ProfileItem> {
    let uid = ProfileId(item.uid().to_string());
    let metadata = ProfileMetadata {
        name: profile_name(item),
        desc: profile_desc(item),
        // Legacy documents do not carry name provenance. ref treats absent
        // provenance as user-owned so refreshes cannot silently rename data.
        custom_name: true,
    };

    let definition = match item {
        Profile::Local(profile) => ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: local_source(profile)?,
                transforms: profile.chain.iter().cloned().map(ProfileId).collect(),
            }),
        },
        Profile::Remote(profile) => ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: remote_source(profile)?,
                transforms: profile.chain.iter().cloned().map(ProfileId).collect(),
            }),
        },
        Profile::Merge(profile) => ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: managed_source(&profile.shared.file, profile.shared.updated)?,
            }),
        },
        Profile::Script(profile) => ProfileDefinition::Transform {
            transform: TransformDefinition::Script(ScriptTransform {
                source: managed_source(&profile.shared.file, profile.shared.updated)?,
                runtime: match profile.script_type {
                    super::item_type::ScriptType::JavaScript => ScriptRuntime::JavaScript,
                    super::item_type::ScriptType::Lua => ScriptRuntime::Lua,
                },
            }),
        },
    };

    Ok(ProfileItem {
        uid,
        metadata,
        definition,
    })
}

fn profile_name(item: &Profile) -> String {
    match item {
        Profile::Local(profile) => profile.shared.name.clone(),
        Profile::Remote(profile) => profile.shared.name.clone(),
        Profile::Merge(profile) => profile.shared.name.clone(),
        Profile::Script(profile) => profile.shared.name.clone(),
    }
}

fn profile_desc(item: &Profile) -> Option<String> {
    match item {
        Profile::Local(profile) => profile.shared.desc.clone(),
        Profile::Remote(profile) => profile.shared.desc.clone(),
        Profile::Merge(profile) => profile.shared.desc.clone(),
        Profile::Script(profile) => profile.shared.desc.clone(),
    }
}

fn local_source(profile: &LocalProfile) -> Result<ProfileSource> {
    managed_source(&profile.shared.file, profile.shared.updated)
}

fn remote_source(profile: &RemoteProfile) -> Result<ProfileSource> {
    Ok(ProfileSource::Remote {
        materialized: materialized_file(&profile.shared.file, profile.shared.updated)?,
        url: profile.url.clone(),
        option: chimera_config::profile::RemoteProfileOptions {
            user_agent: profile.option.user_agent.clone(),
            with_proxy: profile.option.with_proxy,
            self_proxy: profile.option.self_proxy,
            update_interval_minutes: profile.option.update_interval_minutes,
        },
        subscription: SubscriptionInfo {
            upload: nonzero(profile.extra.upload),
            download: nonzero(profile.extra.download),
            total: nonzero(profile.extra.total),
            expire: unix_timestamp(profile.extra.expire),
        },
    })
}

fn managed_source(file: &str, updated: usize) -> Result<ProfileSource> {
    Ok(ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: materialized_file(file, updated)?,
        },
    })
}

fn materialized_file(file: &str, updated: usize) -> Result<MaterializedFile> {
    let file = ManagedProfilePath::new(file.to_string())
        .map_err(|error| anyhow::anyhow!("invalid managed profile path {file}: {error}"))?;
    let updated_at = if updated == 0 {
        None
    } else {
        OffsetDateTime::from_unix_timestamp(updated as i64).ok()
    };
    Ok(MaterializedFile { file, updated_at })
}

fn nonzero(value: usize) -> Option<u64> {
    (value != 0).then_some(value as u64)
}

fn unix_timestamp(value: usize) -> Option<OffsetDateTime> {
    (value != 0)
        .then(|| OffsetDateTime::from_unix_timestamp(value as i64).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::item::{local::LocalProfile, shared::ProfileShared};

    fn local(uid: &str, file: &str) -> Profile {
        Profile::Local(LocalProfile {
            shared: ProfileShared {
                uid: uid.into(),
                name: uid.into(),
                file: file.into(),
                desc: None,
                updated: 1,
            },
            symlinks: None,
            chain: Vec::new(),
        })
    }

    #[test]
    fn converts_legacy_multi_selection_to_ref_composition() {
        let mut source = Profiles::default();
        source.items = vec![local("a", "a.yaml"), local("b", "b.yaml")];
        source.current = vec!["a".into(), "b".into()];

        let runtime = to_runtime_profiles(&source).unwrap();
        let current = runtime.current.unwrap();
        let item = runtime.items.get(&current).unwrap();
        let ProfileDefinition::Config {
            config: ConfigDefinition::Composition(composition),
        } = &item.definition
        else {
            panic!("expected synthetic composition")
        };
        assert_eq!(composition.base, Some(ProfileId("a".into())));
        assert_eq!(composition.extend_proxies_from, vec![ProfileId("b".into())]);
    }

    #[test]
    fn preserves_empty_selection_as_bare_runtime_target() {
        let source = Profiles::default();
        let runtime = to_runtime_profiles(&source).unwrap();
        assert!(runtime.current.is_none());
    }
}

use std::sync::Arc;

use json_patch::{
    AddOperation, Patch, PatchOperation, RemoveOperation, ReplaceOperation, jsonptr::PointerBuf,
};

use crate::runtime::value::{ConfigObject, ConfigValue};

use super::{Ownership, OwnershipEntries};

/// Writes Forced keys over the config and removes Removed keys. Returns the
/// result with the patch that turns config into it.
pub fn apply_settings(config: &ConfigValue, entries: &OwnershipEntries) -> (ConfigValue, Patch) {
    apply(config, entries, Pass::Settings)
}

/// Writes Default keys that the config mapping lacks, with the patch shape
/// used by apply_settings.
pub fn apply_defaults(config: &ConfigValue, entries: &OwnershipEntries) -> (ConfigValue, Patch) {
    apply(config, entries, Pass::Defaults)
}

/// Copies Inherited keys from source into filtered, creating missing parent
/// mappings. The field filter only drops whole top-level keys of source.
pub fn keep_inherited(
    filtered: &ConfigValue,
    source: &ConfigValue,
    entries: &OwnershipEntries,
) -> (ConfigValue, Patch) {
    let mut ops = Vec::new();
    if let Some(source) = source.as_object_arc()
        && inherits(entries)
    {
        let mut map = object_of(Some(filtered));
        keep(
            &mut map,
            source,
            entries,
            &PointerBuf::new(),
            Some(&mut ops),
        );
        if !ops.is_empty() {
            return (ConfigValue::Object(Arc::new(map)), Patch(ops));
        }
    }
    (filtered.clone(), Patch(ops))
}

fn inherits(entries: &OwnershipEntries) -> bool {
    entries.iter().any(|(_, ownership)| match ownership {
        Ownership::Inherited => true,
        Ownership::Table { entries, .. } => inherits(entries),
        _ => false,
    })
}

/// Edits one mapping in place and records changes as JSON Patch operations.
fn keep(
    map: &mut ConfigObject,
    source: &ConfigObject,
    entries: &OwnershipEntries,
    path: &PointerBuf,
    mut ops: Option<&mut Vec<PatchOperation>>,
) -> bool {
    let mut changed = false;
    for (key, ownership) in entries {
        let Some(from) = source.get(*key) else {
            continue;
        };
        match ownership {
            Ownership::Inherited if !map.contains_key(*key) => {
                map.insert(Arc::from(*key), from.clone());
                if let Some(ops) = ops.as_deref_mut() {
                    ops.push(add(path, key, from));
                }
                changed = true;
            }
            Ownership::Table { entries, .. } if inherits(entries) => {
                let Some(from) = from.as_object_arc() else {
                    continue;
                };
                match map.get(*key) {
                    Some(ConfigValue::Object(table)) => {
                        let mut table = (**table).clone();
                        let child = path.with_trailing_token(*key);
                        if keep(&mut table, from, entries, &child, ops.as_deref_mut()) {
                            set(map, key, ConfigValue::Object(Arc::new(table)));
                            changed = true;
                        }
                    }
                    None => {
                        let mut table = ConfigObject::new();
                        if keep(&mut table, from, entries, path, None) {
                            let table = ConfigValue::Object(Arc::new(table));
                            if let Some(ops) = ops.as_deref_mut() {
                                ops.push(add(path, key, &table));
                            }
                            set(map, key, table);
                            changed = true;
                        }
                    }
                    Some(_) => {}
                }
            }
            _ => {}
        }
    }
    changed
}

#[derive(Clone, Copy)]
enum Pass {
    Settings,
    Defaults,
}

impl Pass {
    fn handles(self, ownership: &Ownership) -> bool {
        match (self, ownership) {
            (Self::Settings, Ownership::Forced(_) | Ownership::Removed) => true,
            (Self::Defaults, Ownership::Default(_)) => true,
            (_, Ownership::Table { entries, .. }) => self.touches(entries),
            _ => false,
        }
    }

    fn touches(self, entries: &OwnershipEntries) -> bool {
        entries.iter().any(|(_, ownership)| self.handles(ownership))
    }
}

fn apply(config: &ConfigValue, entries: &OwnershipEntries, pass: Pass) -> (ConfigValue, Patch) {
    let mut ops = Vec::new();
    if pass.touches(entries) {
        let mut map = object_of(Some(config));
        walk(&mut map, entries, pass, &PointerBuf::new(), Some(&mut ops));
        if !ops.is_empty() {
            return (ConfigValue::Object(Arc::new(map)), Patch(ops));
        }
    }
    (config.clone(), Patch(ops))
}

/// Edits one mapping in place. Each changed table costs one shallow copy;
/// untouched values stay shared. A newly created table is recorded whole.
fn walk(
    map: &mut ConfigObject,
    entries: &OwnershipEntries,
    pass: Pass,
    path: &PointerBuf,
    mut ops: Option<&mut Vec<PatchOperation>>,
) -> bool {
    let mut changed = false;
    for (key, ownership) in entries {
        let record = |ops: &mut Option<&mut Vec<PatchOperation>>, operation| {
            if let Some(ops) = ops {
                ops.push(operation);
            }
        };
        match (pass, ownership) {
            (Pass::Settings, Ownership::Forced(value)) => match map.get_mut(*key) {
                Some(slot) if slot == value => {}
                Some(slot) => {
                    *slot = value.clone();
                    record(&mut ops, replace(path, key, value));
                    changed = true;
                }
                None => {
                    map.insert(Arc::from(*key), value.clone());
                    record(&mut ops, add(path, key, value));
                    changed = true;
                }
            },
            (Pass::Settings, Ownership::Removed) => {
                if map.shift_remove(*key).is_some() {
                    record(
                        &mut ops,
                        PatchOperation::Remove(RemoveOperation {
                            path: path.with_trailing_token(*key),
                        }),
                    );
                    changed = true;
                }
            }
            (Pass::Defaults, Ownership::Default(value)) => {
                if !map.contains_key(*key) {
                    map.insert(Arc::from(*key), value.clone());
                    record(&mut ops, add(path, key, value));
                    changed = true;
                }
            }
            (_, Ownership::Table { entries, create }) if pass.touches(entries) => {
                match map.get(*key) {
                    Some(ConfigValue::Object(table)) => {
                        let mut table = (**table).clone();
                        let child = path.with_trailing_token(*key);
                        if walk(&mut table, entries, pass, &child, ops.as_deref_mut()) {
                            set(map, key, ConfigValue::Object(Arc::new(table)));
                            changed = true;
                        }
                    }
                    existing if existing.is_some() || *create => {
                        let replaced = existing.is_some();
                        let mut table = ConfigObject::new();
                        if walk(&mut table, entries, pass, path, None) {
                            let table = ConfigValue::Object(Arc::new(table));
                            record(
                                &mut ops,
                                if replaced {
                                    replace(path, key, &table)
                                } else {
                                    add(path, key, &table)
                                },
                            );
                            set(map, key, table);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    changed
}

fn add(path: &PointerBuf, key: &str, value: &ConfigValue) -> PatchOperation {
    PatchOperation::Add(AddOperation {
        path: path.with_trailing_token(key),
        value: value.to_json(),
    })
}

fn replace(path: &PointerBuf, key: &str, value: &ConfigValue) -> PatchOperation {
    PatchOperation::Replace(ReplaceOperation {
        path: path.with_trailing_token(key),
        value: value.to_json(),
    })
}

fn object_of(value: Option<&ConfigValue>) -> ConfigObject {
    value
        .and_then(ConfigValue::as_object_arc)
        .map(|map| (**map).clone())
        .unwrap_or_default()
}

/// Replaces a value in place, keeping the key's position and allocation.
fn set(map: &mut ConfigObject, key: &str, value: ConfigValue) {
    match map.get_mut(key) {
        Some(slot) => *slot = value,
        None => {
            map.insert(Arc::from(key), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn value(value: serde_json::Value) -> ConfigValue {
        ConfigValue::try_from(value).unwrap()
    }

    fn checked(
        config: serde_json::Value,
        walk: impl FnOnce(&ConfigValue) -> (ConfigValue, Patch),
    ) -> serde_json::Value {
        let (result, patch) = walk(&value(config.clone()));
        let result = result.to_json();
        let mut replayed = config;
        json_patch::patch(&mut replayed, &patch).unwrap();
        assert_eq!(
            serde_json::to_string(&replayed).unwrap(),
            serde_json::to_string(&result).unwrap(),
            "the ownership patch must replay into the result"
        );
        result
    }

    #[test]
    fn keep_inherited_restores_only_the_owned_nested_keys() {
        let source = value(json!({
            "tun": {
                "auto-route": true,
                "stack": "system",
                "user-extra": "preserved only when the table is retained"
            }
        }));
        let filtered = value(json!({ "tun": { "stack": "gvisor" } }));
        let entries = vec![(
            "tun",
            Ownership::Table {
                entries: vec![
                    ("auto-route", Ownership::Inherited),
                    (
                        "stack",
                        Ownership::Forced(ConfigValue::from_json_value(json!("gvisor"))),
                    ),
                ],
                create: true,
            },
        )];

        let result = checked(filtered.to_json(), |filtered| {
            keep_inherited(filtered, &source, &entries)
        });

        assert_eq!(
            result,
            json!({ "tun": { "stack": "gvisor", "auto-route": true } })
        );
    }

    #[test]
    fn inherited_table_is_restored_when_its_parent_was_filtered() {
        let source = value(json!({
            "tun": { "auto-route": true, "stack": "system" },
            "dns": { "enable": false }
        }));
        let entries = vec![
            (
                "tun",
                Ownership::Table {
                    entries: vec![("auto-route", Ownership::Inherited)],
                    create: true,
                },
            ),
            ("dns", Ownership::Inherited),
        ];

        let result = checked(json!({}), |filtered| {
            keep_inherited(filtered, &source, &entries)
        });

        assert_eq!(
            result,
            json!({
                "tun": { "auto-route": true },
                "dns": { "enable": false }
            })
        );
    }

    #[test]
    fn inherited_nested_fields_are_not_restored_from_non_mapping_values() {
        let source = value(json!({ "tun": true }));
        let entries = vec![(
            "tun",
            Ownership::Table {
                entries: vec![("auto-route", Ownership::Inherited)],
                create: true,
            },
        )];

        let result = checked(json!({}), |filtered| {
            keep_inherited(filtered, &source, &entries)
        });

        assert_eq!(result, json!({}));
    }

    #[test]
    fn settings_force_remove_and_preserve_unmanaged_values() {
        let config = value(json!({
            "allow-lan": false,
            "port": 8080,
            "tun": { "auto-route": true, "stack": "system" }
        }));
        let entries = vec![
            (
                "allow-lan",
                Ownership::Forced(ConfigValue::from_json_value(json!(true))),
            ),
            ("port", Ownership::Removed),
            (
                "tun",
                Ownership::Table {
                    entries: vec![(
                        "auto-route",
                        Ownership::Forced(ConfigValue::from_json_value(json!(false))),
                    )],
                    create: true,
                },
            ),
        ];

        let result = checked(config.to_json(), |config| apply_settings(config, &entries));

        assert_eq!(
            result,
            json!({
                "allow-lan": true,
                "tun": { "auto-route": false, "stack": "system" }
            })
        );
    }

    #[test]
    fn a_missing_table_is_created_only_when_requested() {
        let forced = vec![("enable", Ownership::Forced(value(json!(false))))];
        let create = vec![(
            "tun",
            Ownership::Table {
                entries: forced.clone(),
                create: true,
            },
        )];
        let no_create = vec![(
            "tun",
            Ownership::Table {
                entries: forced.clone(),
                create: false,
            },
        )];

        assert_eq!(
            checked(json!({}), |config| apply_settings(config, &create)),
            json!({ "tun": { "enable": false } })
        );
        assert_eq!(
            checked(json!({}), |config| apply_settings(config, &no_create)),
            json!({})
        );
        assert_eq!(
            checked(json!({ "tun": true }), |config| apply_settings(
                config, &no_create
            )),
            json!({ "tun": { "enable": false } })
        );
    }

    #[test]
    fn writing_the_same_values_records_no_change() {
        let config = value(json!({ "mode": "rule", "tun": { "enable": true } }));
        let entries = vec![
            ("mode", Ownership::Forced(value(json!("rule")))),
            ("port", Ownership::Removed),
            (
                "tun",
                Ownership::Table {
                    entries: vec![("enable", Ownership::Forced(value(json!(true))))],
                    create: true,
                },
            ),
        ];

        let (result, patch) = apply_settings(&config, &entries);

        assert!(patch.0.is_empty());
        assert!(Arc::ptr_eq(
            result.as_object_arc().unwrap(),
            config.as_object_arc().unwrap()
        ));
    }

    #[test]
    fn defaults_fill_only_missing_keys() {
        let config = value(json!({ "profile": { "store-selected": false } }));
        let entries = vec![(
            "profile",
            Ownership::Table {
                entries: vec![
                    (
                        "store-selected",
                        Ownership::Default(ConfigValue::from_json_value(json!(true))),
                    ),
                    (
                        "store-fake-ip",
                        Ownership::Default(ConfigValue::from_json_value(json!(false))),
                    ),
                ],
                create: true,
            },
        )];

        let result = checked(config.to_json(), |config| apply_defaults(config, &entries));

        assert_eq!(
            result,
            json!({ "profile": { "store-selected": false, "store-fake-ip": false } })
        );
    }
}

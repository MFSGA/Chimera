//! Frontend namespace, including the narrow legacy migration whitelist.
use super::{Result, Storage, WebStorage};
use std::collections::{BTreeMap, BTreeSet};

pub const WEB_STORAGE_KEY_PREFIX: &str = "web:";

/// Keys written by frontend releases before web storage was namespaced.
/// Keep this list narrow so storage commands never treat internal keys as UI data.
const LEGACY_WEB_STORAGE_KEYS: &[&str] = &["connections_table_columns", "dashboard-widget-configs"];

pub fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}

fn is_legacy_web_storage_key(key: &str) -> bool {
    LEGACY_WEB_STORAGE_KEYS.contains(&key)
}

pub fn get_web_storage_item(storage: &Storage, key: &str) -> Result<super::StorageItem<String>> {
    let namespaced_key = web_key(key);
    if is_legacy_web_storage_key(key) {
        Ok(storage.get_item_with_fallback(&namespaced_key, key)?)
    } else {
        Ok(storage.get_item(namespaced_key)?)
    }
}

pub fn remove_web_storage_item(storage: &Storage, key: &str) -> Result<super::StorageRevision> {
    let namespaced_key = web_key(key);
    if is_legacy_web_storage_key(key) {
        Ok(storage.remove_item_with_fallback(namespaced_key, key)?)
    } else {
        Ok(storage.remove_item(namespaced_key)?)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}

pub fn frontend_storage_entries(items: &[(String, String)]) -> Vec<StorageEntry> {
    let mut entries = BTreeMap::new();
    for (raw_key, value) in items {
        if let Some(key) = raw_key.strip_prefix(WEB_STORAGE_KEY_PREFIX) {
            entries.insert(key.to_owned(), value.clone());
        }
    }
    for key in LEGACY_WEB_STORAGE_KEYS {
        if !entries.contains_key(*key) {
            if let Some((_, value)) = items.iter().find(|(raw_key, _)| raw_key.as_str() == *key) {
                entries.insert((*key).to_owned(), value.clone());
            }
        }
    }
    entries
        .into_iter()
        .map(|(key, value)| StorageEntry { key, value })
        .collect()
}

fn frontend_storage_keys(items: &[(String, String)]) -> Vec<String> {
    let mut keys: BTreeSet<String> = items
        .iter()
        .filter_map(|(raw_key, _)| {
            raw_key
                .strip_prefix(WEB_STORAGE_KEY_PREFIX)
                .map(str::to_owned)
        })
        .collect();
    for key in LEGACY_WEB_STORAGE_KEYS {
        if items.iter().any(|(raw_key, _)| raw_key.as_str() == *key) {
            keys.insert((*key).to_owned());
        }
    }
    keys.into_iter().collect()
}

pub fn clear_frontend_storage(storage: &Storage) -> Result<()> {
    for key in frontend_storage_keys(&storage.get_all()?) {
        remove_web_storage_item(storage, &key)?;
    }
    Ok(())
}

pub fn set_web_storage_item(
    storage: &Storage,
    key: &str,
    value: &str,
    expected_revision: Option<super::StorageRevision>,
) -> Result<super::StorageRevision> {
    let key = web_key(key);
    match expected_revision {
        Some(expected) => storage.set_item_if_revision(key, &value, expected),
        None => storage.set_item(key, &value),
    }
}

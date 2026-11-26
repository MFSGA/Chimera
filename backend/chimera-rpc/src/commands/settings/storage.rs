//! Frontend storage using the existing redb owner and Chimera migration rules.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_core::storage::{
    Storage, WebStorage,
    frontend::{
        StorageEntry, clear_frontend_storage, frontend_storage_entries, get_web_storage_item,
        remove_web_storage_item, set_web_storage_item,
    },
};
use chimera_runtime_values::storage::{StorageItem, StorageRevision};

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[chimera_rpc::rpc(expose = [http])]
pub fn clear_storage(storage: RpcState<'_, Storage>) -> Result<()> {
    Ok(clear_frontend_storage(storage)?)
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub fn get_all_storage_items(storage: RpcState<'_, Storage>) -> Result<Vec<StorageEntry>> {
    Ok(frontend_storage_entries(&storage.get_all()?))
}

#[chimera_rpc::rpc(operation = chimera_rpc_protocol::operations::GetStorageItem, query, sensitive, expose = [http, local])]
pub fn get_storage_item(
    storage: RpcState<'_, Storage>,
    key: String,
) -> Result<StorageItem<String>> {
    Ok(get_web_storage_item(storage, &key)?)
}

#[chimera_rpc::rpc(operation = chimera_rpc_protocol::operations::RemoveStorageItem, expose = [http, local])]
pub fn remove_storage_item(storage: RpcState<'_, Storage>, key: String) -> Result<StorageRevision> {
    Ok(remove_web_storage_item(storage, &key)?)
}

#[chimera_rpc::rpc(operation = chimera_rpc_protocol::operations::SetStorageItem, expose = [http, local])]
pub fn set_storage_item(
    storage: RpcState<'_, Storage>,
    key: String,
    value: String,
) -> Result<StorageRevision> {
    Ok(set_web_storage_item(storage, &key, &value, None)?)
}

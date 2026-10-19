use std::sync::OnceLock;

use parking_lot::Mutex;
use serde::Serialize;

const WIDGET_CONFIG_KEY: &str = "dashboard-widget-configs";

#[derive(Default)]
struct StorageFailure {
    operation: Option<String>,
    armed: bool,
    persist_until_released: bool,
    triggered: bool,
    calls: u32,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageFailureStatus {
    operation: Option<String>,
    armed: bool,
    triggered: bool,
    calls: u32,
}

static STORAGE_FAILURE: OnceLock<Mutex<StorageFailure>> = OnceLock::new();

fn storage_failure() -> &'static Mutex<StorageFailure> {
    STORAGE_FAILURE.get_or_init(|| Mutex::new(StorageFailure::default()))
}

#[tauri::command]
pub fn e2e_arm_widget_storage_failure(
    operation: String,
    persist_until_released: bool,
) -> Result<(), String> {
    if !matches!(operation.as_str(), "get_storage_item" | "set_storage_item") {
        return Err(format!("Unsupported storage operation: {operation}"));
    }

    *storage_failure().lock() = StorageFailure {
        operation: Some(operation),
        armed: true,
        persist_until_released,
        triggered: false,
        calls: 0,
    };
    Ok(())
}

#[tauri::command]
pub fn e2e_widget_storage_failure_status() -> StorageFailureStatus {
    let failure = storage_failure().lock();
    StorageFailureStatus {
        operation: failure.operation.clone(),
        armed: failure.armed,
        triggered: failure.triggered,
        calls: failure.calls,
    }
}

#[tauri::command]
pub fn e2e_clear_widget_storage_failure() {
    *storage_failure().lock() = StorageFailure::default();
}

pub fn fail_storage_operation(operation: &str, key: &str) -> Result<(), crate::ipc::IpcError> {
    if key != WIDGET_CONFIG_KEY {
        return Ok(());
    }

    let mut failure = storage_failure().lock();
    if !failure.armed || failure.operation.as_deref() != Some(operation) {
        return Ok(());
    }

    failure.triggered = true;
    failure.calls = failure.calls.saturating_add(1);
    if !failure.persist_until_released {
        failure.armed = false;
    }

    Err(crate::ipc::IpcError::Custom(format!(
        "Injected {operation} failure for {WIDGET_CONFIG_KEY}."
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        e2e_arm_widget_storage_failure, e2e_clear_widget_storage_failure,
        e2e_widget_storage_failure_status, fail_storage_operation,
    };

    #[test]
    fn storage_failure_is_scoped_observable_and_clearable() {
        assert!(e2e_arm_widget_storage_failure("remove_storage_item".to_owned(), false).is_err());
        e2e_clear_widget_storage_failure();
        e2e_arm_widget_storage_failure("set_storage_item".to_owned(), false).unwrap();

        assert!(fail_storage_operation("set_storage_item", "another-key").is_ok());
        assert!(fail_storage_operation("get_storage_item", "dashboard-widget-configs").is_ok());
        assert!(fail_storage_operation("set_storage_item", "dashboard-widget-configs").is_err());
        assert_eq!(
            e2e_widget_storage_failure_status().calls,
            1,
            "one-shot failure should be counted exactly once"
        );
        assert!(!e2e_widget_storage_failure_status().armed);

        e2e_clear_widget_storage_failure();
        assert_eq!(e2e_widget_storage_failure_status().calls, 0);
        e2e_arm_widget_storage_failure("get_storage_item".to_owned(), true).unwrap();

        assert!(fail_storage_operation("get_storage_item", "dashboard-widget-configs").is_err());
        assert!(fail_storage_operation("get_storage_item", "dashboard-widget-configs").is_err());
        let status = e2e_widget_storage_failure_status();
        assert!(status.armed);
        assert!(status.triggered);
        assert_eq!(status.calls, 2);

        e2e_clear_widget_storage_failure();
        assert!(fail_storage_operation("get_storage_item", "dashboard-widget-configs").is_ok());
    }
}

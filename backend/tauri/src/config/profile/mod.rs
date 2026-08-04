static PROFILE_MUTATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) fn profile_mutation_lock() -> &'static tokio::sync::Mutex<()> {
    &PROFILE_MUTATION_LOCK
}

/// 4
pub mod builder;
/// 2
pub mod item;
/// 3
pub mod item_type;
/// 1
pub mod profiles;

#[cfg(test)]
mod tests {
    use super::profile_mutation_lock;

    #[tokio::test]
    async fn profile_mutation_lock_is_shared_across_all_writers() {
        let guard = profile_mutation_lock().lock().await;
        assert!(
            profile_mutation_lock().try_lock().is_err(),
            "a second profile writer must not enter while the shared transaction is active"
        );
        drop(guard);
        assert!(profile_mutation_lock().try_lock().is_ok());
    }
}

use std::{
    ops::Deref,
    sync::{Arc, atomic::AtomicBool},
};

use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockWriteGuard,
    lock_api::RwLockReadGuard,
};

/// State manager for the application
/// It provides a way to manage the application state, draft and persist it
/// Note: It is safe to clone the StateManager, as it is backed by an Arc
#[derive(Clone)]
pub struct ManagedState<T>
where
    T: Clone + Sync + Send,
{
    inner: Arc<ManagedStateInner<T>>,
}

impl<T> From<T> for ManagedState<T>
where
    T: Clone + Sync + Send,
{
    fn from(state: T) -> Self {
        Self {
            inner: Arc::new(ManagedStateInner::new(state)),
        }
    }
}

impl<T> Deref for ManagedState<T>
where
    T: Clone + Sync + Send,
{
    type Target = ManagedStateInner<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> ManagedState<T>
where
    T: Clone + Sync + Send,
{
    /// to auto commit the state when it is dropped
    pub fn auto_commit(&self) -> ManagedStateAutoCommit<T> {
        ManagedStateAutoCommit(self)
    }

    /// Persist the latest draft snapshot before making it the committed in-memory state.
    /// A persistence failure discards the draft so subsequent reads keep seeing the old state.
    pub fn persist_draft_with<E, F>(&self, persist: F) -> Result<(), E>
    where
        F: FnOnce(&T) -> Result<(), E>,
    {
        let snapshot = self.latest().clone();
        if let Err(error) = persist(&snapshot) {
            self.discard();
            return Err(error);
        }

        self.apply();
        Ok(())
    }

    /// Apply a mutation to a draft, then persist that exact snapshot before committing it.
    /// Mutation and persistence failures both discard partial state.
    pub fn update_and_persist_with<E, U, P>(&self, update: U, persist: P) -> Result<(), E>
    where
        U: FnOnce(&mut T) -> Result<(), E>,
        P: FnOnce(&T) -> Result<(), E>,
    {
        let update_result = {
            let mut draft = self.draft();
            update(&mut draft)
        };
        if let Err(error) = update_result {
            self.discard();
            return Err(error);
        }

        self.persist_draft_with(persist)
    }
}

pub struct ManagedStateAutoCommit<'a, T: Clone + Send + Sync>(&'a ManagedState<T>);

impl<T> Deref for ManagedStateAutoCommit<'_, T>
where
    T: Clone + Send + Sync,
{
    type Target = ManagedState<T>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<T: Clone + Send + Sync> Drop for ManagedStateAutoCommit<'_, T> {
    fn drop(&mut self) {
        println!("Auto committing state triggered the drop...");
        if self.0.is_dirty() {
            self.0.apply();
        }
    }
}

pub struct ManagedStateInner<T>
where
    T: Clone + Sync + Send,
{
    inner: RwLock<T>,
    draft: RwLock<Option<T>>,
    is_dirty: AtomicBool,
}

impl<T> ManagedStateInner<T>
where
    T: Clone + Sync + Send,
{
    /// create a new managed state
    pub fn new(state: T) -> Self {
        Self {
            inner: RwLock::new(state),
            draft: RwLock::new(None),
            is_dirty: AtomicBool::new(false),
        }
    }

    /// Get the committed state
    pub fn data(&self) -> MappedRwLockReadGuard<'_, T> {
        RwLockReadGuard::map(self.inner.read(), |guard| guard)
    }

    /// whether the state is dirty, i.e. a draft is present, and not yet committed or discarded
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(std::sync::atomic::Ordering::Acquire)
    }

    /// You can modify the draft state, and then commit it
    pub fn draft(&self) -> MappedRwLockWriteGuard<'_, T> {
        if self.is_dirty() {
            let guard = self.draft.write();
            if guard.is_some() {
                return RwLockWriteGuard::map(guard, |g| g.as_mut().unwrap());
            }
        }

        let state = self.inner.read().clone();
        self.is_dirty
            .store(true, std::sync::atomic::Ordering::Release);

        RwLockWriteGuard::map(self.draft.write(), move |guard| {
            *guard = Some(state);
            guard.as_mut().unwrap()
        })
    }

    /// commit the draft state, and make it the new state
    pub fn apply(&self) -> Option<T> {
        if !self.is_dirty() {
            return None;
        }

        let mut draft = self.draft.write();
        let mut inner = self.inner.write();
        let old_value = inner.to_owned();
        if let Some(draft_value) = draft.take() {
            *inner = draft_value;
            self.is_dirty
                .store(false, std::sync::atomic::Ordering::Release);
            Some(old_value)
        } else {
            self.is_dirty
                .store(false, std::sync::atomic::Ordering::Release);
            None
        }
    }

    /// get the current state, it will return the ManagedStateLocker for the state
    pub fn latest(&self) -> MappedRwLockReadGuard<'_, T> {
        if self.is_dirty() {
            let draft = self.draft.read();
            if draft.is_some() {
                RwLockReadGuard::map(draft, |guard| guard.as_ref().unwrap())
            } else {
                let state = self.inner.read();
                RwLockReadGuard::map(state, |guard| guard)
            }
        } else {
            let state = self.inner.read();
            RwLockReadGuard::map(state, |guard| guard)
        }
    }

    /// discard the draft state
    pub fn discard(&self) -> Option<T> {
        let v = self.draft.write().take();
        self.is_dirty
            .store(false, std::sync::atomic::Ordering::Release);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::ManagedState;

    #[test]
    fn persistence_failure_discards_draft_without_changing_committed_state() {
        let state = ManagedState::from(vec!["committed"]);
        state.draft().push("draft");

        let error = state
            .persist_draft_with(|snapshot| {
                assert_eq!(snapshot, &["committed", "draft"]);
                Err::<(), _>("injected persistence failure")
            })
            .expect_err("persistence failure must be returned");

        assert_eq!(error, "injected persistence failure");
        assert_eq!(&*state.data(), &["committed"]);
        assert!(!state.is_dirty());
    }

    #[test]
    fn mutation_failure_discards_partial_draft_without_running_persistence() {
        let state = ManagedState::from(vec!["committed"]);
        let mut persistence_called = false;

        let error = state
            .update_and_persist_with(
                |draft| {
                    draft.push("partial");
                    Err::<(), _>("injected mutation failure")
                },
                |_| {
                    persistence_called = true;
                    Ok(())
                },
            )
            .expect_err("mutation failure must be returned");

        assert_eq!(error, "injected mutation failure");
        assert!(!persistence_called);
        assert_eq!(&*state.data(), &["committed"]);
        assert!(!state.is_dirty());
    }

    #[test]
    fn successful_persistence_commits_exact_saved_snapshot() {
        let state = ManagedState::from(vec!["committed"]);
        state.draft().push("draft");
        let mut persisted = Vec::new();

        state
            .persist_draft_with(|snapshot| {
                assert_eq!(&*state.data(), &["committed"]);
                persisted = snapshot.clone();
                Ok::<(), &'static str>(())
            })
            .expect("successful persistence must commit the draft");

        assert_eq!(persisted, ["committed", "draft"]);
        assert_eq!(&*state.data(), &["committed", "draft"]);
        assert!(!state.is_dirty());
    }
}

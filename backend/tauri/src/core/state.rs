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
}

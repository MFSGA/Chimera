use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Semaphore;

use super::super::model::AgentProcessPrivilegeStatus;

const PRIVILEGE_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) type PrivilegeCollector = Arc<dyn Fn() -> AgentProcessPrivilegeStatus + Send + Sync>;

pub(super) struct UnixPlatformReadinessCore {
    collector: PrivilegeCollector,
    single_flight: Arc<Semaphore>,
    cached: Arc<Mutex<Option<AgentProcessPrivilegeStatus>>>,
}

impl UnixPlatformReadinessCore {
    pub(super) fn new(collector: PrivilegeCollector) -> Self {
        Self {
            collector,
            single_flight: Arc::new(Semaphore::new(1)),
            cached: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) async fn process_privilege(&self) -> AgentProcessPrivilegeStatus {
        if let Some(status) = cached_status(&self.cached) {
            return status;
        }

        let collector = self.collector.clone();
        let single_flight = self.single_flight.clone();
        let cached = self.cached.clone();
        let task = tokio::spawn(async move {
            let _permit = match single_flight.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return AgentProcessPrivilegeStatus::Unknown,
            };
            if let Some(status) = cached_status(&cached) {
                return status;
            }
            let status = tokio::task::spawn_blocking(move || collector())
                .await
                .unwrap_or(AgentProcessPrivilegeStatus::Unknown);
            store_status(&cached, status);
            status
        });

        match tokio::time::timeout(PRIVILEGE_PROBE_TIMEOUT, task).await {
            Ok(Ok(status)) => status,
            _ => AgentProcessPrivilegeStatus::Unknown,
        }
    }
}

fn cached_status(
    cached: &Mutex<Option<AgentProcessPrivilegeStatus>>,
) -> Option<AgentProcessPrivilegeStatus> {
    cached.lock().ok().and_then(|guard| *guard)
}

fn store_status(
    cached: &Mutex<Option<AgentProcessPrivilegeStatus>>,
    status: AgentProcessPrivilegeStatus,
) {
    if status == AgentProcessPrivilegeStatus::Unknown {
        return;
    }
    if let Ok(mut guard) = cached.lock() {
        *guard = Some(status);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::UnixPlatformReadinessCore;
    use crate::features::agent::model::AgentProcessPrivilegeStatus;

    #[tokio::test]
    async fn concurrent_queries_share_one_probe_and_cache_the_result() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = calls.clone();
        let readiness = Arc::new(UnixPlatformReadinessCore::new(Arc::new(move || {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            AgentProcessPrivilegeStatus::Standard
        })));

        let (first, second) =
            tokio::join!(readiness.process_privilege(), readiness.process_privilege());
        assert_eq!(first, AgentProcessPrivilegeStatus::Standard);
        assert_eq!(second, AgentProcessPrivilegeStatus::Standard);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Standard
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_fails_closed_while_background_probe_retains_single_flight_and_caches() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = calls.clone();
        let readiness = UnixPlatformReadinessCore::new(Arc::new(move || {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(350));
            AgentProcessPrivilegeStatus::Elevated
        }));

        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Unknown
        );
        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Elevated
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unknown_result_is_not_cached_and_does_not_mask_recovery() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = calls.clone();
        let readiness = UnixPlatformReadinessCore::new(Arc::new(move || {
            if probe_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                AgentProcessPrivilegeStatus::Unknown
            } else {
                AgentProcessPrivilegeStatus::Standard
            }
        }));

        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Unknown
        );
        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Standard
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Standard
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

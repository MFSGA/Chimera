use std::{
    mem::size_of,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Semaphore;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use super::super::{model::AgentProcessPrivilegeStatus, ports::PlatformReadinessPort};

const PRIVILEGE_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

type PrivilegeCollector = Arc<dyn Fn() -> AgentProcessPrivilegeStatus + Send + Sync>;

pub(crate) struct WindowsPlatformReadiness {
    collector: PrivilegeCollector,
    single_flight: Arc<Semaphore>,
    cached: Arc<Mutex<Option<AgentProcessPrivilegeStatus>>>,
}

impl WindowsPlatformReadiness {
    pub(crate) fn new() -> Self {
        Self::with_collector(Arc::new(query_process_privilege))
    }

    fn with_collector(collector: PrivilegeCollector) -> Self {
        Self {
            collector,
            single_flight: Arc::new(Semaphore::new(1)),
            cached: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl PlatformReadinessPort for WindowsPlatformReadiness {
    async fn process_privilege(&self) -> AgentProcessPrivilegeStatus {
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

fn query_process_privilege() -> AgentProcessPrivilegeStatus {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-process handle is valid for OpenProcessToken and token receives the handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
        || token.is_null()
    {
        return AgentProcessPrivilegeStatus::Unknown;
    }

    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned = 0u32;
    // SAFETY: elevation is writable for its full size and token was returned by OpenProcessToken.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: token is an owned handle from OpenProcessToken and is closed exactly once here.
    unsafe {
        CloseHandle(token);
    }

    if result == 0 || returned < size_of::<TOKEN_ELEVATION>() as u32 {
        AgentProcessPrivilegeStatus::Unknown
    } else if elevation.TokenIsElevated == 0 {
        AgentProcessPrivilegeStatus::Standard
    } else {
        AgentProcessPrivilegeStatus::Elevated
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

    use super::{WindowsPlatformReadiness, query_process_privilege};
    use crate::features::agent::{
        model::AgentProcessPrivilegeStatus, ports::PlatformReadinessPort,
    };

    #[tokio::test]
    async fn concurrent_privilege_queries_share_one_probe_and_cache_the_result() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = calls.clone();
        let readiness = Arc::new(WindowsPlatformReadiness::with_collector(Arc::new(
            move || {
                probe_calls.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(50));
                AgentProcessPrivilegeStatus::Standard
            },
        )));

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
        let readiness = WindowsPlatformReadiness::with_collector(Arc::new(move || {
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
    async fn unknown_probe_result_is_not_cached_and_does_not_mask_recovery() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = calls.clone();
        let readiness = WindowsPlatformReadiness::with_collector(Arc::new(move || {
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

    #[test]
    fn native_privilege_probe_returns_only_a_closed_status() {
        assert!(matches!(
            query_process_privilege(),
            AgentProcessPrivilegeStatus::Elevated
                | AgentProcessPrivilegeStatus::Standard
                | AgentProcessPrivilegeStatus::Unknown
        ));
    }
}

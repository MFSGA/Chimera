use std::sync::Arc;

use nix::unistd::Uid;

use super::{
    super::{model::AgentProcessPrivilegeStatus, ports::PlatformReadinessPort},
    unix_platform_readiness::{PrivilegeCollector, UnixPlatformReadinessCore},
};

pub(crate) struct MacosPlatformReadiness {
    core: UnixPlatformReadinessCore,
}

impl MacosPlatformReadiness {
    pub(crate) fn new() -> Self {
        Self::with_collector(Arc::new(query_process_privilege))
    }

    fn with_collector(collector: PrivilegeCollector) -> Self {
        Self {
            core: UnixPlatformReadinessCore::new(collector),
        }
    }
}

#[async_trait::async_trait]
impl PlatformReadinessPort for MacosPlatformReadiness {
    async fn process_privilege(&self) -> AgentProcessPrivilegeStatus {
        self.core.process_privilege().await
    }
}

fn query_process_privilege() -> AgentProcessPrivilegeStatus {
    if Uid::effective().is_root() {
        AgentProcessPrivilegeStatus::Elevated
    } else {
        AgentProcessPrivilegeStatus::Standard
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::{MacosPlatformReadiness, query_process_privilege};
    use crate::features::agent::{
        model::AgentProcessPrivilegeStatus, ports::PlatformReadinessPort,
    };

    #[tokio::test]
    async fn wrapper_delegates_to_the_shared_core() {
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = calls.clone();
        let readiness = MacosPlatformReadiness::with_collector(Arc::new(move || {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            AgentProcessPrivilegeStatus::Standard
        }));

        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Standard
        );
        assert_eq!(
            readiness.process_privilege().await,
            AgentProcessPrivilegeStatus::Standard
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn native_privilege_probe_returns_only_a_closed_status() {
        assert!(matches!(
            query_process_privilege(),
            AgentProcessPrivilegeStatus::Elevated | AgentProcessPrivilegeStatus::Standard
        ));
    }
}

use super::super::{model::AgentProcessPrivilegeStatus, ports::PlatformReadinessPort};

#[derive(Default)]
pub(crate) struct UnavailablePlatformReadiness;

impl UnavailablePlatformReadiness {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl PlatformReadinessPort for UnavailablePlatformReadiness {
    async fn process_privilege(&self) -> AgentProcessPrivilegeStatus {
        AgentProcessPrivilegeStatus::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::UnavailablePlatformReadiness;
    use crate::features::agent::{
        model::AgentProcessPrivilegeStatus, ports::PlatformReadinessPort,
    };

    #[tokio::test]
    async fn unavailable_platform_privilege_fails_closed() {
        assert_eq!(
            UnavailablePlatformReadiness::new()
                .process_privilege()
                .await,
            AgentProcessPrivilegeStatus::Unknown
        );
    }
}

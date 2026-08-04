use std::sync::Arc;

use axum::body::Bytes;

use super::super::{
    ports::{AgentRuntimePort, AgentToolExecutionFuture, AgentToolExecutorPort, NetworkProbePort},
    registry::execute_tool,
};

pub(crate) struct RegistryAgentToolExecutor {
    network_probe: Arc<dyn NetworkProbePort>,
    runtime: Arc<dyn AgentRuntimePort>,
}

impl RegistryAgentToolExecutor {
    pub(crate) fn new(
        runtime: Arc<dyn AgentRuntimePort>,
        network_probe: Arc<dyn NetworkProbePort>,
    ) -> Self {
        Self {
            network_probe,
            runtime,
        }
    }
}

impl AgentToolExecutorPort for RegistryAgentToolExecutor {
    fn execute(&self, tool_name: String, body: Bytes) -> AgentToolExecutionFuture {
        let network_probe = self.network_probe.clone();
        let runtime = self.runtime.clone();
        Box::pin(async move {
            execute_tool(runtime.as_ref(), network_probe.as_ref(), &tool_name, &body).await
        })
    }
}

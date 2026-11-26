//! The system service: its status, installation and mode.
use crate::registry::RpcState;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(query, expose = [http])]
pub fn get_service_status_summary(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_runtime_values::observation::ServiceStatusSummary> {
    let status = client
        .service_actor_status()
        .ok_or_else(|| anyhow::anyhow!("ServiceActor is not connected"))?;
    Ok((&status).into())
}

#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub async fn status_service(
    client: RpcState<'_, ChimeraClient>,
) -> anyhow::Result<chimera_runtime_values::core_status::ServiceHostStatus> {
    client
        .service_actor_status()
        .ok_or_else(|| anyhow::anyhow!("ServiceActor is not connected"))
}

/// Format the install command for the current host without executing it.
#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub async fn get_service_install_prompt(
    prompt: RpcState<'_, dyn crate::ports::ServiceInstallPrompt>,
    paths: RpcState<'_, chimera_paths::PathResolver>,
) -> anyhow::Result<String> {
    prompt.service_install_prompt(paths).await
}

//! Shared declarations from the pinned ref logs.app.
use crate::registry::RpcCaller;
use crate::registry::RpcState;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(expose = [http, local], result, owner, operation = chimera_rpc_protocol::operations::CloseLogSession, access = read)]
pub async fn close_log_session(
    window: RpcCaller,
    client: RpcState<'_, ChimeraClient>,
    source: chimera_contracts::logs::LogSource,
    session: String,
) -> chimera_logging::LogResult<()> {
    client
        .file_logs()?
        .close_log_session(source, window.label().to_string(), session)
        .await
}

#[chimera_rpc::rpc(expose = [http, local], result, operation = chimera_rpc_protocol::operations::ListLogFiles, query)]
pub async fn list_log_files(
    client: RpcState<'_, ChimeraClient>,
    source: chimera_contracts::logs::LogSource,
) -> chimera_logging::LogResult<Vec<chimera_logging::LogFileInfo>> {
    client.file_logs()?.list_log_files(source).await
}

#[chimera_rpc::rpc(expose = [http, local], result, owner, operation = chimera_rpc_protocol::operations::OpenLogSession, access = read)]
pub async fn open_log_session(
    window: RpcCaller,
    client: RpcState<'_, ChimeraClient>,
    source: chimera_contracts::logs::LogSource,
    request: chimera_logging::OpenLogs,
) -> chimera_logging::LogResult<chimera_logging::LogSession> {
    client
        .file_logs()?
        .open_log_session(source, window.label().to_string(), request)
        .await
}

#[chimera_rpc::rpc(expose = [http, local], result, owner, operation = chimera_rpc_protocol::operations::QueryLogs, access = read, sensitive)]
pub async fn query_logs(
    window: RpcCaller,
    client: RpcState<'_, ChimeraClient>,
    source: chimera_contracts::logs::LogSource,
    request: chimera_logging::QueryLogs,
) -> chimera_logging::LogResult<chimera_logging::LogPage> {
    client
        .file_logs()?
        .query_logs(source, window.label().to_string(), request)
        .await
}

#[chimera_rpc::rpc(result, owner, access = read, expose = [http])]
pub async fn query_logs_summary(
    window: RpcCaller,
    client: RpcState<'_, ChimeraClient>,
    source: chimera_contracts::logs::LogSource,
    request: chimera_core::client::logs::QueryLogSummary,
) -> chimera_logging::LogResult<chimera_core::client::logs::LogPageSummary> {
    client
        .file_logs()?
        .query_logs_summary(source, window.label().to_string(), request)
        .await
}

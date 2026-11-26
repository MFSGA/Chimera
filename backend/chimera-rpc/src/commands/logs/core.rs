//! Shared declarations from the pinned ref logs.core.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_contracts::logs::{CoreLogCursor, CoreLogStatus, CoreLogStatusSummary};
use chimera_core::client::ChimeraClient;
use chimera_core::logs::query::{
    CoreLogError, CoreLogPage, CoreLogPageSummary, CoreLogQuery, CoreLogReader, CoreLogRecord,
    CoreLogResult, CoreLogSummaryQuery,
};

#[chimera_rpc::rpc(result, expose = [http])]
pub async fn clear_core_logs(client: RpcState<'_, ChimeraClient>) -> CoreLogResult<()> {
    client
        .streams()
        .clear_history(chimera_core::clash::ws::ClashWsKind::Logs)
        .await
        .map_err(|_| CoreLogError::Unavailable("core log storage unavailable".into()))
}

#[chimera_rpc::rpc(result, query, expose = [http], sensitive)]
pub async fn get_core_log(
    client: RpcState<'_, ChimeraClient>,
    cursor: CoreLogCursor,
) -> CoreLogResult<CoreLogRecord> {
    CoreLogReader::new(client.streams().core_logs())
        .detail(cursor)
        .await
}

#[chimera_rpc::rpc(result, query, expose = [http], sensitive)]
pub async fn get_core_log_status(
    client: RpcState<'_, ChimeraClient>,
) -> CoreLogResult<CoreLogStatus> {
    CoreLogReader::new(client.streams().core_logs())
        .status()
        .await
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_core_log_status_summary(
    client: RpcState<'_, ChimeraClient>,
) -> Result<CoreLogStatusSummary> {
    Ok(CoreLogReader::new(client.streams().core_logs())
        .status_summary()
        .await?)
}

#[chimera_rpc::rpc(expose = [http, local], result, query, sensitive)]
pub async fn query_core_logs(
    client: RpcState<'_, ChimeraClient>,
    query: CoreLogQuery,
) -> CoreLogResult<CoreLogPage> {
    CoreLogReader::new(client.streams().core_logs())
        .query(query)
        .await
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn query_core_logs_summary(
    client: RpcState<'_, ChimeraClient>,
    query: CoreLogSummaryQuery,
) -> Result<CoreLogPageSummary> {
    Ok(CoreLogReader::new(client.streams().core_logs())
        .summary(query)
        .await?)
}

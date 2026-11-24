//! Ref log read surface over the one persistent observer.
use chimera_contracts::logs::{CoreLogCursor, CoreLogStatus, CoreLogStatusSummary};
use chimera_observers::logs::CoreLogsClient;
pub use chimera_observers::logs::{
    CoreLogDirection, CoreLogError, CoreLogPage, CoreLogPageSummary, CoreLogQuery, CoreLogRecord,
    CoreLogResult, CoreLogSummaryQuery,
};

pub fn read_error(error: CoreLogError) -> CoreLogError {
    match error {
        CoreLogError::Unavailable(_) => {
            CoreLogError::Unavailable("core log storage unavailable".into())
        }
        other => other,
    }
}

pub fn safe_status(mut status: CoreLogStatus) -> CoreLogStatus {
    if status.error.is_some() {
        status.error = Some("core log storage unavailable".into());
    }
    status
}

#[derive(Clone)]
pub struct CoreLogReader(CoreLogsClient);

impl CoreLogReader {
    pub fn new(client: &CoreLogsClient) -> Self {
        Self(client.clone())
    }

    pub async fn query(&self, query: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        let mut page = self.0.query(query).await.map_err(read_error)?;
        page.status = safe_status(page.status);
        Ok(page)
    }

    pub async fn summary(&self, query: CoreLogSummaryQuery) -> CoreLogResult<CoreLogPageSummary> {
        self.query(query.try_into()?)
            .await
            .map(CoreLogPageSummary::from)
    }

    pub async fn detail(&self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.0.detail(cursor).await.map_err(read_error)
    }

    pub async fn status(&self) -> CoreLogResult<CoreLogStatus> {
        self.0.status().await.map(safe_status).map_err(read_error)
    }

    pub async fn status_summary(&self) -> CoreLogResult<CoreLogStatusSummary> {
        self.status().await.map(CoreLogStatusSummary::from)
    }
}

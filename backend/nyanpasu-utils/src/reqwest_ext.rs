//! Retry transient Windows named-pipe contention before HTTP data is sent.

use std::{error::Error, future::Future, time::Duration};

use backon::{BackoffBuilder, ExponentialBuilder};
use reqwest::{RequestBuilder, Response};
use reqwest_websocket::Upgrade;

/// True only for a reqwest connection failure caused by ERROR_PIPE_BUSY.
/// Bare IO errors, response failures, and non-Windows errors are not replayable.
pub fn is_named_pipe_busy(error: &(dyn Error + 'static)) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(request) = error.downcast_ref::<reqwest::Error>() {
            if !request.is_connect() {
                return false;
            }
            let mut source = request.source();
            while let Some(error) = source {
                if let Some(io) = error.downcast_ref::<std::io::Error>() {
                    return io.raw_os_error() == Some(231);
                }
                source = error.source();
            }
            return false;
        }
        cause = error.source();
    }
    false
}

/// Request sends with a one-second exponential retry budget for pipe contention.
///
/// Only errors before establishing the connection can be retried. HTTP status
/// codes, response reads, and established WebSocket streams are never replayed.
/// Non-cloneable request bodies are attempted once. Delays start at 50ms, grow
/// to a 200ms base, and use jitter to avoid synchronized reconnects.
///
/// Use a client with automatic redirects disabled: a connection failure after
/// a redirect does not prove that the original operation was never executed.
///
/// The budget bounds retry scheduling, not an individual send. Reqwest timeouts
/// apply per attempt; wrap the whole future in a timeout for a total deadline.
/// Dropping the future cancels its send or wait without spawning background work.
pub trait NamedPipeRequestExt {
    fn send_with_named_pipe_retry(
        self,
    ) -> impl Future<Output = Result<Response, reqwest::Error>> + Send;

    fn upgrade_with_named_pipe_retry(
        self,
    ) -> impl Future<Output = Result<reqwest_websocket::UpgradeResponse, reqwest_websocket::Error>> + Send;
}

impl NamedPipeRequestExt for RequestBuilder {
    async fn send_with_named_pipe_retry(self) -> Result<Response, reqwest::Error> {
        retry_send(self, |request| request.send()).await
    }

    async fn upgrade_with_named_pipe_retry(
        self,
    ) -> Result<reqwest_websocket::UpgradeResponse, reqwest_websocket::Error> {
        retry_send(self, |request| request.upgrade().send()).await
    }
}

async fn retry_send<T, E, F, Fut>(mut request: RequestBuilder, send: F) -> Result<T, E>
where
    E: Error + 'static,
    F: Fn(RequestBuilder) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let started = tokio::time::Instant::now();
    let mut backoff = ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(50))
        .with_max_delay(Duration::from_millis(200))
        .with_jitter()
        .without_max_times()
        .build();
    loop {
        let next = request.try_clone();
        match send(request).await {
            Err(error) if is_named_pipe_busy(&error) => {
                let (Some(next), Some(delay)) = (next, backoff.next()) else {
                    return Err(error);
                };
                // Include time spent in failed sends, not only scheduled sleeps.
                if started.elapsed() + delay > Duration::from_secs(1) {
                    return Err(error);
                }
                tokio::time::sleep(delay).await;
                request = next;
            }
            result => return result,
        }
    }
}

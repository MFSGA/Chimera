//! Desktop icon URI adapter, following ref's unified RPC icon entry.
//! Origin and shutdown admission stay here; shared RPC owns the byte read.

use chimera_rpc::icons as transport;
use chimera_rpc_protocol::ResponseVisibility;
use tauri::{
    Manager as _,
    http::{HeaderValue, Method, Request, Response, StatusCode},
};
use tokio_util::{
    sync::CancellationToken,
    task::{TaskTracker, task_tracker::TaskTrackerToken},
};

use crate::{client::ChimeraClient, setup::StartupOwners};

pub(crate) fn handle(
    context: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let app = context.app_handle();
    let (Some(client), Some(owners)) = (
        app.try_state::<ChimeraClient>(),
        app.try_state::<StartupOwners>(),
    ) else {
        responder.respond(unavailable());
        return;
    };
    let Some(admission) = admit(&owners.tasks, &owners.shutdown) else {
        responder.respond(unavailable());
        return;
    };
    let Some(window) = app.get_webview_window(context.webview_label()) else {
        responder.respond(error_response(StatusCode::FORBIDDEN, "forbidden"));
        return;
    };
    let client = client.inner().clone();
    let dev_origin = if tauri::is_dev() {
        app.config()
            .build
            .dev_url
            .as_ref()
            .map(|url| url.origin().ascii_serialization())
    } else {
        None
    };
    owners.tasks.spawn_on(
        async move {
            // A webview that navigated outside the app must not gain a network proxy,
            // including image requests without an Origin header.
            let trusted = window
                .url()
                .is_ok_and(|url| trusted_page(&url, dev_origin.as_deref()));
            let result = if trusted {
                response(&client, request, dev_origin.as_deref()).await
            } else {
                error_response(StatusCode::FORBIDDEN, "forbidden")
            };
            responder.respond(result);
        },
        tauri::async_runtime::handle().inner(),
    );
    drop(admission);
}

fn trusted_page(url: &url::Url, dev_origin: Option<&str>) -> bool {
    let packaged = if cfg!(windows) {
        matches!(
            url.origin().ascii_serialization().as_str(),
            "http://tauri.localhost" | "https://tauri.localhost"
        )
    } else {
        url.scheme() == "tauri" && url.host_str() == Some("localhost") && url.port().is_none()
    };
    packaged || dev_origin == Some(url.origin().ascii_serialization().as_str())
}

fn admit(tasks: &TaskTracker, shutdown: &CancellationToken) -> Option<TaskTrackerToken> {
    let token = tasks.token();
    (!shutdown.is_cancelled()).then_some(token)
}

fn unavailable() -> Response<Vec<u8>> {
    error_response(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

fn cors_origin(
    request: &Request<Vec<u8>>,
    dev_origin: Option<&str>,
) -> Result<Option<HeaderValue>, ()> {
    let mut origins = request.headers().get_all("origin").iter();
    let Some(origin) = origins.next() else {
        return Ok(None);
    };
    let value = origin.to_str().map_err(|_| ())?;
    let packaged = if cfg!(windows) {
        matches!(value, "http://tauri.localhost" | "https://tauri.localhost")
    } else {
        value == "tauri://localhost"
    };
    if origins.next().is_some() || !(packaged || dev_origin == Some(value)) {
        return Err(());
    }
    Ok(Some(origin.clone()))
}

async fn response(
    client: &ChimeraClient,
    request: Request<Vec<u8>>,
    dev_origin: Option<&str>,
) -> Response<Vec<u8>> {
    let origin = match cors_origin(&request, dev_origin) {
        Ok(origin) => origin,
        Err(()) => return error_response(StatusCode::FORBIDDEN, "forbidden"),
    };
    let mut result = if request.method() != Method::GET {
        error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
    } else {
        match transport::parse(request.uri().path(), request.uri().query()) {
            Ok(request) => match client.rpc() {
                Ok(rpc) => {
                    match transport::read(&rpc.dependencies(), ResponseVisibility::Public, request)
                        .await
                    {
                        Ok(body) => image_response(body),
                        Err(error) if error.kind == "invalid_icon" => {
                            error_response(StatusCode::UNPROCESSABLE_ENTITY, "invalid_icon")
                        }
                        Err(error) if error.kind == "unavailable" => unavailable(),
                        Err(_) => {
                            error_response(StatusCode::INTERNAL_SERVER_ERROR, "icon_fetch_failed")
                        }
                    }
                }
                Err(_) => unavailable(),
            },
            Err(_) => error_response(StatusCode::BAD_REQUEST, "invalid_params"),
        }
    };
    result
        .headers_mut()
        .insert("vary", HeaderValue::from_static("Origin"));
    if let Some(origin) = origin {
        result
            .headers_mut()
            .insert("access-control-allow-origin", origin);
    }
    result
}

fn image_response(body: transport::IconBody) -> Response<Vec<u8>> {
    let mut response = Response::new(body.bytes);
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static(body.content_type));
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static(body.cache_policy.header()),
    );
    secure(response)
}

fn error_response(status: StatusCode, kind: &'static str) -> Response<Vec<u8>> {
    let mut response = Response::new(
        serde_json::to_vec(&serde_json::json!({ "kind": kind })).expect("icon errors serialize"),
    );
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    secure(response)
}

fn secure(mut response: Response<Vec<u8>>) -> Response<Vec<u8>> {
    for (name, value) in transport::SECURITY_HEADERS {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future as _;

    #[tokio::test]
    async fn desktop_icon_requests_join_shutdown_and_refuse_late_admission() {
        let tasks = TaskTracker::new();
        let shutdown = CancellationToken::new();
        let accepted = admit(&tasks, &shutdown).unwrap();
        shutdown.cancel();
        tasks.close();
        assert!(admit(&tasks, &shutdown).is_none());
        assert!(
            std::pin::pin!(tasks.wait())
                .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
                .is_pending()
        );
        drop(accepted);
        tasks.wait().await;
    }

    #[test]
    fn denies_untrusted_webview_and_origin() {
        assert!(!trusted_page(
            &url::Url::parse("https://other.example/").unwrap(),
            None
        ));
        assert!(trusted_page(
            &url::Url::parse("http://localhost:3000/main").unwrap(),
            Some("http://localhost:3000")
        ));
        let request = Request::get("chimera-icons://localhost/cached")
            .header("origin", "https://other.example")
            .body(vec![])
            .unwrap();
        assert!(cors_origin(&request, None).is_err());
        let response = error_response(StatusCode::FORBIDDEN, "forbidden");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}

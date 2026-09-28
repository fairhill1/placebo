//! Action request correlation. No request bodies or query strings are logged.
use axum::{
    extract::{Request, State},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};

const REQUEST_ID: &str = "x-placebo-request-id";
/// Marks responses produced through an action's typed route adapter, so the
/// browser can report a handler registered with a plain Axum route instead.
const ACTION: &str = "x-placebo-action";

pub(crate) async fn request(
    State(action): State<&'static str>,
    mut request: Request,
    next: Next,
) -> Response {
    let id = request
        .headers()
        .get(REQUEST_ID)
        .filter(|value| {
            let bytes = value.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 64
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        })
        .cloned();
    if id.is_none() {
        request.headers_mut().remove(REQUEST_ID);
    }
    #[cfg(debug_assertions)]
    let (method, path, start) = (
        request.method().clone(),
        request.uri().path().to_owned(),
        std::time::Instant::now(),
    );
    #[cfg(debug_assertions)]
    eprintln!(
        "[placebo:request] action={action} request={} {method} {path}",
        id.as_ref()
            .and_then(|id| id.to_str().ok())
            .unwrap_or("none")
    );
    let mut response = next.run(request).await;
    #[cfg(debug_assertions)]
    eprintln!(
        "[placebo:response] action={action} request={} {method} {path} HTTP {} elapsed={}ms",
        id.as_ref()
            .and_then(|id| id.to_str().ok())
            .unwrap_or("none"),
        response.status().as_u16(),
        start.elapsed().as_millis()
    );
    // Echo the bounded client correlation token, including extractor rejections.
    // This is an identifier for logging, never an authorization credential.
    if let Some(id) = id {
        response.headers_mut().insert(REQUEST_ID, id);
    }
    response
        .headers_mut()
        .insert(ACTION, HeaderValue::from_static(action));
    response
}

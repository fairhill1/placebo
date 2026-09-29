//! Form submissions without JavaScript. A mutation form is an ordinary HTML
//! form, so a browser can post it before the runtime loads, or without it.
//! The route adapter runs the same handler and turns its reply into what a
//! browser navigation expects: a redirect after a successful save, or a full
//! page showing the rejected component with the reply's contents.
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use std::{any::TypeId, cell::RefCell, collections::HashMap};
use tower::ServiceExt;

tokio::task_local! {
    /// Set by the mutation adapter while a native submission's handler runs.
    static SUBMISSION: RefCell<Submission>;
    /// Set by [`native_forms`] while it renders the page again.
    static PAGE: RefCell<PageOverride>;
}

#[derive(Default)]
pub(crate) struct Submission {
    /// The page the form was on: its `placebo-page` field, else the Referer.
    back: Option<String>,
    input: Option<TypeId>,
    /// The fields the person changed, by wire name: the submitted values and
    /// the rendered values' fingerprint the form carried.
    edited: HashMap<String, Edit>,
    focused_invalid: bool,
}

struct PageOverride {
    path: String,
    target: String,
    html: String,
    used: bool,
}

/// A rejected reply to a native submission, waiting for its page.
#[derive(Clone)]
struct NativePage {
    target: String,
    html: String,
    path: String,
}

/// Marks requests that pass through [`native_forms`].
#[derive(Clone)]
struct PagesEnabled;

/// What an update envelope needs to become a native response.
#[derive(Clone)]
pub(crate) struct NativeReply {
    pub target: String,
    pub html: String,
    pub navigate: Option<String>,
}

/// Run a native submission's handler and turn its reply into a navigation.
pub(crate) async fn submit(request: Request, next: Next) -> Response {
    let pages = request.extensions().get::<PagesEnabled>().is_some();
    let submission = Submission {
        back: same_origin_referer(request.headers()),
        ..Submission::default()
    };
    let (response, submission) = SUBMISSION
        .scope(RefCell::new(submission), async {
            let response = next.run(request).await;
            (response, SUBMISSION.with(RefCell::take))
        })
        .await;
    respond(submission.back, pages, response)
}

/// A field a native submission changed from the values its form was rendered with.
#[derive(Clone)]
pub(crate) struct Edit {
    pub values: Vec<String>,
    pub base: String,
}

/// Record the submitted page and which fields the person edited, for the
/// replies this handler renders.
pub(crate) fn record_submission<I: 'static>(page: Option<&str>, edited: HashMap<String, Edit>) {
    let _ = SUBMISSION.try_with(|submission| {
        let mut submission = submission.borrow_mut();
        if let Some(page) = page.filter(|page| local_path(page)) {
            submission.back = Some(page.to_owned());
        }
        submission.input = Some(TypeId::of::<I>());
        submission.edited = edited;
    });
}

/// The page a form rendered now is on, when a native submission or a page
/// rendered again for one moved the address to the action's path. Forms carry
/// it in `placebo-page`, so the next native submission from that page can
/// return to it.
pub(crate) fn current_page() -> Option<String> {
    PAGE.try_with(|page| page.borrow().path.clone())
        .ok()
        .or_else(|| {
            SUBMISSION
                .try_with(|submission| submission.borrow().back.clone())
                .ok()
                .flatten()
        })
}

fn local_path(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("//") && !path.contains('\\')
}

/// The submitted values for a field the person edited, while a native
/// submission's handler renders a form for the same payload type. The form
/// keeps the original rendered fingerprint, so the field stays edited, as a
/// control the runtime keeps keeps its original defaults.
pub(crate) fn resubmitted<I: 'static>(name: &str) -> Option<Edit> {
    SUBMISSION
        .try_with(|submission| {
            let submission = submission.borrow();
            (submission.input == Some(TypeId::of::<I>()))
                .then(|| submission.edited.get(name).cloned())
                .flatten()
        })
        .ok()
        .flatten()
}

/// Natively, the first invalid control of a rejected reply takes focus.
pub(crate) fn autofocus_invalid() -> bool {
    SUBMISSION
        .try_with(|submission| {
            let mut submission = submission.borrow_mut();
            !std::mem::replace(&mut submission.focused_invalid, true)
        })
        .unwrap_or(false)
}

/// While a rejected page renders again, the rejected component shows the
/// reply's contents instead of the contents the page handler rendered.
pub(crate) fn mounted_contents(id: &str) -> Option<Markup> {
    PAGE.try_with(|page| {
        let mut page = page.borrow_mut();
        (page.target == id && !page.used).then(|| {
            page.used = true;
            PreEscaped(page.html.clone())
        })
    })
    .ok()
    .flatten()
}

/// Turn a handler's response to a native submission into a navigation.
fn respond(back: Option<String>, pages: bool, response: Response) -> Response {
    let Some(reply) = response.extensions().get::<NativeReply>().cloned() else {
        return response;
    };
    let status = response.status();
    if status.is_success() {
        let location = reply.navigate.or(back).unwrap_or_else(|| {
            #[cfg(debug_assertions)]
            eprintln!(
                "[placebo:native-no-referer] A form submitted without JavaScript was saved, but the \
                 browser sent no same-origin Referer, so it returns to '/'. Keep the default \
                 Referrer-Policy (strict-origin-when-cross-origin) or reply with .navigate(path)."
            );
            "/".to_owned()
        });
        return see_other(&location);
    }
    let path = match back {
        Some(path) => path,
        None => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[placebo:native-no-referer] A rejected form submitted without JavaScript cannot \
                 render its page again: the browser sent no same-origin Referer. It gets a page \
                 with only the component."
            );
            return fallback_page(status, &reply);
        }
    };
    if !pages {
        #[cfg(debug_assertions)]
        eprintln!(
            "[placebo:native-page] A rejected form submitted without JavaScript gets a page with \
             only component '{}'. Wrap the finished router with placebo::native_forms(app) to \
             render its whole page again.",
            reply.target
        );
    }
    let mut response = fallback_page(status, &reply);
    response.extensions_mut().insert(NativePage {
        target: reply.target,
        html: reply.html,
        path,
    });
    response
}

fn see_other(location: &str) -> Response {
    match HeaderValue::try_from(location) {
        Ok(location) => (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, location),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The path and query of a same-origin Referer. Only the path is used, so a
/// forged Referer cannot redirect anywhere else.
fn same_origin_referer(headers: &HeaderMap) -> Option<String> {
    let referer: Uri = headers.get(header::REFERER)?.to_str().ok()?.parse().ok()?;
    let host = headers.get(header::HOST)?.to_str().ok()?;
    if referer.authority()?.as_str() != host {
        return None;
    }
    let path = referer.path_and_query()?.as_str();
    local_path(path).then(|| path.to_owned())
}

/// Without the whole page, a rejected native submission still shows the
/// component with the reply's contents: the form, the person's values, and
/// the feedback.
fn fallback_page(status: StatusCode, reply: &NativeReply) -> Response {
    let page = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Check your changes" }
            }
            body style="font: 1.1rem/1.5 system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem" {
                main {
                    div id=(reply.target) data-placebo-region data-placebo-component {
                        (PreEscaped(&reply.html))
                    }
                }
            }
        }
    };
    (status, [(header::CACHE_CONTROL, "no-store")], page).into_response()
}

/// Render the whole page again when a form submitted without JavaScript is
/// rejected. Wrap the finished router:
///
/// ```
/// use axum::{Router, routing::get};
/// let app: Router = Router::new().route("/", get(|| async { "Home" }));
/// let app = placebo::native_forms(app);
/// ```
///
/// A save that succeeds redirects back to the page it came from (or to its
/// `navigate` path). An `invalid` or `conflict` reply renders that page again,
/// as a GET with the same cookies, and the rejected component shows the
/// reply's contents, with the reply's HTTP status. Without this wrapper, a
/// rejected native submission gets a page with only the component.
pub fn native_forms(app: Router) -> Router {
    Router::new()
        .fallback_service(app.clone())
        .layer(axum::middleware::from_fn_with_state(app, pages))
}

async fn pages(State(app): State<Router>, mut request: Request, next: Next) -> Response {
    let headers = request.headers().clone();
    request.extensions_mut().insert(PagesEnabled);
    let response = next.run(request).await;
    let Some(page) = response.extensions().get::<NativePage>().cloned() else {
        return response;
    };
    let mut get = Request::new(Body::empty());
    *get.uri_mut() = match page.path.parse() {
        Ok(uri) => uri,
        Err(_) => return response,
    };
    for (name, value) in &headers {
        let name_str = name.as_str();
        let skip = matches!(
            name_str,
            "content-type" | "content-length" | "transfer-encoding" | "origin" | "accept"
        ) || name_str.starts_with("x-placebo-")
            || name_str.starts_with("sec-fetch-");
        if !skip {
            get.headers_mut().append(name, value.clone());
        }
    }
    get.headers_mut()
        .insert(header::ACCEPT, HeaderValue::from_static("text/html"));
    let state = RefCell::new(PageOverride {
        path: page.path.clone(),
        target: page.target.clone(),
        html: page.html,
        used: false,
    });
    let (rendered, used) = PAGE
        .scope(state, async {
            let rendered = app.oneshot(get).await.into_response();
            (rendered, PAGE.with(|page| page.borrow().used))
        })
        .await;
    let html = rendered
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    if !used || rendered.status() != StatusCode::OK || !html {
        #[cfg(debug_assertions)]
        eprintln!(
            "[placebo:native-page] Rendering '{}' again did not mount component '{}' (HTTP {}), \
             so the rejected reply gets a page with only the component. Mount the component on \
             the page its form is on.",
            page.path,
            page.target,
            rendered.status().as_u16()
        );
        return response;
    }
    let (mut parts, body) = rendered.into_parts();
    parts.status = response.status();
    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Response::from_parts(parts, body)
}

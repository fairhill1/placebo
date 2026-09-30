//! Every mutation is answered with a page. A mutation form is an ordinary HTML
//! form, so a browser can post it before the runtime loads, or without it.
//! The route adapter runs the same handler and turns its reply into a page:
//!
//! - From the runtime, the page the form was on, rendered again with the
//!   submitted component showing the reply's contents. The runtime morphs it
//!   into the document, so everything else on the page shows current data
//!   without the handler knowing what else is there.
//! - Without JavaScript, a redirect after a successful save, or that same page
//!   for a rejected one.
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, ready},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

use crate::replay::{self, Recorded, ReplayStore};

tokio::task_local! {
    /// Set by the mutation adapter while a mutation's handler runs.
    static SUBMISSION: RefCell<Submission>;
    /// Set by [`native_forms`] while it renders the page again.
    static PAGE: RefCell<PageOverride>;
}

#[derive(Default)]
pub(crate) struct Submission {
    /// Submitted by the browser itself, without the runtime.
    native: bool,
    /// The replay id this submission claimed, to record its reply under.
    claimed: Option<(String, Arc<dyn ReplayStore>)>,
    /// The page the form was on: its `placebo-page` field, else the Referer.
    back: Option<String>,
    input: Option<TypeId>,
    /// The component the submitted form was bound to.
    target: Option<String>,
    /// The fields the person changed, by wire name: the submitted values and
    /// the rendered values' fingerprint the form carried.
    edited: HashMap<String, Edit>,
    focused_invalid: bool,
    /// The runtime's id for this request, for the feed signals it causes.
    request: Option<String>,
}

struct PageOverride {
    path: String,
    target: String,
    html: String,
    used: bool,
    native: bool,
}

/// A reply waiting for its page: any reply to the runtime, or a rejected
/// reply to a native submission.
#[derive(Clone)]
struct NativePage {
    target: String,
    html: String,
    path: String,
    native: bool,
}

/// Response headers of a page reply to the runtime.
const OUTCOME: &str = "x-placebo-outcome";
const NAVIGATE: &str = "x-placebo-navigate";
/// The target component the rendered page does not mount.
const UNMOUNTED: &str = "x-placebo-unmounted";
/// A reply to the runtime without its page shows in its component alone,
/// and one of these says why: a setup error, or a page that did not render
/// (such as a record's page after the record was deleted).
const PAGE_ERROR: &str = "x-placebo-page-error";
const PAGE_MISSING: &str = "x-placebo-page-missing";
/// When the reply's page began rendering; see [`render_stamp`].
const RENDERED: &str = "x-placebo-rendered";
/// The page the runtime submitted from: its path and query.
const PAGE_HEADER: &str = "x-placebo-page";
/// Marks the runtime reading a page: at a read form's query, or again after a
/// feed's signal or a poll.
const REFRESH: &str = "x-placebo-refresh";
const REQUEST_ID: &str = "x-placebo-request-id";

/// Marks requests that pass through [`native_forms`].
#[derive(Clone)]
struct PagesEnabled;

/// A mutation handler's reply: the component, its contents, and where to go
/// instead of the page. Replays keep it as JSON.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeReply {
    pub target: String,
    pub html: String,
    pub navigate: Option<String>,
}

/// Run a mutation's handler, record its reply for replays, and turn the reply
/// into a page, or a navigation for a native submission.
pub(crate) async fn run(native: bool, request: Request, next: Next) -> Response {
    let pages = request.extensions().get::<PagesEnabled>().is_some();
    let back = if native {
        same_origin_referer(request.headers())
    } else {
        request
            .headers()
            .get(PAGE_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|path| local_path(path))
            .map(str::to_owned)
            .or_else(|| same_origin_referer(request.headers()))
    };
    let request_id = request
        .headers()
        .get(REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .map(str::to_owned);
    let submission = Submission {
        native,
        back,
        request: request_id,
        ..Submission::default()
    };
    let work = async move {
        let (response, submission) = SUBMISSION
            .scope(RefCell::new(submission), async {
                let response = next.run(request).await;
                (response, SUBMISSION.with(RefCell::take))
            })
            .await;
        let response = match submission.claimed {
            Some((id, store)) => record(&*store, &id, response).await,
            None => response,
        };
        (response, submission.back)
    };
    let (response, back) = Finish(Some(Box::pin(work))).await;
    if native {
        respond(back, pages, response)
    } else {
        respond_page(back, pages, response)
    }
}

/// Turn a handler's reply to the runtime into its page: the page it was
/// submitted from, rendered again by [`native_forms`] with the component
/// showing the reply. A successful reply that navigates only says where to.
fn respond_page(back: Option<String>, pages: bool, response: Response) -> Response {
    let Some(reply) = response.extensions().get::<NativeReply>().cloned() else {
        return response;
    };
    let status = response.status();
    let outcome = match status {
        StatusCode::UNPROCESSABLE_ENTITY => "invalid",
        StatusCode::CONFLICT => "conflict",
        _ => "applied",
    };
    let mut page = if let (true, Some(path)) = (status.is_success(), &reply.navigate) {
        let Ok(path) = HeaderValue::try_from(path.as_str()) else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let mut page = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        )
            .into_response();
        page.headers_mut().insert(NAVIGATE, path);
        page
    } else {
        match (back, pages) {
            (Some(path), true) => {
                let mut page = status.into_response();
                page.extensions_mut().insert(NativePage {
                    target: reply.target,
                    html: reply.html,
                    path,
                    native: false,
                });
                page
            }
            (back, _) => {
                let problem = if back.is_none() {
                    "the request did not say which page it came from (no X-Placebo-Page or \
                     same-origin Referer)"
                } else {
                    "the router is not wrapped with placebo::native_forms(app), which renders it"
                };
                eprintln!(
                    "[placebo:page-error] The reply to component '{}' needs its page, but {problem}. \
                     It shows in the component alone.",
                    reply.target
                );
                component_only(status, PAGE_ERROR, problem, reply.html)
            }
        }
    };
    let headers = page.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(OUTCOME, HeaderValue::from_static(outcome));
    carry_headers(response.headers(), page.headers_mut());
    page
}

/// A reply to the runtime without its page: the body is the reply's contents,
/// which the runtime shows in the component, and `why` names the reason.
fn component_only(status: StatusCode, why: &'static str, problem: &str, html: String) -> Response {
    let mut response = (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response();
    if let Ok(value) = HeaderValue::try_from(problem) {
        response.headers_mut().insert(why, value);
    }
    response
}

/// Runs a mutation to its end. The handler runs in the request's task while
/// the browser waits. If the request is dropped, for example because the
/// browser disconnected, the rest runs on a task of its own: a handler
/// stopped halfway could have written without recording its reply, leaving
/// its retry with an unknown outcome.
struct Finish<F: Future + Send + 'static>(Option<Pin<Box<F>>>);

impl<F: Future + Send + 'static> Future for Finish<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let work = self
            .0
            .as_mut()
            .expect("a mutation polled after it finished");
        let output = ready!(work.as_mut().poll(cx));
        self.0 = None;
        Poll::Ready(output)
    }
}

impl<F: Future + Send + 'static> Drop for Finish<F> {
    fn drop(&mut self) {
        // A handler that panicked cannot be resumed.
        if let Some(work) = self.0.take()
            && !std::thread::panicking()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                work.await;
            });
        }
    }
}

/// Keep a reply for replays of the claimed submission. Anything else, such as
/// an error status, releases the claim so a retry runs the handler again.
async fn record(store: &dyn ReplayStore, id: &str, response: Response) -> Response {
    let Some(reply) = response.extensions().get::<NativeReply>() else {
        if let Err(error) = store.release(id).await {
            eprintln!("[placebo:replay-store] Could not release a submission: {error}");
        }
        return response;
    };
    let recorded = Recorded {
        status: response.status().as_u16(),
        headers: replay::recorded_headers(response.headers()),
        body: serde_json::to_string(reply).expect("a reply serializes"),
    };
    if let Err(error) = store.record(id, recorded).await {
        // The write happened; a retry will be told its outcome is unknown.
        eprintln!("[placebo:replay-store] Could not record a reply: {error}");
    }
    response
}

/// The runtime's id for the mutation whose handler is running, if any.
pub(crate) fn current_request() -> Option<String> {
    SUBMISSION
        .try_with(|submission| submission.borrow().request.clone())
        .ok()
        .flatten()
}

/// Claim this submission's replay id before its handler runs. `retry` is the
/// age of the first attempt when the runtime retries one. `Err` is the
/// response to send instead: a recorded reply, or an explanation.
pub(crate) async fn claim(
    store: Arc<dyn ReplayStore>,
    id: String,
    retry: Option<Duration>,
) -> Result<(), Box<Response>> {
    if SUBMISSION.try_with(|_| ()).is_err() {
        return Ok(());
    }
    replay::claim(&*store, &id, retry).await?;
    SUBMISSION.with(|submission| submission.borrow_mut().claimed = Some((id, store)));
    Ok(())
}

/// A field a native submission changed from the values its form was rendered with.
#[derive(Clone)]
pub(crate) struct Edit {
    pub values: Vec<String>,
    pub base: String,
}

/// Record the submitted page, the form's component, and which fields the
/// person edited, for the replies this handler renders.
pub(crate) fn record_submission<I: 'static>(
    page: Option<&str>,
    target: Option<&str>,
    edited: HashMap<String, Edit>,
) {
    let _ = SUBMISSION.try_with(|submission| {
        let mut submission = submission.borrow_mut();
        if !submission.native {
            return;
        }
        if let Some(page) = page.filter(|page| local_path(page)) {
            submission.back = Some(page.to_owned());
        }
        submission.input = Some(TypeId::of::<I>());
        submission.target = target.map(str::to_owned);
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
/// shows them only if it is the one submitted (see [`submitted_from`]), and
/// keeps the original rendered fingerprint, so the field stays edited, as the
/// runtime keeps an edited control's original defaults.
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

/// Whether a form bound to `target` is the one a native submission came
/// from, so it shows the submitted values. Other forms of the same payload
/// type show what the handler rendered.
pub(crate) fn submitted_from(target: &str) -> bool {
    SUBMISSION
        .try_with(|submission| submission.borrow().target.as_deref() == Some(target))
        .unwrap_or(false)
}

/// Natively, the first invalid control of a rejected reply takes focus.
pub(crate) fn autofocus_invalid() -> bool {
    SUBMISSION
        .try_with(|submission| {
            let mut submission = submission.borrow_mut();
            submission.native && !std::mem::replace(&mut submission.focused_invalid, true)
        })
        .unwrap_or(false)
}

/// While a page renders for a reply, the replying component shows the
/// reply's contents instead of the contents the page handler rendered. The
/// flag is set for a page rendered again for a native submission.
pub(crate) fn mounted_contents(id: &str) -> Option<(Markup, bool)> {
    PAGE.try_with(|page| {
        let mut page = page.borrow_mut();
        (page.target == id && !page.used).then(|| {
            page.used = true;
            (PreEscaped(page.html.clone()), page.native)
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
            eprintln!(
                "[placebo:native-no-referer] A form submitted without JavaScript was saved, but the \
                 browser sent no same-origin Referer, so it returns to '/'. Keep the default \
                 Referrer-Policy (strict-origin-when-cross-origin) or reply with .navigate(path)."
            );
            "/".to_owned()
        });
        let mut redirect = see_other(&location);
        carry_headers(response.headers(), redirect.headers_mut());
        return redirect;
    }
    let path = match back {
        Some(path) => path,
        None => {
            eprintln!(
                "[placebo:native-no-referer] A rejected form submitted without JavaScript cannot \
                 render its page again: the browser sent no same-origin Referer. It gets a page \
                 with only the component."
            );
            let mut page = fallback_page(status, &reply);
            carry_headers(response.headers(), page.headers_mut());
            return page;
        }
    };
    if !pages {
        eprintln!(
            "[placebo:native-page] A rejected form submitted without JavaScript gets a page with \
             only component '{}'. Wrap the finished router with placebo::native_forms(app) to \
             render its whole page again.",
            reply.target
        );
    }
    let mut page = fallback_page(status, &reply);
    carry_headers(response.headers(), page.headers_mut());
    page.extensions_mut().insert(NativePage {
        target: reply.target,
        html: reply.html,
        path,
        native: true,
    });
    page
}

/// Keep the headers a handler added, such as `Set-Cookie` after a login, when
/// its reply becomes a redirect or a page. The body's own headers stay behind.
/// A header the page already has keeps the page's value, except cookies,
/// which both may set.
fn carry_headers(from: &HeaderMap, to: &mut HeaderMap) {
    for name in from.keys() {
        let body_header = matches!(
            *name,
            header::CONTENT_TYPE
                | header::CONTENT_LENGTH
                | header::CONTENT_ENCODING
                | header::TRANSFER_ENCODING
                | header::CACHE_CONTROL
                | header::LOCATION
        );
        if body_header || (name != header::SET_COOKIE && to.contains_key(name)) {
            continue;
        }
        for value in from.get_all(name) {
            to.append(name.clone(), value.clone());
        }
    }
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
                    div id=(reply.target) data-placebo-component {
                        (PreEscaped(&reply.html))
                    }
                }
            }
        }
    };
    (status, [(header::CACHE_CONTROL, "no-store")], page).into_response()
}

/// Render the page a reply belongs to. Wrap the finished router:
///
/// ```
/// use axum::{Router, routing::get};
/// let app: Router = Router::new().route("/", get(|| async { "Home" }));
/// let app = placebo::native_forms(app);
/// ```
///
/// Every reply to the runtime renders the page its form was on again, as a
/// GET with the same cookies (plus any the handler set), and the replying
/// component shows the reply's contents, with the reply's HTTP status. Without
/// JavaScript, a save that succeeds redirects back to the page it came from
/// (or to its `navigate` path), and an `invalid` or `conflict` reply renders
/// that page the same way. Without this wrapper, a reply shows only in its
/// component, the rest of the page goes out of date, and the runtime reports
/// `[placebo:page-error]`.
pub fn native_forms(app: Router) -> Router {
    Router::new()
        .fallback_service(app.clone())
        .layer(axum::middleware::from_fn_with_state(app, pages))
}

async fn pages(State(app): State<Router>, mut request: Request, next: Next) -> Response {
    // A page the runtime reads (a search, a refresh) is ordered like a
    // reply's page.
    if request.headers().contains_key(REFRESH) {
        let stamp = render_stamp();
        let mut response = next.run(request).await;
        response
            .headers_mut()
            .insert(RENDERED, HeaderValue::from(stamp));
        return response;
    }
    let headers = request.headers().clone();
    // What layers outside the app and the server attached, such as
    // `ConnectInfo` or a signed-in user, for the page's handler. The POST's
    // own routing is left behind; the app routes the GET afresh.
    let mut extensions = request.extensions().clone();
    extensions.remove::<axum::extract::OriginalUri>();
    extensions.remove::<axum::extract::MatchedPath>();
    request.extensions_mut().insert(PagesEnabled);
    let response = next.run(request).await;
    let Some(page) = response.extensions().get::<NativePage>().cloned() else {
        return response;
    };
    let mut get = Request::new(Body::empty());
    *get.extensions_mut() = extensions;
    *get.uri_mut() = match page.path.parse() {
        Ok(uri) => uri,
        Err(_) => return response,
    };
    for (name, value) in &headers {
        let name_str = name.as_str();
        let skip = matches!(
            name_str,
            "content-type"
                | "content-length"
                | "transfer-encoding"
                | "origin"
                | "accept"
                | "cookie"
        ) || name_str.starts_with("x-placebo-")
            || name_str.starts_with("sec-fetch-");
        if !skip {
            get.headers_mut().append(name, value.clone());
        }
    }
    if let Some(cookie) = cookies_after(&headers, response.headers()) {
        get.headers_mut().insert(header::COOKIE, cookie);
    }
    get.headers_mut()
        .insert(header::ACCEPT, HeaderValue::from_static("text/html"));
    let state = RefCell::new(PageOverride {
        path: page.path.clone(),
        target: page.target.clone(),
        html: page.html.clone(),
        used: false,
        native: page.native,
    });
    let stamp = render_stamp();
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
    if rendered.status() != StatusCode::OK || !html {
        let problem = format!(
            "rendering '{}' again answered HTTP {}{}",
            page.path,
            rendered.status().as_u16(),
            if html { "" } else { " without an HTML page" }
        );
        eprintln!(
            "[placebo:page-missing] The reply to component '{}' needs its page, but {problem}. \
             It shows in the component alone.",
            page.target
        );
        if page.native {
            return response;
        }
        let mut alone = component_only(response.status(), PAGE_MISSING, &problem, page.html);
        alone
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        carry_headers(response.headers(), alone.headers_mut());
        return alone;
    }
    if !used {
        eprintln!(
            "[placebo:page-unmounted] The page '{}' does not mount component '{}', so its reply \
             is not shown{}. Mount the component on the page its form is on.",
            page.path,
            page.target,
            if page.native {
                "; the native submission gets a page with only the component"
            } else {
                ""
            }
        );
        if page.native {
            return response;
        }
    }
    let (mut parts, body) = rendered.into_parts();
    parts.status = response.status();
    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if !used && let Ok(target) = HeaderValue::try_from(&page.target) {
        parts.headers.insert(UNMOUNTED, target);
    }
    if !page.native {
        parts.headers.insert(RENDERED, HeaderValue::from(stamp));
    }
    carry_headers(response.headers(), &mut parts.headers);
    Response::from_parts(parts, body)
}

/// When a page began rendering, in microseconds since the Unix epoch, and
/// never the same twice in this process. It is taken after the handler's
/// write, so a page with a later stamp shows every write whose page has an
/// earlier one: the runtime lets a later page replace an earlier one, and
/// never the reverse.
fn render_stamp() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_micros() as u64);
    let previous = LAST
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
            Some(now.max(last + 1))
        })
        .unwrap_or_else(|last| last);
    now.max(previous + 1)
}

/// The request's cookies with the ones the handler set applied, so a page
/// rendered for a reply shows what it changed, such as a preference or a new
/// session. A cookie set to expire (`Max-Age=0`) is left out.
fn cookies_after(request: &HeaderMap, response: &HeaderMap) -> Option<HeaderValue> {
    let mut cookies: Vec<(String, String)> = request
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            Some((name.to_owned(), value.to_owned()))
        })
        .collect();
    for set in response.get_all(header::SET_COOKIE) {
        let Some((pair, attributes)) = set.to_str().ok().map(|set| {
            let mut parts = set.splitn(2, ';');
            (
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            )
        }) else {
            continue;
        };
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        cookies.retain(|(existing, _)| existing != name);
        let expired = attributes.split(';').any(|attribute| {
            attribute
                .trim()
                .split_once('=')
                .is_some_and(|(key, value)| {
                    key.eq_ignore_ascii_case("max-age")
                        && value.trim().parse::<i64>().is_ok_and(|age| age <= 0)
                })
        });
        if !expired {
            cookies.push((name.to_owned(), value.to_owned()));
        }
    }
    let joined = cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    (!joined.is_empty())
        .then(|| HeaderValue::try_from(joined).ok())
        .flatten()
}

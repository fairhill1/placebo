//! A repeated mutation submission runs its handler once and replays the reply.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{Markup, html};
use placebo::{
    Component, Control, FormInput, Input, MutationAction, fields,
    replay::{Claim, MemoryReplays, Recorded, ReplayStore, StoreFuture},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Add {
    title: String,
}

const ADD: MutationAction<Add> = MutationAction::new("add", "/add");

#[derive(Clone, Default)]
struct Store {
    items: Arc<Mutex<Vec<String>>>,
    runs: Arc<AtomicUsize>,
}

fn composer(feedback: &str) -> Markup {
    let fields = fields! { Add {
        @field title = Control::text("").id("title");
        p #feedback role="status" { (feedback) }
    } };
    ADD.bind(&Component::new("composer", 1)).form(fields)
}

async fn home() -> Markup {
    html! { (Component::new("composer", 1).mount(composer(""))) }
}

async fn add(State(store): State<Store>, Input(input): Input<Add>) -> Response {
    store.runs.fetch_add(1, Ordering::SeqCst);
    let binding = ADD.bind(&Component::new("composer", 1));
    match input.title.as_str() {
        "x" => {
            return (
                [(SET_COOKIE, "rejected=1")],
                binding.invalid(composer("Too short.")),
            )
                .into_response();
        }
        "missing" => return StatusCode::NOT_FOUND.into_response(),
        "slow" => tokio::time::sleep(Duration::from_millis(150)).await,
        _ => {}
    }
    store.items.lock().unwrap().push(input.title);
    ([(SET_COOKIE, "added=1")], binding.reply(composer("Added."))).into_response()
}

fn app(store: Store) -> Router {
    Router::new()
        .route("/", get(home))
        .route(ADD.path(), ADD.route(add))
        .with_state(store)
}

async fn rendered_key(app: &Router) -> String {
    let page = app
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let page =
        String::from_utf8(to_bytes(page.into_body(), 1 << 16).await.unwrap().to_vec()).unwrap();
    let start = page.find("name=\"placebo-key\" value=\"").unwrap() + 26;
    page[start..start + page[start..].find('"').unwrap()].to_owned()
}

fn post(title: &str, key: &str, native: bool, cookie: &str) -> Request<Body> {
    let mut request = Request::post("/add")
        .header("host", "app.example")
        .header("sec-fetch-site", "same-origin")
        .header("referer", "http://app.example/")
        .header("cookie", cookie)
        .header("content-type", "application/x-www-form-urlencoded");
    if !native {
        request = request.header("x-placebo-request", placebo::VERSION.to_string());
    }
    let body = form_urlencoded::Serializer::new(String::new())
        .extend_pairs([("title", title), ("placebo-key", key)])
        .finish();
    request.body(Body::from(body)).unwrap()
}

/// The runtime's retry of an attempt first sent `ms` ago.
fn retried(mut request: Request<Body>, ms: u64) -> Request<Body> {
    request
        .headers_mut()
        .insert("x-placebo-retry", ms.to_string().parse().unwrap());
    request
}

async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn each_rendered_form_gets_a_fresh_key() {
    let app = app(Store::default());
    let first = rendered_key(&app).await;
    let second = rendered_key(&app).await;
    assert_eq!(first.len(), 32);
    assert_ne!(first, second);
}

#[tokio::test]
async fn a_repeated_submission_replays_the_recorded_reply() {
    let store = Store::default();
    let app = app(store.clone());
    let key = rendered_key(&app).await;
    let first = app
        .clone()
        .oneshot(post("Once", &key, false, "session=a"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert!(!first.headers().contains_key("x-placebo-replay"));
    let first = text(first).await;
    let again = app
        .clone()
        .oneshot(post("Once", &key, false, "session=a"))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(again.headers()["x-placebo-replay"], "replayed");
    assert_eq!(again.headers()["x-placebo-action"], "add");
    assert_eq!(again.headers()["content-type"], placebo::UPDATE_TYPE);
    assert_eq!(text(again).await, first);
    assert_eq!(store.runs.load(Ordering::SeqCst), 1);
    assert_eq!(*store.items.lock().unwrap(), ["Once"]);
}

#[tokio::test]
async fn other_values_are_new_submissions_but_changed_cookies_are_not() {
    let store = Store::default();
    let app = app(store.clone());
    let key = rendered_key(&app).await;
    for (title, cookie) in [("One", "session=a"), ("Two", "session=a")] {
        let response = app
            .clone()
            .oneshot(post(title, &key, false, cookie))
            .await
            .unwrap();
        assert!(!response.headers().contains_key("x-placebo-replay"));
    }
    assert_eq!(store.runs.load(Ordering::SeqCst), 2);
    // A cookie that changed between an attempt and its retry, such as a
    // refreshed session or an analytics cookie, does not make it run again.
    let response = app
        .clone()
        .oneshot(post("One", &key, false, "session=a; _ga=2"))
        .await
        .unwrap();
    assert_eq!(response.headers()["x-placebo-replay"], "replayed");
    assert_eq!(store.runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn headers_the_handler_adds_survive_native_replies_and_replays() {
    let store = Store::default();
    let app = placebo::native_forms(app(store.clone()));
    let key = rendered_key(&app).await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("Native", &key, true, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[SET_COOKIE], "added=1");
    }
    // The page rendered again for a rejected submission keeps them too.
    let key = rendered_key(&app).await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("x", &key, true, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(response.headers()[SET_COOKIE], "rejected=1");
        assert!(text(response).await.contains("Too short."));
    }
    let key = rendered_key(&app).await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("Runtime", &key, false, ""))
            .await
            .unwrap();
        assert_eq!(response.headers()[SET_COOKIE], "added=1");
    }
    assert_eq!(store.runs.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn a_submission_whose_browser_disconnected_still_finishes() {
    let store = Store::default();
    let app = app(store.clone());
    let key = rendered_key(&app).await;
    // The browser gives up while the handler is still running.
    let dropped = tokio::time::timeout(
        Duration::from_millis(30),
        app.clone().oneshot(post("slow", &key, false, "")),
    )
    .await;
    assert!(dropped.is_err());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(*store.items.lock().unwrap(), ["slow"]);
    // Its reply was recorded, so the retry replays it.
    let response = app
        .clone()
        .oneshot(retried(post("slow", &key, false, ""), 400))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-placebo-replay"], "replayed");
    assert_eq!(store.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_retry_older_than_the_store_remembers_is_not_run() {
    let store = Store::default();
    let app = app(store.clone()).layer(placebo::replays(MemoryReplays::new(
        Duration::from_secs(60),
        1,
    )));
    // A retry whose first attempt never arrived runs.
    let key = rendered_key(&app).await;
    let response = app
        .clone()
        .oneshot(retried(post("Never arrived", &key, false, ""), 0))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // One older than the store's time-to-live may have written.
    let key = rendered_key(&app).await;
    let response = app
        .clone()
        .oneshot(retried(post("Too late", &key, false, ""), 61_000))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["x-placebo-replay"], "unknown");
    // With room for one reply, a second submission pushes out the first. A
    // retry of the first is no longer vouched for, however recent.
    let first = rendered_key(&app).await;
    app.clone()
        .oneshot(post("First", &first, false, ""))
        .await
        .unwrap();
    let second = rendered_key(&app).await;
    app.clone()
        .oneshot(post("Second", &second, false, ""))
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(retried(post("First", &first, false, ""), 1_000))
        .await
        .unwrap();
    assert_eq!(response.headers()["x-placebo-replay"], "unknown");
    assert_eq!(
        *store.items.lock().unwrap(),
        ["Never arrived", "First", "Second"]
    );
}

#[tokio::test]
async fn rejected_replies_replay_and_errors_run_again() {
    let store = Store::default();
    let app = app(store.clone());
    let key = rendered_key(&app).await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("x", &key, false, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert_eq!(store.runs.load(Ordering::SeqCst), 1);
    // A response that is not a reply is not recorded.
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("missing", &key, false, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(store.runs.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn a_concurrent_duplicate_waits_for_the_first_and_replays_it() {
    let store = Store::default();
    let app = app(store.clone());
    let key = rendered_key(&app).await;
    let (first, second) = tokio::join!(app.clone().oneshot(post("slow", &key, false, "")), async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        app.clone().oneshot(post("slow", &key, false, "")).await
    });
    assert_eq!(first.unwrap().status(), StatusCode::OK);
    let second = second.unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(second.headers()["x-placebo-replay"], "replayed");
    assert_eq!(store.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_double_native_submit_saves_once_and_both_redirect() {
    let store = Store::default();
    let app = placebo::native_forms(app(store.clone()));
    let key = rendered_key(&app).await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("Native", &key, true, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()["location"], "/");
    }
    assert_eq!(*store.items.lock().unwrap(), ["Native"]);
    // A replayed rejection renders its page again.
    let key = rendered_key(&app).await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(post("x", &key, true, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(text(response).await.contains("Too short."));
    }
    assert_eq!(store.runs.load(Ordering::SeqCst), 2);
}

/// A store an application might write on its database, reduced to a map.
#[derive(Default)]
struct Table(Mutex<HashMap<String, Option<Recorded>>>);

impl ReplayStore for Table {
    fn claim<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Claim> {
        Box::pin(async move {
            let mut rows = self.0.lock().unwrap();
            Ok(match rows.get(id) {
                Some(Some(reply)) => Claim::Recorded(reply.clone()),
                Some(None) => Claim::Pending,
                None => {
                    rows.insert(id.to_owned(), None);
                    Claim::New
                }
            })
        })
    }
    fn record<'a>(&'a self, id: &'a str, reply: Recorded) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().insert(id.to_owned(), Some(reply));
            Ok(())
        })
    }
    fn release<'a>(&'a self, id: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().remove(id);
            Ok(())
        })
    }
}

#[tokio::test(start_paused = true)]
async fn an_unfinished_submission_is_reported_instead_of_run_again() {
    let table = Arc::new(Table::default());
    let store = Store::default();
    let app = app(store.clone()).layer(placebo::replays(Shared(table.clone())));
    let key = rendered_key(&app).await;
    // The first submission claimed its id and then the process stopped.
    let response = app
        .clone()
        .oneshot(post("Once", &key, false, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for reply in table.0.lock().unwrap().values_mut() {
        *reply = None;
    }
    let response = app
        .clone()
        .oneshot(post("Once", &key, false, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.headers()["x-placebo-replay"], "pending");
    assert!(text(response).await.contains("Still saving"));
    assert_eq!(store.runs.load(Ordering::SeqCst), 1);
}

struct Shared(Arc<Table>);

impl ReplayStore for Shared {
    fn claim<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Claim> {
        self.0.claim(id)
    }
    fn record<'a>(&'a self, id: &'a str, reply: Recorded) -> StoreFuture<'a, ()> {
        self.0.record(id, reply)
    }
    fn release<'a>(&'a self, id: &'a str) -> StoreFuture<'a, ()> {
        self.0.release(id)
    }
}

#[tokio::test]
async fn the_memory_store_forgets_expired_and_excess_replies() {
    let reply = || Recorded {
        status: 200,
        headers: Vec::new(),
        body: "{}".into(),
    };
    let store = MemoryReplays::new(Duration::from_secs(600), 2);
    for id in ["a", "b", "c"] {
        assert!(matches!(store.claim(id).await.unwrap(), Claim::New));
        store.record(id, reply()).await.unwrap();
    }
    // "a" was the oldest beyond two entries.
    assert!(matches!(store.claim("a").await.unwrap(), Claim::New));
    assert!(matches!(
        store.claim("c").await.unwrap(),
        Claim::Recorded(_)
    ));
    let store = MemoryReplays::new(Duration::ZERO, 10);
    store.claim("a").await.unwrap();
    store.record("a", reply()).await.unwrap();
    assert!(matches!(store.claim("a").await.unwrap(), Claim::New));
}

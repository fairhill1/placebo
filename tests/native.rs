//! Mutation forms submitted without JavaScript run the same handler. These
//! tests post what a browser posts for a plain HTML form.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{Markup, html};
use placebo::{Component, Control, FormInput, Input, MutationAction, fields};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Save {
    id: u64,
    version: u64,
    title: String,
    done: bool,
}

const SAVE: MutationAction<Save> = MutationAction::new("save", "/save");

struct Item {
    title: String,
    done: bool,
    version: u64,
}
type Store = Arc<Mutex<Item>>;

fn editor(item: &Item, title: &str, done: bool, feedback: &str) -> Markup {
    let fields = fields! { Save {
        @field id = Control::hidden(1);
        @field version = Control::hidden(item.version);
        @field title = Control::text(title).id("title").invalid(feedback == "Too short.");
        @field done = Control::checkbox(done).id("done");
        p #feedback role="status" { (feedback) }
    } };
    SAVE.bind(&Component::new("editor", 1)).form(fields)
}

async fn home(State(store): State<Store>) -> Markup {
    let item = store.lock().unwrap();
    html! {
        h1 { "The page" }
        (Component::new("editor", 1).mount(editor(&item, &item.title, item.done, "")))
    }
}

async fn save(State(store): State<Store>, Input(input): Input<Save>) -> Response {
    let mut item = store.lock().unwrap();
    let binding = SAVE.bind(&Component::new("editor", input.id));
    if input.title.len() < 3 {
        return binding
            .invalid(editor(&item, &input.title, input.done, "Too short."))
            .into_response();
    }
    if input.version != item.version {
        let current = (item.title.clone(), item.done);
        return binding
            .conflict(editor(&item, &current.0, current.1, "Changed elsewhere."))
            .into_response();
    }
    item.title = input.title;
    item.done = input.done;
    item.version += 1;
    let reply = binding.reply(editor(&item, &item.title, item.done, "Saved."));
    if item.title == "Go elsewhere" {
        return reply.navigate("/elsewhere").into_response();
    }
    reply.into_response()
}

fn store() -> Store {
    Arc::new(Mutex::new(Item {
        title: "First".into(),
        done: false,
        version: 1,
    }))
}

fn router(store: Store) -> Router {
    Router::new()
        .route("/", get(home))
        .route(SAVE.path(), SAVE.route(save))
        .with_state(store)
}

/// The base field the form rendered, as a browser would submit it.
async fn rendered_base(app: &Router) -> String {
    let page = app
        .clone()
        .oneshot(page_request("/"))
        .await
        .unwrap()
        .into_body();
    let page = String::from_utf8(to_bytes(page, 1 << 16).await.unwrap().to_vec()).unwrap();
    let start = page.find("name=\"placebo-base\" value=\"").unwrap() + 27;
    let end = start + page[start..].find('"').unwrap();
    page[start..end].replace("&amp;", "&")
}

fn page_request(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("host", "app.example")
        .body(Body::empty())
        .unwrap()
}

fn native_post(body: String, referer: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/save")
        .header("host", "app.example")
        .header("origin", "http://app.example")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(referer) = referer {
        request = request.header("referer", referer);
    }
    request.body(Body::from(body)).unwrap()
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
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
async fn a_successful_native_save_redirects_back_to_its_page() {
    let store = store();
    let app = placebo::native_forms(router(store.clone()));
    let base = rendered_base(&app).await;
    let body = form_body(&[
        ("id", "1"),
        ("version", "1"),
        ("title", "Second"),
        ("placebo-base", &base),
    ]);
    let response = app
        .clone()
        .oneshot(native_post(body, Some("http://app.example/?tab=2")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["location"], "/?tab=2");
    assert_eq!(store.lock().unwrap().title, "Second");
}

#[tokio::test]
async fn a_native_redirect_uses_navigate_and_never_another_origin() {
    let app = placebo::native_forms(router(store()));
    let navigate = form_body(&[("id", "1"), ("version", "1"), ("title", "Go elsewhere")]);
    let response = app
        .clone()
        .oneshot(native_post(navigate, Some("http://app.example/")))
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/elsewhere");
    // A Referer from another host is ignored.
    let body = form_body(&[("id", "1"), ("version", "2"), ("title", "Third")]);
    let response = app
        .oneshot(native_post(body, Some("https://attacker.example/steal")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()["location"], "/");
}

#[tokio::test]
async fn an_invalid_native_save_renders_the_whole_page_with_the_reply() {
    let app = placebo::native_forms(router(store()));
    let base = rendered_base(&app).await;
    let body = form_body(&[
        ("id", "1"),
        ("version", "1"),
        ("title", "x"),
        ("done", "true"),
        ("placebo-base", &base),
    ]);
    let response = app
        .oneshot(native_post(body, Some("http://app.example/")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let page = text(response).await;
    assert!(page.contains("<h1>The page</h1>"), "{page}");
    assert!(page.contains("Too short."), "{page}");
    // The person's values, and focus on the invalid control.
    assert!(page.contains("value=\"x\""), "{page}");
    assert!(page.contains("autofocus"), "{page}");
    assert!(page.contains("id=\"done\" checked") || page.contains("checked id=\"done\""));
}

#[tokio::test]
async fn a_native_conflict_keeps_edited_fields_and_shows_the_saved_rest() {
    let store = store();
    let app = placebo::native_forms(router(store.clone()));
    let base = rendered_base(&app).await;
    // Another tab marks it done and renames it.
    {
        let mut item = store.lock().unwrap();
        item.done = true;
        item.title = "Renamed elsewhere".into();
        item.version = 2;
    }
    // This person only changed the title.
    let body = form_body(&[
        ("id", "1"),
        ("version", "1"),
        ("title", "My edit"),
        ("placebo-base", &base),
    ]);
    let response = app
        .clone()
        .oneshot(native_post(body, Some("http://app.example/")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let page = text(response).await;
    assert!(page.contains("Changed elsewhere."), "{page}");
    assert!(page.contains("value=\"My edit\""), "{page}");
    assert!(!page.contains("Renamed elsewhere\" id"), "{page}");
    // Untouched: the other tab's value. Hidden: the new version.
    assert!(page.contains("value=\"true\" checked"), "{page}");
    assert!(page.contains("name=\"version\" value=\"2\""), "{page}");
    // The title stays edited on the rejected page, so a second conflict
    // keeps it too, as the runtime keeps an edited control's node.
    store.lock().unwrap().version = 3;
    let start = page.find("name=\"placebo-base\" value=\"").unwrap() + 27;
    let base = page[start..start + page[start..].find('"').unwrap()].replace("&amp;", "&");
    let body = form_body(&[
        ("id", "1"),
        ("version", "2"),
        ("title", "My edit"),
        ("done", "true"),
        ("placebo-base", &base),
    ]);
    let response = app
        .oneshot(native_post(body, Some("http://app.example/")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(text(response).await.contains("value=\"My edit\""));
}

#[tokio::test]
async fn without_the_wrapper_a_rejected_native_save_gets_the_component_alone() {
    let app = router(store());
    let body = form_body(&[("id", "1"), ("version", "1"), ("title", "x")]);
    let response = app
        .oneshot(native_post(body, Some("http://app.example/")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let page = text(response).await;
    assert!(!page.contains("<h1>The page</h1>"));
    assert!(page.contains("id=\"editor:1\""));
    assert!(page.contains("Too short."));
    assert!(page.contains("value=\"x\""));
}

#[tokio::test]
async fn a_page_that_does_not_mount_the_component_falls_back_to_the_component() {
    let app = placebo::native_forms(router(store()).route(
        "/other",
        get(|| async {
            html! { h1 { "Other" } }
        }),
    ));
    let body = form_body(&[("id", "1"), ("version", "1"), ("title", "x")]);
    let response = app
        .oneshot(native_post(body, Some("http://app.example/other")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let page = text(response).await;
    assert!(!page.contains("Other"));
    assert!(page.contains("Too short."));
}

#[tokio::test]
async fn runtime_requests_still_get_updates() {
    let app = placebo::native_forms(router(store()));
    let mut request = native_post(
        form_body(&[("id", "1"), ("version", "1"), ("title", "x")]),
        Some("http://app.example/"),
    );
    request.headers_mut().insert(
        "x-placebo-request",
        placebo::VERSION.to_string().parse().unwrap(),
    );
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers()["content-type"], placebo::UPDATE_TYPE);
}

#[tokio::test]
async fn forms_on_a_rejected_page_remember_the_page_they_are_on() {
    let app = placebo::native_forms(router(store()));
    let body = form_body(&[("id", "1"), ("version", "1"), ("title", "x")]);
    let page = text(
        app.clone()
            .oneshot(native_post(body, Some("http://app.example/?tab=2")))
            .await
            .unwrap(),
    )
    .await;
    // The address now shows the action path, so the form carries its page.
    assert!(
        page.contains("name=\"placebo-page\" value=\"/?tab=2\""),
        "{page}"
    );
    let body = form_body(&[
        ("id", "1"),
        ("version", "1"),
        ("title", "Fixed"),
        ("placebo-page", "/?tab=2"),
    ]);
    let response = app
        .clone()
        .oneshot(native_post(body, Some("http://app.example/save")))
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/?tab=2");
    // Only paths on this site.
    let body = form_body(&[
        ("id", "1"),
        ("version", "2"),
        ("title", "Again"),
        ("placebo-page", "//attacker.example/"),
    ]);
    let response = app
        .oneshot(native_post(body, Some("http://app.example/")))
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/");
    // Pages rendered normally carry no page field.
    let home = text(router(store()).oneshot(page_request("/")).await.unwrap()).await;
    assert!(!home.contains("placebo-page"));
}

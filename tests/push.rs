//! Feeds: declared targets, the subscription mount, replay after a
//! reconnect, and component revisions in replies.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use maud::html;
use placebo::{Component, Feed, FormInput, List, MutationAction, Position, VersionedRegion};
use std::time::Duration;
use tower::ServiceExt;

const COUNT: VersionedRegion = VersionedRegion::new("count");
const TASKS: List = List::new("tasks");

fn feed() -> Feed {
    Feed::new("live", "/live")
        .affects(COUNT)
        .affects(TASKS)
        .affects_kind("task")
}

/// The feed's position as its mount renders it.
fn position(feed: &Feed) -> String {
    let mount = feed.mount().into_string();
    let start = mount.find("after=").unwrap() + 6;
    mount[start..start + mount[start..].find('&').unwrap()].to_owned()
}

/// Read events from a feed's stream until `count` have arrived.
async fn events(feed: &Feed, request: Request<Body>, count: usize) -> Vec<String> {
    let app = Router::new().route(feed.path(), feed.route());
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut stream = response.into_body().into_data_stream();
    let mut text = String::new();
    // The stream opens with a comment, then the events.
    while text.matches("\n\n").count() < count + 1 {
        let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("an event arrives")
            .unwrap()
            .unwrap();
        text.push_str(std::str::from_utf8(&chunk).unwrap());
    }
    assert!(
        text.starts_with(":connected\n\n") || text.starts_with(": connected\n\n"),
        "{text}"
    );
    text.split("\n\n")
        .filter(|event| !event.is_empty() && !event.starts_with(':'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_mount_declares_targets_and_the_position_to_follow_from() {
    let feed = feed();
    let mount = feed.mount().into_string();
    assert!(mount.starts_with("<div id=\"live\" hidden data-placebo-feed="));
    assert!(mount.contains("&quot;targets&quot;:[&quot;count&quot;,&quot;tasks&quot;]"));
    assert!(mount.contains("&quot;kinds&quot;:[&quot;task&quot;]"));
    assert!(position(&feed).ends_with("-0"));
    feed.push().replace(COUNT, 2, html! { "2" }).send();
    assert!(position(&feed).ends_with("-1"));
}

#[tokio::test]
async fn a_page_gets_what_was_published_after_its_position_then_new_updates() {
    let feed = feed();
    let rendered = position(&feed);
    feed.push().replace(COUNT, 2, html! { "2 tasks" }).send();
    feed.push()
        .insert(TASKS.item(3).mount(html! { "Three" }), Position::End)
        .send();
    let request = Request::get(format!("/live?after={rendered}"))
        .body(Body::empty())
        .unwrap();
    let publisher = feed.clone();
    let later = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        publisher
            .push()
            .refresh(
                &Component::new("task", 3).revision(7),
                html! { "Three, saved" },
            )
            .send();
    });
    let events = events(&feed, request, 3).await;
    later.await.unwrap();
    let instance = rendered.rsplit_once('-').unwrap().0;
    assert!(events[0].contains("event: update"));
    assert!(events[0].contains(&format!("id: {instance}-1")));
    assert!(
        events[0].contains(r#""target":"count","operation":"replace-children","revision":"2""#)
    );
    assert!(events[1].contains(r#""operation":"insert-item""#));
    assert!(events[1].contains(r#""item":"tasks/3""#));
    assert!(events[2].contains(&format!("id: {instance}-3")));
    assert!(
        events[2].contains(r#""target":"task:3","operation":"refresh-component","revision":"7""#)
    );
}

#[tokio::test]
async fn a_reconnect_resumes_from_its_last_event_id() {
    let feed = feed();
    let rendered = position(&feed);
    let instance = rendered.rsplit_once('-').unwrap().0.to_owned();
    for revision in 2..5 {
        feed.push().replace(COUNT, revision, html! {}).send();
    }
    // The browser's Last-Event-ID wins over the mount's position.
    let request = Request::get(format!("/live?after={rendered}"))
        .header("last-event-id", format!("{instance}-2"))
        .body(Body::empty())
        .unwrap();
    let events = events(&feed, request, 1).await;
    assert!(
        events[0].contains(&format!("id: {instance}-3")),
        "{events:?}"
    );
}

#[tokio::test]
async fn an_unknown_position_asks_the_page_to_resync() {
    let feed = feed();
    feed.push().replace(COUNT, 2, html! {}).send();
    // Another server process, or updates older than the feed keeps.
    let request = Request::get("/live?after=gone-1")
        .body(Body::empty())
        .unwrap();
    let events = events(&feed, request, 1).await;
    assert!(events[0].contains("event: resync"), "{events:?}");
    assert!(events[0].contains(r#"data: {"version":5,"feed":"live"}"#));
}

#[test]
#[should_panic(expected = "needs a revision")]
fn a_pushed_component_needs_a_revision() {
    feed()
        .push()
        .refresh(&Component::new("task", 1), html! {})
        .send();
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "not declared on feed 'live'")]
fn a_feed_pushes_only_declared_targets() {
    feed()
        .push()
        .replace(VersionedRegion::new("other"), 1, html! {})
        .send();
}

#[allow(dead_code)]
#[derive(serde::Deserialize, FormInput)]
struct Save {
    title: String,
}

#[tokio::test]
async fn versioned_components_carry_their_revision_in_mounts_and_replies() {
    use axum::response::IntoResponse;
    let component = Component::new("task", 1).revision(4);
    let mount = html! { (component.mount(html! {})) }.into_string();
    assert!(mount.contains("data-placebo-revision=\"4\""));
    let other = Component::new("task", 2).revision(9);
    let action = MutationAction::<Save>::new("save", "/save");
    let reply = action
        .bind(&component)
        .affects(&other)
        .reply(html! {})
        .also_refresh(&other, html! {})
        .into_response();
    let body = axum::body::to_bytes(reply.into_body(), 4096).await.unwrap();
    let update: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(update["revision"], "4");
    assert_eq!(update["patches"][0]["revision"], "9");
}

#[tokio::test]
async fn keyed_feeds_reach_only_the_pages_that_mount_their_key() {
    let inboxes: placebo::Feeds<String> =
        placebo::Feeds::new("inbox", "/live/inbox/{key}").affects(COUNT);
    let ada = inboxes.get(&"ada".to_owned());
    let bob = inboxes.get(&"bob smith".to_owned());
    // Each key has its own id and URL, and the same feed on every get.
    assert!(
        ada.mount()
            .into_string()
            .starts_with("<div id=\"inbox:ada\" hidden")
    );
    assert_eq!(bob.path(), "/live/inbox/bob%20smith");
    assert!(
        bob.mount()
            .into_string()
            .contains("id=\"inbox:bob%20smith\"")
    );
    assert_eq!(position(&inboxes.get(&"ada".to_owned())), position(&ada));
    let ada_position = position(&ada);
    let bob_position = position(&bob);
    bob.push().replace(COUNT, 5, html! { "Bob's" }).send();
    ada.push().replace(COUNT, 2, html! { "Ada's" }).send();

    // A handler picks the feed; here from the path.
    let feeds = inboxes.clone();
    let app: Router = Router::new().route(
        inboxes.path(),
        axum::routing::get(
            move |axum::extract::Path(key): axum::extract::Path<String>,
                  request: axum::extract::Request| async move {
                feeds.get(&key).stream(&request)
            },
        ),
    );
    for (path, position, own, other) in [
        ("/live/inbox/ada", ada_position, "Ada's", "Bob's"),
        ("/live/inbox/bob%20smith", bob_position, "Bob's", "Ada's"),
    ] {
        let request = Request::get(format!("{path}?after={position}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let mut stream = response.into_body().into_data_stream();
        let mut text = String::new();
        while !text.contains(own) {
            let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .expect("an event arrives")
                .unwrap()
                .unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
        }
        assert!(!text.contains(other), "{text}");
    }
}

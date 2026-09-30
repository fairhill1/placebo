//! Feeds: the subscription mount, the "changed" signal, and what a page that
//! connects late or reconnects is told.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use placebo::Feed;
use std::time::Duration;
use tower::ServiceExt;

/// The feed's position as its mount renders it.
fn position(feed: &Feed) -> String {
    let mount = feed.mount().into_string();
    let start = mount.find("after=").unwrap() + 6;
    mount[start..start + mount[start..].find('&').unwrap()].to_owned()
}

/// The response to a subscription, checked to be an event stream.
async fn subscribe(app: Router, request: Request<Body>) -> axum::body::BodyDataStream {
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    response.into_body().into_data_stream()
}

/// Read events from a stream until `count` have arrived.
async fn events(stream: &mut axum::body::BodyDataStream, count: usize) -> Vec<String> {
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

/// Whether the stream stays quiet for a moment.
async fn quiet(stream: &mut axum::body::BodyDataStream) -> bool {
    let mut text = String::new();
    while let Ok(Some(chunk)) =
        tokio::time::timeout(Duration::from_millis(100), stream.next()).await
    {
        text.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
    }
    !text.contains("event:")
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

#[test]
fn the_mount_records_the_position_to_follow_from() {
    let feed = Feed::new("live", "/live");
    let mount = feed.mount().into_string();
    assert!(mount.starts_with("<div id=\"live\" hidden data-placebo-feed="));
    assert!(position(&feed).ends_with("-0"));
    feed.changed();
    assert!(position(&feed).ends_with("-1"));
}

#[tokio::test]
async fn a_page_that_missed_changes_is_told_once_then_follows_new_ones() {
    let feed = Feed::new("live", "/live");
    let rendered = position(&feed);
    let instance = rendered.rsplit_once('-').unwrap().0.to_owned();
    feed.changed();
    feed.changed();
    let app = Router::new().route(feed.path(), feed.route());
    let mut stream = subscribe(app, get(&format!("/live?after={rendered}"))).await;
    let publisher = feed.clone();
    let later = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        publisher.changed();
    });
    let events = events(&mut stream, 2).await;
    later.await.unwrap();
    // One signal covers both missed changes.
    assert!(events[0].contains("event: changed"), "{events:?}");
    assert!(events[0].contains(&format!("id: {instance}-2")));
    assert!(events[0].contains(r#"data: {"version":6,"feed":"live"}"#));
    assert!(events[1].contains(&format!("id: {instance}-3")));
}

#[tokio::test]
async fn an_up_to_date_page_is_told_nothing_until_a_change() {
    let feed = Feed::new("live", "/live");
    feed.changed();
    let rendered = position(&feed);
    let app = Router::new().route(feed.path(), feed.route());
    let mut stream = subscribe(app.clone(), get(&format!("/live?after={rendered}"))).await;
    assert!(quiet(&mut stream).await);
    // Without a position, a page follows from now.
    let mut stream = subscribe(app, get("/live")).await;
    assert!(quiet(&mut stream).await);
}

#[tokio::test]
async fn a_reconnect_is_told_about_changes_after_its_last_event_id() {
    let feed = Feed::new("live", "/live");
    let rendered = position(&feed);
    let instance = rendered.rsplit_once('-').unwrap().0.to_owned();
    feed.changed();
    feed.changed();
    let app = Router::new().route(feed.path(), feed.route());
    // The browser's Last-Event-ID wins over the mount's position.
    let request = Request::get(format!("/live?after={rendered}"))
        .header("last-event-id", format!("{instance}-2"))
        .body(Body::empty())
        .unwrap();
    let mut stream = subscribe(app.clone(), request).await;
    assert!(quiet(&mut stream).await);
    let request = Request::get(format!("/live?after={rendered}"))
        .header("last-event-id", format!("{instance}-1"))
        .body(Body::empty())
        .unwrap();
    let mut stream = subscribe(app, request).await;
    let events = events(&mut stream, 1).await;
    assert!(
        events[0].contains(&format!("id: {instance}-2")),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_position_from_another_server_process_is_told_to_refresh() {
    let feed = Feed::new("live", "/live");
    let app = Router::new().route(feed.path(), feed.route());
    let mut stream = subscribe(app, get("/live?after=gone-7")).await;
    let events = events(&mut stream, 1).await;
    assert!(events[0].contains("event: changed"), "{events:?}");
}

#[tokio::test]
async fn keyed_feeds_reach_only_the_pages_that_mount_their_key() {
    let inboxes: placebo::Feeds<String> = placebo::Feeds::new("inbox", "/live/inbox/{key}");
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
    bob.changed();

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
    let mut stream = subscribe(
        app.clone(),
        get(&format!("/live/inbox/bob%20smith?after={bob_position}")),
    )
    .await;
    let events = events(&mut stream, 1).await;
    assert!(
        events[0].contains(r#""feed":"inbox:bob%20smith""#),
        "{events:?}"
    );
    let mut stream = subscribe(app, get(&format!("/live/inbox/ada?after={ada_position}"))).await;
    assert!(quiet(&mut stream).await);
}

#[tokio::test]
async fn a_signal_from_a_save_names_the_request_that_made_it() {
    use placebo::{Component, FormInput, Input, MutationAction};
    #[derive(serde::Deserialize, FormInput)]
    struct Save {
        #[allow(dead_code)]
        title: String,
    }
    const SAVE: MutationAction<Save> = MutationAction::new("save", "/save");
    let feed = Feed::new("live", "/live");
    let rendered = position(&feed);
    let writer = feed.clone();
    let app = Router::new().route(feed.path(), feed.route()).route(
        SAVE.path(),
        SAVE.route(move |Input(_): Input<Save>| {
            let feed = writer.clone();
            async move {
                feed.changed();
                SAVE.bind(&Component::new("editor", 1))
                    .reply(maud::html! { "Saved." })
            }
        }),
    );
    let mut stream = subscribe(app.clone(), get(&format!("/live?after={rendered}"))).await;
    let save = Request::post("/save")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header("x-placebo-request", "6")
        .header("x-placebo-request-id", "req-42")
        .body(Body::from("title=Hello"))
        .unwrap();
    app.clone().oneshot(save).await.unwrap();
    // A change made outside a save names no request.
    feed.changed();
    let events = events(&mut stream, 2).await;
    assert!(
        events[0].contains(r#"data: {"version":6,"feed":"live","request":"req-42"}"#),
        "{events:?}"
    );
    assert!(
        events[1].contains(r#"data: {"version":6,"feed":"live"}"#),
        "{events:?}"
    );
}

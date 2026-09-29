//! Server push. A [`Feed`] tells every page that follows it that something
//! changed. Each page reads itself again and morphs in the differences, by
//! the same rules as a reply: edited controls, open dialogs, and components
//! with a request in flight keep what the person has.
use axum::{
    extract::Request,
    http::{self, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::MethodRouter,
};
use futures_util::stream;
use maud::{Markup, html};
use serde::Serialize;
use std::{
    collections::HashMap,
    convert::Infallible,
    fmt::Display,
    hash::Hash,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

use crate::VERSION;

/// How long a keyed feed nobody follows is kept, for pages rendered with its
/// mount that have not connected yet, and for pages that reconnect.
const IDLE: Duration = Duration::from_secs(10 * 60);

/// A signal for the pages that mount it: "what you show has changed". Mount
/// it on the page, register its route, and call [`Feed::changed`] after a
/// write:
///
/// ```
/// use axum::{Router, routing::get};
/// use maud::html;
/// use placebo::Feed;
///
/// let live = Feed::new("live", "/live");
///
/// // The page: its contents and the feed's mount.
/// let page = html! { p { "1 task" } (live.mount()) };
/// // The route that streams it.
/// let app: Router = Router::new().route(live.path(), live.route());
/// // A handler, after a write.
/// live.changed();
/// ```
///
/// Every page following the feed reads itself again, so each renders what
/// its viewer may see. To tell only some pages, such as one person's or one
/// document's, use [`Feeds`]. Guard a feed's route like any other route.
///
/// A page that reconnects after missing a change reads itself again. Several
/// changes close together may reach a page as one.
#[derive(Clone)]
pub struct Feed(Arc<Inner>);

struct Inner {
    id: String,
    path: String,
    instance: String,
    sender: broadcast::Sender<Signal>,
    state: Mutex<State>,
}

struct State {
    seq: u64,
    /// When the feed was last mounted, published to, or subscribed to.
    used: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            seq: 0,
            used: Instant::now(),
        }
    }
}

fn assert_kind(kind: &str) {
    assert!(
        !kind.is_empty() && !kind.contains(':') && !kind.contains(char::is_whitespace),
        "a kind is the part of a keyed id before ':'"
    );
}

/// A family of feeds, one per key: per person, per document, or per team.
/// Each keyed feed has its own subscribers, so a change reaches only the
/// pages mounting that key's feed.
///
/// ```
/// use axum::{Router, extract::{Path, Request}, response::Response, routing::get};
/// use maud::html;
/// use placebo::Feeds;
///
/// let inboxes: Feeds<u64> = Feeds::new("inbox", "/live/inbox/{key}");
///
/// // The page, for person 7: their inbox's mount.
/// let page = html! { p { "No messages" } (inboxes.get(&7).mount()) };
/// // The route picks the feed. Check that the person may follow it, as for
/// // any other route, for example from the session.
/// let feeds = inboxes.clone();
/// let app: Router = Router::new().route(
///     inboxes.path(),
///     get(move |Path(person): Path<u64>, request: Request| async move {
///         feeds.get(&person).stream(&request)
///     }),
/// );
/// // A handler, after a write for person 7.
/// inboxes.get(&7).changed();
/// ```
///
/// A keyed feed's id, and so its mount's element id, is `kind:key`, with
/// characters other than letters, digits, and `-._~` in the key escaped. A
/// path with `{key}` has the key in each feed's URL; without it, every keyed
/// feed streams from the same path, and the handler picks the feed from the
/// request, such as the signed-in person's. Check access before `get`, as
/// each key opens a feed. A keyed feed that no page has followed or mounted
/// for ten minutes is dropped: get a keyed feed when you mount or push,
/// rather than keeping it.
pub struct Feeds<K>(Arc<Family<K>>);

impl<K> Clone for Feeds<K> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

struct Family<K> {
    kind: &'static str,
    path: &'static str,
    open: Mutex<Open<K>>,
}

struct Open<K> {
    feeds: HashMap<K, Feed>,
    /// Drop idle feeds when there are this many.
    prune_at: usize,
}

impl<K: Eq + Hash + Clone + Display> Feeds<K> {
    pub fn new(kind: &'static str, path: &'static str) -> Self {
        assert_kind(kind);
        assert!(path.starts_with('/'), "a feed's path starts with '/'");
        Self(Arc::new(Family {
            kind,
            path,
            open: Mutex::new(Open {
                feeds: HashMap::new(),
                prune_at: 64,
            }),
        }))
    }

    /// The route path, with `{key}` if the key is part of each feed's URL.
    pub fn path(&self) -> &'static str {
        self.0.path
    }

    /// The feed for `key`, opened on first use.
    pub fn get(&self, key: &K) -> Feed {
        let mut open = self.0.open.lock().unwrap();
        if let Some(feed) = open.feeds.get(key) {
            return feed.clone();
        }
        if open.feeds.len() >= open.prune_at {
            open.feeds.retain(|_, feed| !feed.idle());
            open.prune_at = (open.feeds.len() * 2).max(64);
        }
        let key_text = path_segment(&key.to_string());
        let feed = Feed::with(
            format!("{}:{key_text}", self.0.kind),
            self.0.path.replace("{key}", &key_text),
        );
        open.feeds.insert(key.clone(), feed.clone());
        feed
    }
}

/// A key as one path segment and part of an id: bytes outside the
/// unreserved set are escaped.
fn path_segment(text: &str) -> String {
    let mut segment = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            segment.push(byte as char);
        } else {
            segment.push_str(&format!("%{byte:02X}"));
        }
    }
    segment
}

impl Feed {
    pub fn new(id: &'static str, path: &'static str) -> Self {
        Self::with(id.to_owned(), path.to_owned())
    }

    fn with(id: String, path: String) -> Self {
        assert!(
            !id.is_empty() && !id.contains(char::is_whitespace),
            "a feed needs an id without whitespace"
        );
        assert!(path.starts_with('/'), "a feed's path starts with '/'");
        // A page that falls behind is told once that something changed.
        let (sender, _) = broadcast::channel(16);
        Self(Arc::new(Inner {
            id,
            path,
            instance: crate::replay::new_key()[..12].to_owned(),
            sender,
            state: Mutex::default(),
        }))
    }

    pub fn path(&self) -> &str {
        &self.0.path
    }

    /// The element that subscribes the page. It records the feed's position,
    /// so a change made after the page was rendered is not missed: render it
    /// under the same lock or transaction as the data it follows.
    pub fn mount(&self) -> Markup {
        let seq = {
            let mut state = self.0.state.lock().unwrap();
            state.used = Instant::now();
            state.seq
        };
        let config = Config {
            version: VERSION,
            feed: &self.0.id,
            url: format!("{}?after={}-{seq}", self.0.path, self.0.instance),
        };
        let config = serde_json::to_string(&config).expect("feed configuration serializes");
        html! { div id=(&self.0.id) hidden data-placebo-feed=(config) {} }
    }

    /// Tell every page following this feed that what it shows has changed.
    /// Call it after the write is committed. Called from a save's handler,
    /// the signal names the save's request, and the page that sent it skips
    /// it: the save's reply already shows that page.
    pub fn changed(&self) {
        let request = crate::native::current_request().map(Arc::from);
        let mut state = self.0.state.lock().unwrap();
        state.seq += 1;
        state.used = Instant::now();
        // No subscribers is not an error.
        let _ = self.0.sender.send(Signal {
            seq: state.seq,
            request,
        });
    }

    /// The GET route that streams this feed as Server-Sent Events. For a
    /// feed a handler picks, such as one of [`Feeds`], use [`Feed::stream`].
    pub fn route<S: Clone + Send + Sync + 'static>(&self) -> MethodRouter<S> {
        let feed = self.clone();
        axum::routing::get(move |request: Request| {
            let feed = feed.clone();
            async move { feed.stream(&request) }
        })
    }

    fn event(&self, seq: u64, request: Option<&str>) -> Event {
        let data = SignalData {
            version: VERSION,
            feed: &self.0.id,
            request,
        };
        Event::default()
            .event("changed")
            .id(format!("{}-{seq}", self.0.instance))
            .data(serde_json::to_string(&data).expect("a signal serializes"))
    }

    fn idle(&self) -> bool {
        self.0.sender.receiver_count() == 0 && self.0.state.lock().unwrap().used.elapsed() > IDLE
    }

    /// Answer a subscription with this feed's Server-Sent Events: tell a
    /// page that missed a change, then follow new ones. [`Feed::route`] does
    /// this; call it from your own handler to choose the feed, such as the
    /// signed-in person's feed of [`Feeds`].
    pub fn stream<B>(&self, request: &http::Request<B>) -> Response {
        let header = request
            .headers()
            .get("last-event-id")
            .and_then(|value| value.to_str().ok());
        let query = request.uri().query().and_then(|query| {
            form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "after")
                .map(|(_, value)| value.into_owned())
        });
        let last = header.map(str::to_owned).or(query);
        let receiver = self.0.sender.subscribe();
        // Some browsers report a stream open only once bytes arrive.
        let mut first = vec![Event::default().comment("connected")];
        let seen = {
            let mut state = self.0.state.lock().unwrap();
            state.used = Instant::now();
            let after = last.as_deref().map(|last| {
                last.rsplit_once('-')
                    .filter(|(instance, _)| *instance == self.0.instance)
                    .and_then(|(_, seq)| seq.parse::<u64>().ok())
            });
            match after {
                // A position from before a change, or from before the server
                // restarted: the page may be out of date.
                Some(after) if after != Some(state.seq) => first.push(self.event(state.seq, None)),
                _ => {}
            }
            state.seq
        };
        let feed = self.clone();
        let events = stream::unfold(
            (first, receiver, seen),
            move |(mut first, mut receiver, seen)| {
                let feed = feed.clone();
                async move {
                    if !first.is_empty() {
                        let event = first.remove(0);
                        return Some((Ok::<_, Infallible>(event), (first, receiver, seen)));
                    }
                    loop {
                        match receiver.recv().await {
                            Ok(signal) if signal.seq <= seen => continue,
                            Ok(Signal { seq, request }) => {
                                let event = feed.event(seq, request.as_deref());
                                return Some((Ok(event), (first, receiver, seq)));
                            }
                            // Behind the channel: one signal covers every change missed.
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                let seq = feed.0.state.lock().unwrap().seq;
                                return Some((Ok(feed.event(seq, None)), (first, receiver, seq)));
                            }
                            Err(broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                }
            },
        );
        let mut response = Sse::new(events)
            .keep_alive(KeepAlive::default())
            .into_response();
        // Proxies such as nginx otherwise buffer the stream.
        response
            .headers_mut()
            .insert("x-accel-buffering", header::HeaderValue::from_static("no"));
        response
    }
}

#[derive(Serialize)]
struct Config<'a> {
    version: u8,
    feed: &'a str,
    url: String,
}

/// One change, and the runtime request whose handler made it, if any.
#[derive(Clone)]
struct Signal {
    seq: u64,
    request: Option<Arc<str>>,
}

#[derive(Serialize)]
struct SignalData<'a> {
    version: u8,
    feed: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    request: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_feeds_nobody_follows_are_dropped_after_a_while() {
        let feeds: Feeds<u32> = Feeds::new("inbox", "/live/inbox");
        let followed = feeds.get(&0);
        let _receiver = followed.0.sender.subscribe();
        for key in 1..64 {
            feeds.get(&key);
        }
        let long_ago = Instant::now() - IDLE - Duration::from_secs(1);
        for key in 0..32 {
            feeds.get(&key).0.state.lock().unwrap().used = long_ago;
        }
        // The next new key prunes: idle ones go, followed and recent ones stay.
        feeds.get(&64);
        let open = feeds.0.open.lock().unwrap();
        assert_eq!(open.feeds.len(), 1 + 32 + 1);
        assert!(open.feeds.contains_key(&0) && !open.feeds.contains_key(&1));
        assert_eq!(open.prune_at, 66);
    }
}

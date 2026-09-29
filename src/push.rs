//! Server push. A [`Feed`] sends the same updates a mutation reply can make
//! (versioned region replacements, component refreshes, and list item
//! operations) to every page that mounts it, over Server-Sent Events.
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
    collections::{HashMap, VecDeque},
    convert::Infallible,
    fmt::Display,
    hash::Hash,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

use crate::{Component, Item, List, MountedItem, Patch, Position, VERSION, VersionedRegion};

/// How many recent updates a feed keeps for pages that reconnect.
const RECENT: usize = 256;
/// How long a keyed feed nobody follows is kept, for pages rendered with its
/// mount that have not connected yet, and for pages that reconnect.
const IDLE: Duration = Duration::from_secs(10 * 60);

/// A stream of updates for the pages that mount it. Declare what it may
/// update, mount it on the page, register its route, and publish with
/// [`Feed::push`]:
///
/// ```
/// use axum::{Router, routing::get};
/// use maud::html;
/// use placebo::{Feed, VersionedRegion};
///
/// const COUNT: VersionedRegion = VersionedRegion::new("count");
/// let live = Feed::new("live", "/live").affects(COUNT);
///
/// // The page: the region and the feed's mount.
/// let page = html! { (COUNT.mount(1, html! { "1 task" })) (live.mount()) };
/// // The route that streams it.
/// let app: Router = Router::new().route(live.path(), live.route());
/// // A handler, after a write, under the same lock as its revision.
/// live.push().replace(COUNT, 2, html! { "2 tasks" }).send();
/// ```
///
/// Every page receives every update, rendered once for all of them. For
/// updates only some people may see, or markup that depends on the viewer,
/// use [`Feeds`]: a feed per user or per document. Guard a feed's route like
/// any other route. Updates are ordered against
/// replies by revision: a versioned region or a versioned component (see
/// [`Component::revision`]) takes an update only when it is newer. List items
/// are ordered by the stream: a reply leaves an item a push changed after its
/// request was sent.
///
/// A page that reconnects gets the updates it missed from the feed's recent
/// updates. If they are gone, or the server restarted, it reads the page again
/// and takes the declared targets' newer state from it.
#[derive(Clone)]
pub struct Feed(Arc<Inner>);

struct Inner {
    id: String,
    path: String,
    instance: String,
    targets: Vec<String>,
    kinds: Vec<String>,
    sender: broadcast::Sender<Arc<Sent>>,
    state: Mutex<State>,
}

struct State {
    seq: u64,
    recent: VecDeque<Arc<Sent>>,
    /// When the feed was last mounted, published to, or subscribed to.
    used: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            seq: 0,
            recent: VecDeque::new(),
            used: Instant::now(),
        }
    }
}

struct Sent {
    seq: u64,
    data: String,
}

fn assert_kind(kind: &str) {
    assert!(
        !kind.is_empty() && !kind.contains(':') && !kind.contains(char::is_whitespace),
        "a kind is the part of a keyed id before ':'"
    );
}

/// A family of feeds, one per key: per person, per document, or per team.
/// Each keyed feed has its own subscribers and recent updates, so what one
/// is sent, rendered for its viewer, reaches only the pages mounting it.
///
/// ```
/// use axum::{Router, extract::{Path, Request}, response::Response, routing::get};
/// use maud::html;
/// use placebo::{Feeds, VersionedRegion};
///
/// const UNREAD: VersionedRegion = VersionedRegion::new("unread");
/// let inboxes: Feeds<u64> = Feeds::new("inbox", "/live/inbox/{key}").affects(UNREAD);
///
/// // The page, for person 7: their inbox's mount.
/// let page = html! { (UNREAD.mount(1, html! { "No messages" })) (inboxes.get(&7).mount()) };
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
/// inboxes.get(&7).push().replace(UNREAD, 2, html! { "1 message" }).send();
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
    targets: Vec<String>,
    kinds: Vec<String>,
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
            targets: Vec::new(),
            kinds: Vec::new(),
            open: Mutex::new(Open {
                feeds: HashMap::new(),
                prune_at: 64,
            }),
        }))
    }

    fn declare(mut self, declare: impl FnOnce(&mut Family<K>)) -> Self {
        declare(Arc::get_mut(&mut self.0).expect("declare feeds' targets before cloning them"));
        self
    }

    /// Declare a region, list, or component every feed of the family may update.
    pub fn affects(self, target: impl PushTarget) -> Self {
        let id = target.id().to_owned();
        self.declare(|family| family.targets.push(id))
    }

    /// Declare every keyed region and component of a kind; see [`Feed::affects_kind`].
    pub fn affects_kind(self, kind: &'static str) -> Self {
        assert_kind(kind);
        self.declare(|family| family.kinds.push(kind.to_owned()))
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
            self.0.targets.clone(),
            self.0.kinds.clone(),
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

/// A destination a feed may update: a [`VersionedRegion`], a [`List`], or a
/// versioned [`Component`]. Families of keyed ones use [`Feed::affects_kind`].
pub trait PushTarget: sealed::PushTarget {
    fn id(&self) -> &str;
}

mod sealed {
    pub trait PushTarget {}
    impl PushTarget for crate::VersionedRegion {}
    impl PushTarget for crate::List {}
    impl PushTarget for &crate::Component {}
}

impl PushTarget for VersionedRegion {
    fn id(&self) -> &str {
        self.id()
    }
}
impl PushTarget for List {
    fn id(&self) -> &str {
        self.id()
    }
}
impl PushTarget for &Component {
    fn id(&self) -> &str {
        Component::id(self)
    }
}

impl Feed {
    pub fn new(id: &'static str, path: &'static str) -> Self {
        Self::with(id.to_owned(), path.to_owned(), Vec::new(), Vec::new())
    }

    fn with(id: String, path: String, targets: Vec<String>, kinds: Vec<String>) -> Self {
        assert!(
            !id.is_empty() && !id.contains(char::is_whitespace),
            "a feed needs an id without whitespace"
        );
        assert!(path.starts_with('/'), "a feed's path starts with '/'");
        let (sender, _) = broadcast::channel(RECENT);
        Self(Arc::new(Inner {
            id,
            path,
            instance: crate::replay::new_key()[..12].to_owned(),
            targets,
            kinds,
            sender,
            state: Mutex::default(),
        }))
    }

    fn declare(mut self, declare: impl FnOnce(&mut Inner)) -> Self {
        declare(Arc::get_mut(&mut self.0).expect("declare a feed's targets before cloning it"));
        self
    }

    /// Declare a region, list, or component this feed may update.
    pub fn affects(self, target: impl PushTarget) -> Self {
        let id = target.id().to_owned();
        self.declare(|inner| inner.targets.push(id))
    }

    /// Declare every keyed region and component of a kind, such as each
    /// `VersionedRegion::keyed("task-summary", id)` or `Component::new("task", id)`.
    pub fn affects_kind(self, kind: &'static str) -> Self {
        assert_kind(kind);
        self.declare(|inner| inner.kinds.push(kind.to_owned()))
    }

    pub fn path(&self) -> &str {
        &self.0.path
    }

    /// The element that subscribes the page. It records the feed's position,
    /// so updates published after the page was rendered are not missed:
    /// render it under the same lock or transaction as the data it follows.
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
            targets: &self.0.targets,
            kinds: &self.0.kinds,
        };
        let config = serde_json::to_string(&config).expect("feed configuration serializes");
        html! { div id=(&self.0.id) hidden data-placebo-feed=(config) {} }
    }

    /// Start an update for every subscribed page.
    pub fn push(&self) -> Push<'_> {
        Push {
            feed: self,
            patches: Vec::new(),
        }
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

    fn declared(&self, id: &str) -> bool {
        self.0.targets.iter().any(|target| target == id)
            || id
                .split_once(':')
                .is_some_and(|(kind, _)| self.0.kinds.iter().any(|declared| declared == kind))
    }

    fn publish(&self, patches: Vec<Patch>) {
        let mut state = self.0.state.lock().unwrap();
        state.seq += 1;
        state.used = Instant::now();
        let update = Update {
            version: VERSION,
            feed: &self.0.id,
            patches,
        };
        let sent = Arc::new(Sent {
            seq: state.seq,
            data: serde_json::to_string(&update).expect("push update serializes"),
        });
        if state.recent.len() == RECENT {
            state.recent.pop_front();
        }
        state.recent.push_back(sent.clone());
        // No subscribers is not an error.
        let _ = self.0.sender.send(sent);
    }

    fn event(&self, sent: &Sent) -> Event {
        Event::default()
            .event("update")
            .id(format!("{}-{}", self.0.instance, sent.seq))
            .data(&sent.data)
    }

    fn idle(&self) -> bool {
        self.0.sender.receiver_count() == 0
            && self.0.state.lock().unwrap().used.elapsed() > IDLE
    }

    fn resync(&self, seq: u64) -> Event {
        Event::default()
            .event("resync")
            .id(format!("{}-{seq}", self.0.instance))
            .data(format!(
                r#"{{"version":{VERSION},"feed":{}}}"#,
                serde_json::to_string(&self.0.id).expect("a feed id serializes")
            ))
    }

    /// Answer a subscription with this feed's Server-Sent Events: replay
    /// what a reconnecting page missed, or tell it to resync, then follow new
    /// updates. [`Feed::route`] does this; call it from your own handler to
    /// choose the feed, such as the signed-in person's feed of [`Feeds`].
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
        let mut first = VecDeque::from([Event::default().comment("connected")]);
        let sent_up_to = {
            let mut state = self.0.state.lock().unwrap();
            state.used = Instant::now();
            let oldest = state.recent.front().map_or(state.seq + 1, |sent| sent.seq);
            let after = last.as_deref().and_then(|last| {
                let (instance, seq) = last.rsplit_once('-')?;
                (instance == self.0.instance)
                    .then(|| seq.parse::<u64>().ok())
                    .flatten()
            });
            match after {
                // Everything after it is still kept: replay it.
                Some(after) if after + 1 >= oldest && after <= state.seq => {
                    first.extend(
                        state
                            .recent
                            .iter()
                            .filter(|sent| sent.seq > after)
                            .map(|sent| self.event(sent)),
                    );
                }
                // No position: follow from now.
                None if last.is_none() => {}
                _ => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[placebo:push-resync] feed={} position={} is not among the recent \
                         updates; the page reads itself again.",
                        self.0.id,
                        last.as_deref().unwrap_or("none")
                    );
                    first.push_back(self.resync(state.seq));
                }
            }
            state.seq
        };
        let feed = self.clone();
        let events = stream::unfold(
            (first, receiver, sent_up_to),
            move |(mut first, mut receiver, sent_up_to)| {
                let feed = feed.clone();
                async move {
                    if let Some(event) = first.pop_front() {
                        return Some((Ok::<_, Infallible>(event), (first, receiver, sent_up_to)));
                    }
                    loop {
                        match receiver.recv().await {
                            // Already replayed from the recent updates.
                            Ok(sent) if sent.seq <= sent_up_to => continue,
                            Ok(sent) => {
                                let event = feed.event(&sent);
                                return Some((Ok(event), (first, receiver, sent.seq)));
                            }
                            // This page fell behind the channel: read the page again.
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                let seq = feed.0.state.lock().unwrap().seq;
                                return Some((Ok(feed.resync(seq)), (first, receiver, seq)));
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
    targets: &'a [String],
    kinds: &'a [String],
}

#[derive(Serialize)]
struct Update<'a> {
    version: u8,
    feed: &'a str,
    patches: Vec<Patch>,
}

/// One update for every page subscribed to a feed. Nothing is sent until
/// [`Push::send`].
#[must_use = "call .send() to publish the update"]
pub struct Push<'a> {
    feed: &'a Feed,
    patches: Vec<Patch>,
}

impl Push<'_> {
    /// Replace a declared versioned region, on pages whose revision is older.
    pub fn replace(mut self, region: VersionedRegion, revision: u64, content: Markup) -> Self {
        self.declared(region.id());
        self.patches.push(Patch {
            revision: Some(revision.to_string()),
            html: Some(content.into_string()),
            ..Patch::new(region.id(), "replace-children")
        });
        self
    }

    /// Refresh a declared component, on pages whose revision is older. The
    /// component needs a revision. Edited controls keep their edits, and a
    /// component with its own request in flight takes the update afterwards,
    /// if it is still newer.
    pub fn refresh(mut self, component: &Component, content: Markup) -> Self {
        self.declared(component.id());
        assert!(
            component.revision_value().is_some(),
            "a pushed component needs a revision: Component::new(..).revision(n)"
        );
        self.patches.push(Patch::refresh(component, content));
        self
    }

    /// Insert an item into a declared list. A page that already has it keeps it.
    pub fn insert_item(mut self, item: MountedItem, at: Position) -> Self {
        self.declared(&item.item.list);
        let position = at.wire(&item.item.list);
        self.patches.push(Patch {
            item: Some(item.item.id),
            position: Some(position),
            html: Some(item.markup.into_string()),
            ..Patch::new(&item.item.list, "insert-item")
        });
        self
    }

    pub fn move_item(mut self, item: &Item, to: Position) -> Self {
        self.declared(&item.list);
        self.patches.push(Patch {
            item: Some(item.id.clone()),
            position: Some(to.wire(&item.list)),
            ..Patch::new(&item.list, "move-item")
        });
        self
    }

    pub fn remove_item(mut self, item: &Item) -> Self {
        self.declared(&item.list);
        self.patches.push(Patch {
            item: Some(item.id.clone()),
            ..Patch::new(&item.list, "remove-item")
        });
        self
    }

    pub fn order_items(mut self, list: &List, items: impl IntoIterator<Item = Item>) -> Self {
        self.declared(list.id());
        let items = items
            .into_iter()
            .map(|item| {
                assert_eq!(
                    item.list,
                    list.id(),
                    "ordered items must belong to the list"
                );
                item.id
            })
            .collect();
        self.patches.push(Patch {
            items: Some(items),
            ..Patch::new(list.id(), "order-items")
        });
        self
    }

    /// Publish the update to every subscribed page.
    pub fn send(self) {
        if !self.patches.is_empty() {
            self.feed.publish(self.patches);
        }
    }

    /// The browser rejects an update for an undeclared target; catch it here
    /// in debug builds, before it is sent.
    fn declared(&self, id: &str) {
        debug_assert!(
            self.feed.declared(id),
            "'{id}' is not declared on feed '{}'. Declare it with .affects() or .affects_kind().",
            self.feed.0.id
        );
    }
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

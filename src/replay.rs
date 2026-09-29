//! Idempotent mutations. Every mutation form carries a fresh key, so a
//! submission sent twice (a retry after a lost response, a double click, a
//! form submitted again from the Back button) runs its handler once. The
//! second one gets the reply the first one recorded.
use axum::{
    Extension,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use maud::{DOCTYPE, html};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    hash::{BuildHasher, Hasher},
    pin::Pin,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use crate::{UPDATE_TYPE, native::NativeReply};

/// A mutation form's hidden field with its idempotency key.
pub(crate) const KEY: &str = "placebo-key";
/// Marks a response that the server replayed, or could not decide.
pub(crate) const REPLAY_HEADER: &str = "x-placebo-replay";

/// How long a repeated submission waits for the first one to finish.
const WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(25);

/// The future a [`ReplayStore`] method returns.
pub type StoreFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;

/// Where mutation replies are recorded, so a repeated submission replays the
/// first one's reply instead of running its handler again.
///
/// Placebo uses [`MemoryReplays`] unless the application installs another
/// store with [`replays`]. An application with several server processes, or
/// one whose writes must survive a restart, implements this trait on its
/// database: `claim` inserts the id as pending unless it exists, `record`
/// stores the reply, and `release` deletes the id. Expiring old rows is the
/// application's choice.
///
/// The claim and the application's write are separate transactions. If the
/// process stops after the write commits and before `record`, the id stays
/// pending, and a retry is told that the outcome is unknown rather than
/// running the write again.
pub trait ReplayStore: Send + Sync + 'static {
    /// Claim `id` for a new submission, or report what an earlier one left.
    fn claim<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Claim>;
    /// Store the reply of the submission that claimed `id`.
    fn record<'a>(&'a self, id: &'a str, reply: Recorded) -> StoreFuture<'a, ()>;
    /// Forget `id`: its handler answered with something that is not a reply,
    /// such as an error status, so a retry should run it again.
    fn release<'a>(&'a self, id: &'a str) -> StoreFuture<'a, ()>;
}

/// What [`ReplayStore::claim`] found.
#[derive(Clone, Debug)]
pub enum Claim {
    /// The id is new and now pending: run the handler.
    New,
    /// Another submission with this id is running, or stopped without a reply.
    Pending,
    /// A submission with this id finished with this reply.
    Recorded(Recorded),
}

/// A recorded reply: its HTTP status and update body.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub status: u16,
    pub body: String,
}

/// The store installed on a router, as a request extension.
#[derive(Clone)]
pub struct Replays(Arc<dyn ReplayStore>);

/// Record replies in `store` instead of the default [`MemoryReplays`]:
///
/// ```
/// use axum::{Router, routing::get};
/// let app: Router = Router::new()
///     .route("/", get(|| async { "Home" }))
///     .layer(placebo::replays(placebo::MemoryReplays::new(
///         std::time::Duration::from_secs(60),
///         1000,
///     )));
/// ```
pub fn replays(store: impl ReplayStore) -> Extension<Replays> {
    Extension(Replays(Arc::new(store)))
}

static DEFAULT: LazyLock<Replays> = LazyLock::new(|| Replays(Arc::new(MemoryReplays::default())));

pub(crate) fn store(extensions: &axum::http::Extensions) -> Arc<dyn ReplayStore> {
    extensions.get::<Replays>().unwrap_or(&DEFAULT).0.clone()
}

/// An in-memory [`ReplayStore`] for one server process. Entries expire after
/// `ttl`, and the oldest go first beyond `capacity`. The default keeps ten
/// minutes and 10,000 replies.
pub struct MemoryReplays {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
    replies: HashMap<String, Option<Recorded>>,
    order: VecDeque<(Instant, String)>,
}

impl Entries {
    fn evict_oldest(&mut self) {
        if let Some((_, id)) = self.order.pop_front() {
            self.replies.remove(&id);
        }
    }
}

impl MemoryReplays {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        assert!(capacity > 0, "a replay store needs room for one reply");
        Self {
            ttl,
            capacity,
            entries: Mutex::default(),
        }
    }
}

impl Default for MemoryReplays {
    fn default() -> Self {
        Self::new(Duration::from_secs(600), 10_000)
    }
}

impl ReplayStore for MemoryReplays {
    fn claim<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Claim> {
        Box::pin(async move {
            let mut entries = self.entries.lock().unwrap();
            let now = Instant::now();
            while entries
                .order
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) >= self.ttl)
            {
                entries.evict_oldest();
            }
            Ok(match entries.replies.get(id) {
                Some(Some(reply)) => Claim::Recorded(reply.clone()),
                Some(None) => Claim::Pending,
                None => {
                    while entries.order.len() >= self.capacity {
                        entries.evict_oldest();
                    }
                    entries.replies.insert(id.to_owned(), None);
                    entries.order.push_back((now, id.to_owned()));
                    Claim::New
                }
            })
        })
    }

    fn record<'a>(&'a self, id: &'a str, reply: Recorded) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            if let Some(entry) = self.entries.lock().unwrap().replies.get_mut(id) {
                *entry = Some(reply);
            }
            Ok(())
        })
    }

    fn release<'a>(&'a self, id: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let mut entries = self.entries.lock().unwrap();
            entries.replies.remove(id);
            entries.order.retain(|(_, entry)| entry != id);
            Ok(())
        })
    }
}

/// A fresh, unguessable key for one rendered form: 128 bits of SipHash, with
/// the process's random keys, over a counter.
pub(crate) fn new_key() -> String {
    static STATE: LazyLock<std::hash::RandomState> = LazyLock::new(std::hash::RandomState::new);
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let half = |salt: u64| {
        let mut hasher = STATE.build_hasher();
        hasher.write_u64(count);
        hasher.write_u64(salt);
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(1), half(2))
}

/// A submission's replay id: its key, and a fingerprint of the path, the
/// submitted body, and the credentials. A replay is only served to the same
/// submission from the same person; the same key with other values is a new
/// submission, for example a form edited again after the Back button.
pub(crate) fn submission_id(key: &str, path: &str, headers: &HeaderMap, parts: &[&[u8]]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(path.as_bytes());
    for name in [header::COOKIE, header::AUTHORIZATION] {
        for value in headers.get_all(&name) {
            feed(value.as_bytes());
        }
    }
    for part in parts {
        feed(part);
    }
    format!("{key}-{hash:016x}")
}

/// Claim a submission, waiting a little for a pending one to finish. `Err`
/// is the response to send instead of running the handler.
pub(crate) async fn claim(store: &dyn ReplayStore, id: &str) -> Result<(), Response> {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        match store.claim(id).await {
            Ok(Claim::New) => return Ok(()),
            Ok(Claim::Recorded(reply)) => return Err(replayed(reply)),
            Ok(Claim::Pending) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(POLL).await
            }
            Ok(Claim::Pending) => return Err(pending()),
            Err(error) => {
                eprintln!("[placebo:replay-store] Could not claim a submission: {error}");
                return Err(unavailable());
            }
        }
    }
}

/// The recorded reply, again. It becomes a navigation for a native submission.
fn replayed(reply: Recorded) -> Response {
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::OK);
    let native = serde_json::from_str::<serde_json::Value>(&reply.body)
        .ok()
        .map(|envelope| NativeReply {
            target: envelope["target"].as_str().unwrap_or_default().to_owned(),
            html: envelope["html"].as_str().unwrap_or_default().to_owned(),
            navigate: envelope["navigate"].as_str().map(str::to_owned),
        });
    let mut response = (
        status,
        [
            (header::CONTENT_TYPE, UPDATE_TYPE),
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static(REPLAY_HEADER), "replayed"),
        ],
        reply.body,
    )
        .into_response();
    if let Some(native) = native {
        response.extensions_mut().insert(native);
    }
    response
}

fn page(status: StatusCode, marker: &'static str, title: &str, explanation: &str) -> Response {
    let mut response = (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        html! {
            (DOCTYPE)
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { (title) }
                }
                body style="font: 1.1rem/1.5 system-ui, sans-serif; max-width: 36rem; margin: 3rem auto; padding: 0 1rem" {
                    h1 { (title) }
                    p { (explanation) }
                }
            }
        },
    )
        .into_response();
    response.headers_mut().insert(
        header::HeaderName::from_static(REPLAY_HEADER),
        HeaderValue::from_static(marker),
    );
    response
}

/// The first submission with this key has not finished: it is slow, or it
/// stopped after possibly writing. Running the handler again could write twice.
fn pending() -> Response {
    page(
        StatusCode::CONFLICT,
        "pending",
        "Still saving",
        "This form was already sent and the server has not finished with it. \
         Wait a moment and reload the page to see whether it was saved.",
    )
}

fn unavailable() -> Response {
    page(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        "Your changes were not saved",
        "The server could not check whether this form was already sent, so it \
         did not save it. Go back and submit again.",
    )
}

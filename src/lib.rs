//! Experimental HTML updates: a region, a read action, and a shared protocol.
//!
//! The same action emits form configuration and addresses its response. This
//! Input-derived builders connect form controls and handler payloads. Region
//! existence and the contents of browser requests still need runtime checks.
//!
//! Start from `examples/quickstart.rs` in the repository.
#![doc = include_str!("../docs/rules.md")]

extern crate self as placebo;

use axum::{
    handler::Handler,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use maud::{Markup, html};
use serde::Serialize;
use std::{borrow::Cow, marker::PhantomData};

mod forms;
#[doc(hidden)]
pub use forms::private as __private;
pub use forms::{
    Control, FieldValue, FormEnum, FormFields, FormInput, FormValue, Input, NumberValue,
    SingleValue, TextValue,
};
/// Render a typed form body using Maud markup and `@field name = control;` entries.
/// See [`FormInput`] for examples and compile-time guarantees.
pub use placebo_macros::fields;
pub use placebo_macros::{FormEnum, FormInput};

mod component;
mod diagnostics;
mod native;
mod push;
mod replay;
mod upload;
pub use component::{
    Component, MountedComponent, MutationAction, MutationBinding, MutationRequest,
};
pub use native::native_forms;
pub use push::{Feed, Push, PushTarget};
pub use replay::{Claim, MemoryReplays, Recorded, ReplayStore, Replays, StoreFuture, replays};
pub use upload::{DEFAULT_MAX_BYTES, FileValue, Upload};

#[cfg(all(feature = "dev", debug_assertions))]
pub mod dev;

pub const VERSION: u8 = 5;
pub const UPDATE_TYPE: &str = "application/vnd.placebo.update+json";
pub const RUNTIME: &str = include_str!("../client/placebo.js");

/// A stable instance name shared by the declaring view and its action.
/// Each mounted instance must have a unique id, including repeated components.
#[derive(Clone, Debug)]
pub struct Region(Cow<'static, str>);

impl Region {
    pub const fn new(id: &'static str) -> Self {
        assert!(!id.is_empty(), "a region needs a nonempty id");
        let bytes = id.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            // HTML ids cannot contain ASCII whitespace.
            assert!(
                !bytes[i].is_ascii_whitespace(),
                "a region id cannot contain whitespace"
            );
            i += 1;
        }
        Self(Cow::Borrowed(id))
    }

    /// An instance-specific region, such as a summary next to an editor.
    pub fn keyed(kind: &str, key: impl std::fmt::Display) -> Self {
        Self(Cow::Owned(Component::new(kind, key).id().to_owned()))
    }

    pub fn id(&self) -> &str {
        &self.0
    }

    /// Updates replace only this element's children. Keep persistent inputs
    /// outside it; this prototype does not morph or restore replaced nodes.
    pub fn mount(&self, content: Markup) -> Markup {
        html! { div id=(self.id()) data-placebo-region { (content) } }
    }
}

/// A shared server snapshot, such as counts updated by several editors.
/// Mounting always requires a revision; only this type supports `also_replace`.
/// Revisions must increase with the corresponding server data under the same
/// transaction/lock. Types cannot prove that a region exists in the actual DOM
/// or that a revision reflects the data it accompanies.
///
/// ```
/// use placebo::VersionedRegion;
/// use maud::html;
/// const COUNTS: VersionedRegion = VersionedRegion::new("counts");
/// let initial = COUNTS.mount(1, html! { "3 items" });
/// assert!(initial.into_string().contains("data-placebo-revision=\"1\""));
/// ```
///
/// A plain region cannot receive a versioned snapshot:
/// ```compile_fail
/// use placebo::{Component, FormInput, MutationAction, Region};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// let component = Component::new("editor", 1);
/// MutationAction::<Save>::new("save", "/save").bind(&component)
///     .reply(html! {}).also_replace(Region::new("counts"), 1, html! {});
/// ```
///
/// A versioned region cannot be mounted without its revision:
/// ```compile_fail
/// use placebo::VersionedRegion;
/// use maud::html;
/// VersionedRegion::new("counts").mount(html! { "3 items" });
/// ```
#[derive(Clone, Debug)]
pub struct VersionedRegion(Region);

impl VersionedRegion {
    pub const fn new(id: &'static str) -> Self {
        Self(Region::new(id))
    }

    pub fn keyed(kind: &str, key: impl std::fmt::Display) -> Self {
        Self(Region::keyed(kind, key))
    }

    pub fn id(&self) -> &str {
        self.0.id()
    }

    pub fn mount(&self, revision: u64, content: Markup) -> Markup {
        html! { div id=(self.id()) data-placebo-region data-placebo-revision=(revision) { (content) } }
    }
}

/// A keyed collection whose items can be inserted, moved, and removed without
/// replacing the others. Items keep their DOM nodes, so drafts, focus, and
/// behaviors inside them survive every list update.
///
/// ```
/// use placebo::{List, Position};
/// use maud::html;
/// const TASKS: List = List::new("tasks");
/// let page = TASKS.mount(html! {
///     @for id in [1, 2] { (TASKS.item(id).mount(html! { "Task " (id) })) }
/// });
/// assert!(page.into_string().contains("id=\"tasks/2\" data-placebo-item"));
/// let _ = Position::Before(TASKS.item(2));
/// ```
#[derive(Clone, Debug)]
pub struct List(Region);

impl List {
    pub const fn new(id: &'static str) -> Self {
        Self(Region::new(id))
    }

    pub fn keyed(kind: &str, key: impl std::fmt::Display) -> Self {
        Self(Region::keyed(kind, key))
    }

    pub fn id(&self) -> &str {
        self.0.id()
    }

    /// Mount the list with its initial items, rendered with [`Item::mount`].
    pub fn mount(&self, items: Markup) -> Markup {
        html! { div id=(self.id()) data-placebo-region data-placebo-list { (items) } }
    }

    /// The item with this key. Its element id is `{list}/{key}`.
    pub fn item(&self, key: impl std::fmt::Display) -> Item {
        let key = key.to_string();
        assert!(
            !key.is_empty() && !key.chars().any(char::is_whitespace),
            "an item key must be nonempty and contain no whitespace"
        );
        Item {
            list: self.id().to_owned(),
            id: format!("{}/{key}", self.id()),
        }
    }
}

/// One item of a [`List`], addressed by its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    list: String,
    id: String,
}

impl Item {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn mount(&self, content: impl maud::Render) -> MountedItem {
        MountedItem {
            item: self.clone(),
            markup: html! { div id=(self.id) data-placebo-item { (content) } },
        }
    }
}

/// A rendered item, for a list's initial markup or `Applied::also_insert`.
pub struct MountedItem {
    item: Item,
    markup: Markup,
}

impl maud::Render for MountedItem {
    fn render_to(&self, buffer: &mut String) {
        self.markup.render_to(buffer);
    }
}

/// Where `also_insert` and `also_move` place an item. An anchor that is no
/// longer in the browser's list places the item at the end, and the applied
/// event reports it in `misplacedItems`.
#[derive(Clone, Debug)]
pub enum Position {
    Start,
    End,
    Before(Item),
    After(Item),
}

impl Position {
    fn wire(&self, list: &str) -> WirePosition {
        let (at, item) = match self {
            Self::Start => ("start", None),
            Self::End => ("end", None),
            Self::Before(item) => ("before", Some(item)),
            Self::After(item) => ("after", Some(item)),
        };
        if let Some(item) = item {
            assert_eq!(
                item.list, list,
                "a position anchor must be an item of the same list"
            );
        }
        WirePosition {
            at,
            item: item.map(|item| item.id.clone()),
        }
    }
}

mod sealed {
    pub trait RegionTarget {}
    impl RegionTarget for super::Region {}
    impl RegionTarget for super::VersionedRegion {}
    impl RegionTarget for super::List {}
    impl RegionTarget for &super::Component {}
}

/// A destination accepted by `MutationBinding::affects`: a plain [`Region`]
/// for appends and refetches, a [`VersionedRegion`] for shared snapshots, a
/// [`List`] for item updates, or another [`Component`] to refresh.
/// The operation-specific reply methods retain their stricter target types.
pub trait RegionTarget: sealed::RegionTarget {
    fn id(&self) -> &str;
}

impl RegionTarget for List {
    fn id(&self) -> &str {
        self.id()
    }
}

impl RegionTarget for &Component {
    fn id(&self) -> &str {
        Component::id(self)
    }
}

impl RegionTarget for Region {
    fn id(&self) -> &str {
        self.id()
    }
}

impl RegionTarget for VersionedRegion {
    fn id(&self) -> &str {
        self.id()
    }
}

/// The extractor list of an action handler: any Axum extractors, then
/// [`Input<I>`] last. `action.route(handler)` requires it, so a handler for
/// another payload, or without one, does not compile.
///
/// ```compile_fail
/// use placebo::{FormInput, Input, MutationAction};
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// #[derive(serde::Deserialize, FormInput)]
/// struct Other { name: String }
/// async fn save(Input(_): Input<Other>) {}
/// let route: axum::routing::MethodRouter<()> =
///     MutationAction::<Save>::new("save", "/save").route(save);
/// ```
#[diagnostic::on_unimplemented(
    message = "an action handler must take `placebo::Input<{I}>` as its last argument",
    label = "handler does not end with `Input<{I}>`",
    note = "other Axum extractors, such as `State` or a session, go before the input"
)]
pub trait EndsWithInput<I> {}

// Axum's handler extractor tuples are `(M, T1, ..., Tn)`, with up to 16 extractors.
macro_rules! ends_with_input {
    ($($ty:ident),*) => {
        impl<M, $($ty,)* I> EndsWithInput<I> for (M, $($ty,)* Input<I>,) {}
    };
}
ends_with_input!();
ends_with_input!(T1);
ends_with_input!(T1, T2);
ends_with_input!(T1, T2, T3);
ends_with_input!(T1, T2, T3, T4);
ends_with_input!(T1, T2, T3, T4, T5);
ends_with_input!(T1, T2, T3, T4, T5, T6);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
ends_with_input!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
ends_with_input!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);

/// A GET-only action with latest-request-wins scheduling per mounted target.
/// Aborting a request is NOT a way to undo server writes: this API is for reads.
pub struct ReadAction<I: FormInput> {
    name: &'static str,
    path: &'static str,
    input: PhantomData<fn() -> I>,
}

impl<I: FormInput> Copy for ReadAction<I> {}
impl<I: FormInput> Clone for ReadAction<I> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I: FormInput> ReadAction<I> {
    pub const fn new(name: &'static str, path: &'static str) -> Self {
        assert!(!name.is_empty(), "an action needs a name");
        assert!(!path.is_empty(), "an action needs a path");
        assert!(
            !I::MULTIPART,
            "a read submits its fields in the URL, so its payload cannot have an Upload field"
        );
        Self {
            name,
            path,
            input: PhantomData,
        }
    }

    pub const fn path(self) -> &'static str {
        self.path
    }

    /// The endpoint is reusable; a view chooses its target and input behavior.
    pub const fn bind(self, target: Region) -> ReadBinding<I> {
        ReadBinding {
            action: self,
            target,
            input_delay_ms: None,
            history: false,
            load: false,
            reveal: false,
            every_ms: None,
            effects: Vec::new(),
        }
    }

    /// Registers a GET handler. It may take any Axum extractors, and must end
    /// with [`Input`] of this action's payload type.
    pub fn route<H, T, S>(self, handler: H) -> axum::routing::MethodRouter<S>
    where
        H: Handler<T, S>,
        T: EndsWithInput<I> + 'static,
        S: Clone + Send + Sync + 'static,
    {
        axum::routing::get(handler).layer(axum::middleware::from_fn_with_state(
            self.name,
            diagnostics::request,
        ))
    }
}

pub struct ReadBinding<I: FormInput> {
    action: ReadAction<I>,
    target: Region,
    input_delay_ms: Option<u32>,
    history: bool,
    load: bool,
    reveal: bool,
    every_ms: Option<u32>,
    effects: Vec<String>,
}

impl<I: FormInput> Clone for ReadBinding<I> {
    fn clone(&self) -> Self {
        Self {
            action: self.action,
            target: self.target.clone(),
            input_delay_ms: self.input_delay_ms,
            history: self.history,
            load: self.load,
            reveal: self.reveal,
            every_ms: self.every_ms,
            effects: self.effects.clone(),
        }
    }
}

impl<I: FormInput> ReadBinding<I> {
    /// A new input immediately invalidates older work, before this delay runs.
    pub const fn on_input(mut self, delay_ms: u32) -> Self {
        assert!(delay_ms <= 60_000, "input delay exceeds one minute");
        self.input_delay_ms = Some(delay_ms);
        self
    }

    /// Keep the page URL in step with this form. Each result puts the form's
    /// query on the current page's path: typing replaces the history entry,
    /// a submit pushes a new one, and Back or Forward puts that entry's values
    /// back into the form and reads again. Render the page from the same query,
    /// for example with `Input<I>` in the page handler, so reloads and
    /// bookmarks show the same results.
    pub const fn history(mut self) -> Self {
        self.history = true;
        self
    }

    /// Read once as soon as the form is on the page: a section rendered
    /// without its slow contents, filled in after the page shows.
    pub const fn on_load(mut self) -> Self {
        self.load = true;
        self
    }

    /// Read once when the form scrolls near the viewport: a section below the
    /// fold, or the "load more" form at the end of a list. A form inside its
    /// own region is replaced by the reply, so the next page's form starts
    /// watching again.
    pub const fn on_reveal(mut self) -> Self {
        self.reveal = true;
        self
    }

    /// Read again every `interval_ms` while the form and its region are on the
    /// page. Polling pauses while the page is hidden and reads once when it is
    /// shown again. For changes caused by writes in this app, a [`Feed`]
    /// updates pages without asking.
    pub const fn every(mut self, interval_ms: u32) -> Self {
        assert!(
            interval_ms >= 500 && interval_ms <= 86_400_000,
            "poll between every 500 ms and once a day"
        );
        self.every_ms = Some(interval_ms);
        self
    }

    /// Declare a list this read's replies may insert items into, such as the
    /// list an infinite "load more" form extends. Reads insert; they do not
    /// move or remove.
    pub fn affects(mut self, list: List) -> Self {
        assert!(!self.effects.iter().any(|id| id == list.id()));
        self.effects.push(list.id().to_owned());
        self
    }

    pub fn form(&self, fields: FormFields<I>) -> Markup {
        let content = fields.into_markup();
        let config = Config {
            version: VERSION,
            action: self.action.name,
            target: self.target.id(),
            policy: "latest",
            operation: "replace-children",
            input_delay_ms: self.input_delay_ms,
            history: self.history,
            load: self.load,
            reveal: self.reveal,
            every_ms: self.every_ms,
            effects: self.effects.iter().map(String::as_str).collect(),
        };
        let config = serde_json::to_string(&config).expect("static configuration serializes");
        html! {
            form method="get" action=(self.action.path) data-placebo=(config) { (content) }
        }
    }

    pub fn reply(&self, content: Markup) -> ReadUpdate {
        ReadUpdate {
            envelope: Envelope::new(
                self.action.name,
                self.target.id(),
                "replace-children",
                StatusCode::OK,
                content,
            ),
            effects: self.effects.clone(),
        }
    }
}

#[derive(Serialize)]
struct Config<'a> {
    version: u8,
    action: &'static str,
    target: &'a str,
    policy: &'static str,
    operation: &'static str,
    input_delay_ms: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    history: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    load: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    reveal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    every_ms: Option<u32>,
    effects: Vec<&'a str>,
}

/// The serialized update. The public reply types decide which additions apply.
#[derive(Serialize)]
struct Envelope {
    version: u8,
    action: &'static str,
    target: String,
    operation: &'static str,
    html: String,
    /// The target component's revision, for a versioned component.
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<String>,
    outcome: &'static str,
    patches: Vec<Patch>,
    navigate: Option<String>,
    #[serde(skip)]
    status: StatusCode,
}

impl Envelope {
    fn new(
        action: &'static str,
        target: &str,
        operation: &'static str,
        status: StatusCode,
        content: Markup,
    ) -> Self {
        Self {
            version: VERSION,
            action,
            target: target.into(),
            operation,
            html: content.into_string(),
            revision: None,
            status,
            outcome: match status {
                StatusCode::UNPROCESSABLE_ENTITY => "invalid",
                StatusCode::CONFLICT => "conflict",
                _ => "applied",
            },
            patches: Vec::new(),
            navigate: None,
        }
    }
}

impl IntoResponse for Envelope {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            [
                (header::CONTENT_TYPE, UPDATE_TYPE),
                (header::CACHE_CONTROL, "no-store"),
            ],
            serde_json::to_string(&self).expect("HTML update serializes"),
        )
            .into_response();
        // A native submission's adapter turns the reply into a navigation.
        response.extensions_mut().insert(native::NativeReply {
            target: self.target,
            html: self.html,
            navigate: self.navigate,
        });
        response
    }
}

/// A read reply. It replaces its region's children, and can insert items
/// into lists its binding declares.
///
/// Reads cannot patch other regions:
/// ```compile_fail
/// use placebo::{FormInput, ReadAction, Region};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Search { q: String }
/// ReadAction::<Search>::new("search", "/search").bind(Region::new("results"))
///     .reply(html! {}).also_append(Region::new("list"), html! {});
/// ```
pub struct ReadUpdate {
    envelope: Envelope,
    effects: Vec<String>,
}

impl ReadUpdate {
    /// Insert an item into a list the binding declares with `affects`, such
    /// as the next page of an infinite list. An item already in the list keeps
    /// its node.
    pub fn also_insert(mut self, item: MountedItem, at: Position) -> Self {
        declared(&self.effects, &item.item.list);
        let position = at.wire(&item.item.list);
        self.envelope.patches.push(Patch {
            item: Some(item.item.id),
            position: Some(position),
            html: Some(item.markup.into_string()),
            ..Patch::new(&item.item.list, "insert-item")
        });
        self
    }
}

impl IntoResponse for ReadUpdate {
    fn into_response(self) -> Response {
        self.envelope.into_response()
    }
}

/// A successful mutation reply, from `MutationBinding::reply`. Only a
/// successful write can add, move, or remove items, refresh other components,
/// or navigate. Controls the user submitted take the reply's values unless
/// they were edited again while the request was in flight.
pub struct Applied {
    envelope: Envelope,
    effects: Vec<String>,
}

impl Applied {
    /// Refresh a declared shared region only when its server revision is newer.
    /// `VersionedRegion::mount` requires the corresponding initial revision.
    pub fn also_replace(mut self, region: VersionedRegion, revision: u64, content: Markup) -> Self {
        replace(&mut self.envelope, &self.effects, region, revision, content);
        self
    }

    /// Append new content without replacing existing component instances.
    /// Delivery is not retried; existing/duplicate ids cause rejection.
    pub fn also_append(self, region: Region, content: impl maud::Render) -> Self {
        declared(&self.effects, region.id());
        self.push(Patch {
            html: Some(content.render().into_string()),
            ..Patch::new(region.id(), "append-children")
        })
    }

    /// Insert a new item into its declared list. An item whose id is already on
    /// the page rejects the update; move existing items with `also_move`.
    pub fn also_insert(self, item: MountedItem, at: Position) -> Self {
        declared(&self.effects, &item.item.list);
        let position = at.wire(&item.item.list);
        let patch = Patch {
            item: Some(item.item.id),
            position: Some(position),
            html: Some(item.markup.into_string()),
            ..Patch::new(&item.item.list, "insert-item")
        };
        self.push(patch)
    }

    /// Move an existing item. A missing item is skipped and reported.
    pub fn also_move(self, item: &Item, to: Position) -> Self {
        declared(&self.effects, &item.list);
        self.push(Patch {
            item: Some(item.id.clone()),
            position: Some(to.wire(&item.list)),
            ..Patch::new(&item.list, "move-item")
        })
    }

    /// Remove an item, including any components inside it. Removing an item
    /// that is already gone is not an error. Focus inside it moves to a neighbour.
    pub fn also_remove(self, item: &Item) -> Self {
        declared(&self.effects, &item.list);
        self.push(Patch {
            item: Some(item.id.clone()),
            ..Patch::new(&item.list, "remove-item")
        })
    }

    /// Reorder existing items: the listed ones first, in this order, then any
    /// the browser has that the server did not list.
    pub fn also_order(self, list: &List, items: impl IntoIterator<Item = Item>) -> Self {
        declared(&self.effects, list.id());
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
        self.push(Patch {
            items: Some(items),
            ..Patch::new(list.id(), "order-items")
        })
    }

    /// Refresh another declared component, such as an editor this write
    /// locked. Its controls follow the same rules as a rejected reply: edited
    /// ones are kept. Skipped while that component has its own request in flight.
    pub fn also_refresh(self, component: &Component, content: Markup) -> Self {
        declared(&self.effects, component.id());
        self.push(Patch::refresh(component, content))
    }

    /// Run the read form bound to a declared region again, with the browser's
    /// current input, so the result respects its filters and sort order.
    pub fn also_refetch(self, region: &Region) -> Self {
        declared(&self.effects, region.id());
        self.push(Patch::new(region.id(), "rerun-read"))
    }

    /// Navigate to a same-site path after applying the reply, for example to a
    /// record that was just created or away from one that was deleted.
    pub fn navigate(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        assert!(
            path.starts_with('/') && !path.starts_with("//"),
            "navigate to a path on this site, starting with a single '/'"
        );
        self.envelope.navigate = Some(path);
        self
    }

    fn push(mut self, patch: Patch) -> Self {
        self.envelope.patches.push(patch);
        self
    }
}

impl IntoResponse for Applied {
    fn into_response(self) -> Response {
        self.envelope.into_response()
    }
}

/// A validation or conflict reply, from `MutationBinding::invalid` or
/// `conflict`. It can refresh shared snapshots. Controls with edits keep them;
/// untouched controls show the reply's values. It adds, removes, and navigates nothing.
///
/// ```compile_fail
/// use placebo::{Component, FormInput, MutationAction};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// MutationAction::<Save>::new("save", "/save").bind(&Component::new("editor", 1))
///     .invalid(html! {}).navigate("/");
/// ```
pub struct Rejected {
    envelope: Envelope,
    effects: Vec<String>,
}

impl Rejected {
    /// Refresh a declared shared region only when its server revision is newer.
    pub fn also_replace(mut self, region: VersionedRegion, revision: u64, content: Markup) -> Self {
        replace(&mut self.envelope, &self.effects, region, revision, content);
        self
    }
}

impl IntoResponse for Rejected {
    fn into_response(self) -> Response {
        self.envelope.into_response()
    }
}

fn replace(
    envelope: &mut Envelope,
    effects: &[String],
    region: VersionedRegion,
    revision: u64,
    content: Markup,
) {
    declared(effects, region.id());
    envelope.patches.push(Patch {
        revision: Some(revision.to_string()),
        html: Some(content.into_string()),
        ..Patch::new(region.id(), "replace-children")
    });
}

/// The browser rejects patches its form did not declare. Checking the reply's
/// own binding catches a handler that builds a different binding from the view.
fn declared(effects: &[String], id: &str) {
    debug_assert!(
        effects.iter().any(|effect| effect == id),
        "region '{id}' is not declared with .affects() on this reply's binding. \
         Build the view's form and the handler's reply from one binding function."
    );
}

#[derive(Serialize)]
struct Patch {
    target: String,
    operation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<WirePosition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<String>>,
}

impl Patch {
    fn refresh(component: &Component, content: Markup) -> Self {
        Self {
            html: Some(content.into_string()),
            revision: component
                .revision_value()
                .map(|revision| revision.to_string()),
            ..Patch::new(component.id(), "refresh-component")
        }
    }

    fn new(target: &str, operation: &'static str) -> Self {
        Self {
            target: target.into(),
            operation,
            revision: None,
            html: None,
            item: None,
            position: None,
            items: None,
        }
    }
}

#[derive(Serialize)]
struct WirePosition {
    at: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<String>,
}

/// Serve the exact browser half embedded in this crate, with no independent
/// dependency resolution. Version mismatches still fail visibly at runtime.
pub async fn runtime() -> Response {
    #[cfg(all(feature = "dev", debug_assertions))]
    let content =
        match std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/client/placebo.js")) {
            Ok(content) => content,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not read the development runtime.",
                )
                    .into_response();
            }
        };
    #[cfg(not(all(feature = "dev", debug_assertions)))]
    let content = RUNTIME;
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        content,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize, FormInput)]
    struct Search {
        q: String,
    }

    #[test]
    fn response_and_view_share_the_target_and_escape_untrusted_text() {
        let region = Region::new("results");
        let action = ReadAction::<Search>::new("search", "/search").bind(region.clone());
        let update = serde_json::to_value(
            action
                .reply(html! { p { "<script>bad()</script>" } })
                .envelope,
        )
        .unwrap();
        assert_eq!(update["target"], region.id());
        assert_eq!(update["html"], "<p>&lt;script&gt;bad()&lt;/script&gt;</p>");
        let form = action
            .form(Search::fields().with_q(Control::search("rust")).finish())
            .into_string();
        assert!(form.contains("&quot;target&quot;:&quot;results&quot;"));
        let input: Search = serde_json::from_str(r#"{"q":"rust"}"#).unwrap();
        assert_eq!(input.q, "rust");
    }

    #[test]
    fn update_uses_the_protocol_media_type() {
        let response = ReadAction::<Search>::new("search", "/search")
            .bind(Region::new("results"))
            .reply(html! {})
            .into_response();
        assert_eq!(response.headers()[header::CONTENT_TYPE], UPDATE_TYPE);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn coordinated_updates_keep_exact_server_revisions() {
        let component = Component::new("editor", 42);
        let summary = VersionedRegion::keyed("summary", 42);
        let list = Region::new("list");
        let binding = MutationAction::<Search>::new("save", "/save")
            .bind(&component)
            .affects(summary.clone())
            .affects(list.clone());
        let form = binding
            .form(Search::fields().with_q(Control::text("draft")).finish())
            .into_string();
        assert!(form.contains("&quot;effects&quot;:[&quot;summary:42&quot;,&quot;list&quot;]"));
        let update = serde_json::to_value(
            binding
                .reply(html! { p { "Saved" } })
                .also_replace(summary.clone(), u64::MAX, html! { p { "<new>" } })
                .also_append(list, html! { p { "Next" } })
                .envelope,
        )
        .unwrap();
        assert_eq!(update["target"], component.id());
        assert_eq!(update["navigate"], serde_json::Value::Null);
        assert_eq!(update["patches"][0]["target"], summary.id());
        assert_eq!(update["patches"][0]["revision"], u64::MAX.to_string());
        assert_eq!(update["patches"][0]["html"], "<p>&lt;new&gt;</p>");
        assert_eq!(update["patches"][1]["operation"], "append-children");
        assert!(
            summary
                .mount(u64::MAX, html! {})
                .into_string()
                .contains(&u64::MAX.to_string())
        );
    }

    #[test]
    fn list_updates_address_items_and_other_targets_by_id() {
        let tasks = List::new("tasks");
        let other = Component::new("order", 1);
        let results = Region::new("results");
        let binding = MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .affects(tasks.clone())
            .affects(&other)
            .affects(results.clone());
        let update = serde_json::to_value(
            binding
                .reply(html! {})
                .also_insert(
                    tasks.item(3).mount(html! { "Three" }),
                    Position::Before(tasks.item(1)),
                )
                .also_move(&tasks.item(2), Position::Start)
                .also_remove(&tasks.item(1))
                .also_order(&tasks, [tasks.item(2), tasks.item(3)])
                .also_refresh(&other, html! { "Order" })
                .also_refetch(&results)
                .navigate("/tasks/3")
                .envelope,
        )
        .unwrap();
        let patches = &update["patches"];
        assert_eq!(patches[0]["operation"], "insert-item");
        assert_eq!(patches[0]["item"], "tasks/3");
        assert_eq!(
            patches[0]["position"],
            serde_json::json!({ "at": "before", "item": "tasks/1" })
        );
        assert_eq!(
            patches[0]["html"],
            "<div id=\"tasks/3\" data-placebo-item>Three</div>"
        );
        assert_eq!(patches[1]["position"], serde_json::json!({ "at": "start" }));
        assert_eq!(
            patches[2],
            serde_json::json!({ "target": "tasks", "operation": "remove-item", "item": "tasks/1" })
        );
        assert_eq!(
            patches[3]["items"],
            serde_json::json!(["tasks/2", "tasks/3"])
        );
        assert_eq!(patches[4]["target"], "order:1");
        assert_eq!(
            patches[5],
            serde_json::json!({ "target": "results", "operation": "rerun-read" })
        );
        assert_eq!(update["navigate"], "/tasks/3");
    }

    #[test]
    #[should_panic(expected = "same list")]
    fn positions_anchor_to_the_same_list() {
        let tasks = List::new("tasks");
        MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .affects(tasks.clone())
            .reply(html! {})
            .also_move(&tasks.item(1), Position::After(List::new("other").item(2)));
    }

    #[test]
    #[should_panic(expected = "single '/'")]
    fn replies_navigate_only_within_the_site() {
        MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .reply(html! {})
            .navigate("//example.com/");
    }

    #[test]
    #[should_panic(expected = "not declared with .affects()")]
    #[cfg(debug_assertions)]
    fn replies_patch_only_regions_their_binding_declares() {
        MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .reply(html! {})
            .also_replace(VersionedRegion::new("counts"), 1, html! {});
    }

    #[test]
    #[should_panic(expected = "whitespace")]
    fn region_ids_cannot_contain_whitespace() {
        Region::new("my region");
    }
}

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
pub use component::{
    Component, MountedComponent, MutationAction, MutationBinding, MutationRequest,
};

#[cfg(all(feature = "dev", debug_assertions))]
pub mod dev;

pub const VERSION: u8 = 3;
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

mod sealed {
    pub trait RegionTarget {}
    impl RegionTarget for super::Region {}
    impl RegionTarget for super::VersionedRegion {}
}

/// A destination accepted by `MutationBinding::affects`: either a plain
/// [`Region`] for appends, or a [`VersionedRegion`] for shared snapshots.
/// The operation-specific reply methods retain their stricter target types.
pub trait RegionTarget: sealed::RegionTarget {
    fn id(&self) -> &str;
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
}

impl<I: FormInput> Clone for ReadBinding<I> {
    fn clone(&self) -> Self {
        Self {
            action: self.action,
            target: self.target.clone(),
            input_delay_ms: self.input_delay_ms,
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

    pub fn form(&self, fields: FormFields<I>) -> Markup {
        let content = fields.into_markup();
        let config = Config {
            version: VERSION,
            action: self.action.name,
            target: self.target.id(),
            policy: "latest",
            operation: "replace-children",
            input_delay_ms: self.input_delay_ms,
            effects: Vec::new(),
        };
        let config = serde_json::to_string(&config).expect("static configuration serializes");
        html! {
            form method="get" action=(self.action.path) data-placebo=(config) { (content) }
        }
    }

    pub fn reply(&self, content: Markup) -> ReadUpdate {
        ReadUpdate(Envelope::new(
            self.action.name,
            self.target.id(),
            "replace-children",
            StatusCode::OK,
            content,
        ))
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
    outcome: &'static str,
    reset_local: Vec<String>,
    patches: Vec<Patch>,
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
            status,
            outcome: match status {
                StatusCode::UNPROCESSABLE_ENTITY => "invalid",
                StatusCode::CONFLICT => "conflict",
                _ => "applied",
            },
            reset_local: Vec::new(),
            patches: Vec::new(),
        }
    }
}

impl IntoResponse for Envelope {
    fn into_response(self) -> Response {
        (
            self.status,
            [
                (header::CONTENT_TYPE, UPDATE_TYPE),
                (header::CACHE_CONTROL, "no-store"),
            ],
            serde_json::to_string(&self).expect("HTML update serializes"),
        )
            .into_response()
    }
}

/// A read reply. It replaces its region's children and nothing else.
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
pub struct ReadUpdate(Envelope);

impl IntoResponse for ReadUpdate {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

/// A successful mutation reply, from `MutationBinding::reply`. Only a
/// successful write can reset drafts or append new instances.
pub struct Applied {
    envelope: Envelope,
    effects: Vec<String>,
}

impl Applied {
    /// Accept incoming local markup only if it has not changed since submission.
    /// Newer browser edits win.
    pub fn reset_local(mut self, key: &str) -> Self {
        let resets = &mut self.envelope.reset_local;
        assert!(!key.is_empty() && !resets.iter().any(|k| k == key));
        resets.push(key.into());
        self
    }

    /// Refresh a declared shared region only when its server revision is newer.
    /// `VersionedRegion::mount` requires the corresponding initial revision.
    pub fn also_replace(mut self, region: VersionedRegion, revision: u64, content: Markup) -> Self {
        replace(&mut self.envelope, &self.effects, region, revision, content);
        self
    }

    /// Append new content without replacing existing component instances.
    /// Delivery is not retried; existing/duplicate ids cause rejection.
    pub fn also_append(mut self, region: Region, content: impl maud::Render) -> Self {
        declared(&self.effects, region.id());
        self.envelope.patches.push(Patch {
            target: region.id().into(),
            operation: "append-children",
            revision: None,
            html: content.render().into_string(),
        });
        self
    }
}

impl IntoResponse for Applied {
    fn into_response(self) -> Response {
        self.envelope.into_response()
    }
}

/// A validation or conflict reply, from `MutationBinding::invalid` or
/// `conflict`. It can refresh shared snapshots, but keeps drafts and adds nothing.
///
/// ```compile_fail
/// use placebo::{Component, FormInput, MutationAction};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// MutationAction::<Save>::new("save", "/save").bind(&Component::new("editor", 1))
///     .invalid(html! {}).reset_local("draft");
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
        target: region.id().into(),
        operation: "replace-children",
        revision: Some(revision.to_string()),
        html: content.into_string(),
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
    revision: Option<String>,
    html: String,
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
        let update =
            serde_json::to_value(action.reply(html! { p { "<script>bad()</script>" } }).0).unwrap();
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
    fn coordinated_updates_keep_exact_server_revisions_and_explicit_resets() {
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
                .reset_local("draft")
                .also_replace(summary.clone(), u64::MAX, html! { p { "<new>" } })
                .also_append(list, html! { p { "Next" } })
                .envelope,
        )
        .unwrap();
        assert_eq!(update["target"], component.id());
        assert_eq!(update["reset_local"], serde_json::json!(["draft"]));
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

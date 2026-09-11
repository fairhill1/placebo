//! Experimental HTML updates: a region, a read action, and a shared protocol.
//!
//! The same action emits form configuration and addresses its response. This
//! Input-derived builders connect form controls and handler payloads. Region
//! existence and the contents of browser requests still need runtime checks.

extern crate self as placebo;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use maud::{Markup, html};
use serde::Serialize;
use std::{borrow::Cow, future::Future, marker::PhantomData};

mod forms;
#[doc(hidden)]
pub use forms::private as __private;
pub use forms::{Control, FieldValue, FormFields, FormInput};
pub use placebo_macros::FormInput;
/// Render a typed form body using Maud markup and `@field name = control;` entries.
/// See [`FormInput`] for examples and compile-time guarantees.
pub use placebo_macros::fields;

mod component;
mod diagnostics;
pub use component::{Component, MutationAction, MutationBinding, MutationRequest};

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

    /// Shared snapshots use a monotonic server revision, scoped to this region.
    pub fn mount_versioned(&self, revision: u64, content: Markup) -> Markup {
        html! { div id=(self.id()) data-placebo-region data-placebo-revision=(revision) { (content) } }
    }
}

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

    /// Registers a GET handler whose payload must match this action's input.
    pub fn route<S, H, F, R>(self, handler: H) -> axum::routing::MethodRouter<S>
    where
        S: Clone + Send + Sync + 'static,
        I: Send + 'static,
        H: Fn(S, I, HeaderMap) -> F + Clone + Send + Sync + 'static,
        F: Future<Output = R> + Send + 'static,
        R: IntoResponse + 'static,
    {
        axum::routing::get(
            move |State(state): State<S>, Query(input): Query<I>, headers: HeaderMap| {
                handler(state, input, headers)
            },
        )
        .layer(axum::middleware::from_fn_with_state(
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

    pub fn reply(&self, content: Markup) -> Update {
        Update::new(
            self.action.name,
            self.target.id(),
            "replace-children",
            StatusCode::OK,
            content,
        )
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

#[derive(Serialize)]
pub struct Update {
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

impl Update {
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

    /// Accept incoming local markup only if it has not changed since submission.
    /// Newer browser edits win. Validation/conflict responses cannot reset drafts.
    pub fn reset_local(mut self, key: &str) -> Self {
        assert!(self.status == StatusCode::OK && self.operation == "refresh-component");
        assert!(!key.is_empty() && !self.reset_local.iter().any(|k| k == key));
        self.reset_local.push(key.into());
        self
    }

    /// Refresh a declared shared region only when its server revision is newer.
    /// The region must have been mounted with mount_versioned().
    pub fn also_replace(mut self, region: Region, revision: u64, content: Markup) -> Self {
        self.patches.push(Patch {
            target: region.id().into(),
            operation: "replace-children",
            revision: Some(revision.to_string()),
            html: content.into_string(),
        });
        self
    }

    /// Append new content without replacing existing component instances.
    /// Delivery is not retried; existing/duplicate ids cause rejection.
    pub fn also_append(mut self, region: Region, content: Markup) -> Self {
        self.patches.push(Patch {
            target: region.id().into(),
            operation: "append-children",
            revision: None,
            html: content.into_string(),
        });
        self
    }
}

#[derive(Serialize)]
struct Patch {
    target: String,
    operation: &'static str,
    revision: Option<String>,
    html: String,
}

impl IntoResponse for Update {
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
            serde_json::to_value(action.reply(html! { p { "<script>bad()</script>" } })).unwrap();
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
        let summary = Region::keyed("summary", 42);
        let action = MutationAction::<Search>::new("save", "/save");
        let form = action
            .bind(&component)
            .affects(summary.clone())
            .form(Search::fields().with_q(Control::text("draft")).finish())
            .into_string();
        assert!(form.contains("&quot;effects&quot;:[&quot;summary:42&quot;]"));
        let update = serde_json::to_value(
            action
                .bind(&component)
                .reply(html! { p { "Saved" } })
                .reset_local("draft")
                .also_replace(summary.clone(), u64::MAX, html! { p { "<new>" } })
                .also_append(Region::new("list"), html! { p { "Next" } }),
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
                .mount_versioned(u64::MAX, html! {})
                .into_string()
                .contains(&u64::MAX.to_string())
        );
    }

    #[test]
    #[should_panic]
    fn validation_cannot_request_a_draft_reset() {
        MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .invalid(html! {})
            .reset_local("draft");
    }
}

//! Experimental HTML updates for Rust and Axum: typed forms and actions,
//! whole-page replies, reads, feeds, and a shared protocol with the browser
//! runtime.
//!
//! A save answers with the page it came from, rendered again; the runtime
//! morphs in what changed. A handler validates, writes, and returns its
//! component's contents, and never lists what else is on the page. The same
//! action emits form configuration and addresses its response, and
//! input-derived builders connect form controls and handler payloads.
//!
//! Start from `examples/quickstart.rs` in the repository.
#![doc = include_str!("../docs/rules.md")]

extern crate self as placebo;

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use maud::{Markup, html};
use serde::Serialize;

mod forms;
#[doc(hidden)]
pub use forms::private as __private;
pub use forms::{
    Control, FieldValue, FormEnum, FormFields, FormInput, FormValue, Input, NumberValue,
    DateValue, SingleValue, TextValue,
};
/// Render a typed form body using Maud markup and `@field name = control;` entries.
/// See [`FormInput`] for examples and compile-time guarantees.
pub use placebo_macros::fields;
/// A [Lucide](https://lucide.dev/icons) icon as inline SVG, for Maud:
/// `button .btn { (icon!("check")) "Save" }`. The name is checked while
/// compiling, and only the icons an app names are in its binary and pages.
/// The SVG carries the kit's `.icon` class and `aria-hidden`, so an icon
/// with no text beside it needs a visually hidden label:
/// `span .visually-hidden { "Delete" }`.
///
/// ```compile_fail
/// let _ = placebo::icon!("chek"); // Lucide has no icon `chek`. Did you mean `check`?
/// ```
pub use placebo_macros::icon;
pub use placebo_macros::{FormEnum, FormInput};

mod component;
mod diagnostics;
mod native;
mod push;
pub mod replay;
pub mod styles;
mod upload;
pub use component::{
    Component, MountedComponent, MutationAction, MutationBinding, MutationRequest,
};
pub use native::native_forms;
pub use push::{Feed, Feeds};
pub use replay::replays;
pub use upload::{DEFAULT_MAX_BYTES, FileValue, Upload};

#[cfg(all(feature = "dev", debug_assertions))]
pub mod dev;

pub const VERSION: u8 = 6;
/// The browser runtime: the vendored idiomorph, then Placebo's own code.
pub const RUNTIME: &str = concat!(
    include_str!("../client/idiomorph.js"),
    "\n",
    include_str!("../client/placebo.js")
);

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

/// A form that reads the page it is on again with its fields as the query,
/// such as a search or a filter. The runtime fetches the page at that query,
/// puts the query in the address bar, and morphs the page in, keeping what
/// the person is typing. Without JavaScript the browser loads the same URL.
///
/// Render the page from its query with `Input<Q>` in the page handler, and
/// build every read form on the page from that same `Q`: the form replaces
/// the whole query, so a form renders the fields it keeps as hidden controls,
/// and leaves out with `@omit` an optional field it resets, such as the page
/// a new search starts again from.
///
/// ```
/// use placebo::{Control, FormInput, Read, fields};
/// #[derive(serde::Deserialize, FormInput)]
/// struct Search { q: String, page: Option<u32> }
/// let form = Read::new().on_input(150).form(fields! { Search {
///     @field q = Control::search("rust");
///     @omit page;
/// } });
/// assert!(form.into_string().starts_with("<form method=\"get\" data-placebo="));
/// ```
///
/// Only a field whose absence decodes can be omitted: an `Option`, a `bool`,
/// or one with `serde(default)`.
///
/// ```compile_fail
/// use placebo::{Control, FormInput, Read, fields};
/// #[derive(serde::Deserialize, FormInput)]
/// struct Search { q: String, page: u32 }
/// let form = Read::new().form(fields! { Search {
///     @field q = Control::search("rust");
///     @omit page;
/// } });
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct Read {
    input_delay_ms: Option<u32>,
    reveal: bool,
}

impl Read {
    pub const fn new() -> Self {
        Self {
            input_delay_ms: None,
            reveal: false,
        }
    }

    /// Read as the person types, once they pause for `delay_ms`. The newest
    /// read wins: an older one still in flight is dropped.
    pub const fn on_input(mut self, delay_ms: u32) -> Self {
        assert!(delay_ms <= 60_000, "input delay exceeds one minute");
        self.input_delay_ms = Some(delay_ms);
        self
    }

    /// Read when the form scrolls near the viewport, such as a "load more"
    /// form at the end of a list that asks for a longer page. After each read
    /// it watches again if its fields now ask for something else.
    pub const fn on_reveal(mut self) -> Self {
        self.reveal = true;
        self
    }

    pub fn form<I: FormInput>(self, fields: FormFields<I>) -> Markup {
        const {
            assert!(
                !I::MULTIPART,
                "a read submits its fields in the URL, so its payload cannot have an Upload field"
            )
        };
        let config = ReadConfig {
            version: VERSION,
            read: true,
            input_delay_ms: self.input_delay_ms,
            reveal: self.reveal,
        };
        let config = serde_json::to_string(&config).expect("static configuration serializes");
        // No action: a GET form without one submits to the page it is on.
        html! {
            form method="get" data-placebo=(config) { (fields.into_markup()) }
        }
    }
}

#[derive(Serialize)]
struct ReadConfig {
    version: u8,
    read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_delay_ms: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    reveal: bool,
}

/// Read the page again every `interval_ms` while this element is on it, such
/// as while a job the page shows is still running: render it only while
/// there is something to wait for. Polling pauses while the page is hidden
/// and reads once when it is shown again. For changes this app makes, a
/// [`Feed`] tells pages without polling.
pub fn refresh_every(interval_ms: u32) -> Markup {
    assert!(
        (500..=86_400_000).contains(&interval_ms),
        "poll between every 500 ms and once a day"
    );
    html! { div hidden data-placebo-refresh-every=(interval_ms) {} }
}

/// What a mutation handler returns: its component's contents and outcome.
/// The route adapter turns it into the page it was submitted from.
struct Reply {
    target: String,
    html: String,
    status: StatusCode,
    navigate: Option<String>,
}

impl IntoResponse for Reply {
    fn into_response(self) -> Response {
        let mut response = (self.status, [(header::CACHE_CONTROL, "no-store")]).into_response();
        // The mutation adapter renders the page, or redirects a native
        // submission, from this.
        response.extensions_mut().insert(native::NativeReply {
            target: self.target,
            html: self.html,
            navigate: self.navigate,
        });
        response
    }
}

/// A successful mutation reply, from `MutationBinding::reply`. The page it
/// was submitted from is rendered again with the component showing these
/// contents, so everything else on the page shows the write too. Controls the
/// person submitted take the reply's values unless they were edited again
/// while the request was in flight.
pub struct Applied(Reply);

impl Applied {
    /// Go to a same-site path instead of showing the page, for example to a
    /// record that was just created or away from one that was deleted.
    pub fn navigate(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        assert!(
            path.starts_with('/') && !path.starts_with("//"),
            "navigate to a path on this site, starting with a single '/'"
        );
        self.0.navigate = Some(path);
        self
    }
}

impl IntoResponse for Applied {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

/// A validation or conflict reply, from `MutationBinding::invalid` or
/// `conflict`. The page is rendered again around it, so the rest shows
/// current data. Controls with edits keep them; untouched controls show the
/// reply's values. It navigates nowhere.
///
/// ```compile_fail
/// use placebo::{Component, FormInput, MutationAction};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// MutationAction::<Save>::new("save", "/save").bind(&Component::new("editor", 1))
///     .invalid(html! {}).navigate("/");
/// ```
pub struct Rejected(Reply);

impl IntoResponse for Rejected {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

/// Serve the exact browser half embedded in this crate, with no independent
/// dependency resolution. Version mismatches still fail visibly at runtime.
pub async fn runtime() -> Response {
    #[cfg(all(feature = "dev", debug_assertions))]
    let content = match ["idiomorph.js", "placebo.js"].map(|file| {
        std::fs::read_to_string(format!("{}/client/{file}", env!("CARGO_MANIFEST_DIR")))
    }) {
        [Ok(morph), Ok(runtime)] => format!("{morph}\n{runtime}"),
        _ => {
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

/// Where [`kit`] serves the CSS kit; an app's stylesheet starts with
/// `@import url("/placebo/kit/main.css");`.
pub const KIT_PATH: &str = "/placebo/kit/{file}";

/// Serve the CSS kit embedded in this crate, as [`runtime`] serves the
/// script, so an app's kit is always the one its Placebo version ships and
/// `cargo update -p placebo` updates it: `.route(placebo::KIT_PATH,
/// get(placebo::kit))`.
pub async fn kit(axum::extract::Path(file): axum::extract::Path<String>) -> Response {
    let Some((name, shipped)) = styles::KIT.iter().find(|(name, _)| *name == file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Placebo's own kit edits show without rebuilding the app.
    #[cfg(all(feature = "dev", debug_assertions))]
    let content = std::fs::read_to_string(format!("{}/kit/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|_| (*shipped).to_owned());
    #[cfg(not(all(feature = "dev", debug_assertions)))]
    let content = {
        let _ = name;
        *shipped
    };
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        content,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    #[derive(serde::Deserialize, FormInput)]
    struct Search {
        q: String,
    }

    #[test]
    fn a_read_form_reads_its_own_page_with_its_triggers() {
        let form = Read::new()
            .on_input(150)
            .on_reveal()
            .form(Search::fields().with_q(Control::search("<rust>")).finish())
            .into_string();
        assert!(
            form.starts_with("<form method=\"get\" data-placebo="),
            "{form}"
        );
        assert!(!form.contains("action="));
        assert!(form.contains(
            "{&quot;version&quot;:6,&quot;read&quot;:true,&quot;input_delay_ms&quot;:150,&quot;reveal&quot;:true}"
        ));
        assert!(form.contains("value=\"&lt;rust&gt;\""));
        let plain = Read::new()
            .form(Search::fields().with_q(Control::search("")).finish())
            .into_string();
        assert!(plain.contains("{&quot;version&quot;:6,&quot;read&quot;:true}"));
    }

    #[test]
    fn polling_renders_its_interval() {
        assert_eq!(
            refresh_every(5000).into_string(),
            "<div hidden data-placebo-refresh-every=\"5000\"></div>"
        );
    }

    #[test]
    #[should_panic(expected = "poll between every 500 ms and once a day")]
    fn polling_faster_than_twice_a_second_is_refused() {
        refresh_every(100);
    }

    #[test]
    fn a_mutation_reply_carries_its_component_for_the_page() {
        let response = MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .reply(html! { p { "<Saved>" } })
            .navigate("/tasks/3")
            .into_response();
        let reply = response.extensions().get::<native::NativeReply>().unwrap();
        assert_eq!(reply.target, "editor:1");
        assert_eq!(reply.html, "<p>&lt;Saved&gt;</p>");
        assert_eq!(reply.navigate.as_deref(), Some("/tasks/3"));
    }

    #[test]
    #[should_panic(expected = "single '/'")]
    fn replies_navigate_only_within_the_site() {
        MutationAction::<Search>::new("save", "/save")
            .bind(&Component::new("editor", 1))
            .reply(html! {})
            .navigate("//example.com/");
    }
}

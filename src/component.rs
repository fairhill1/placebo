use axum::{
    extract::{FromRequestParts, Request},
    handler::Handler,
    http::{StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use maud::{DOCTYPE, Markup, Render, html};
use std::marker::PhantomData;

use crate::{
    Applied, Config, EndsWithInput, Envelope, FormFields, FormInput, RegionTarget, Rejected,
    VERSION,
};

/// Identity is scoped by component kind and a runtime instance key. This is
/// UI addressing, not authorization to modify the record with that key.
pub struct Component {
    id: String,
}

/// An initial component mount, renderable inside `html!`, not reply contents.
/// This distinction catches accidentally sending a second component wrapper.
/// It does not inspect arbitrary HTML composed around a mount.
///
/// ```compile_fail
/// use placebo::{Component, FormInput, MutationAction};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// let component = Component::new("editor", 1);
/// let save = MutationAction::<Save>::new("save", "/save");
/// save.bind(&component).reply(component.mount(html! { p { "Contents" } }));
/// ```
pub struct MountedComponent(Markup);

impl Render for MountedComponent {
    fn render_to(&self, buffer: &mut String) {
        self.0.render_to(buffer);
    }
}

impl Component {
    pub fn new(kind: &str, key: impl std::fmt::Display) -> Self {
        assert!(
            !kind.is_empty() && kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "component kind must contain only letters, digits or hyphens"
        );
        let key = key.to_string();
        assert!(
            !key.is_empty() && !key.chars().any(char::is_whitespace),
            "invalid component key"
        );
        Self {
            id: format!("{kind}:{key}"),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn mount(&self, content: Markup) -> MountedComponent {
        MountedComponent(
            html! { div id=(self.id()) data-placebo-region data-placebo-component { (content) } },
        )
    }

    /// Mount the native dialog itself as the persistent component root.
    /// Replies replace its contents, preserving the dialog node, open state,
    /// native modality and listeners. Include the labelled heading in every
    /// render of the contents. Opening and closing remain application behavior.
    /// Alternatively, mount a normal component *inside* a persistent dialog.
    pub fn mount_dialog(&self, labelled_by: &str, content: Markup) -> MountedComponent {
        assert!(!labelled_by.is_empty(), "a dialog needs a heading id");
        MountedComponent(html! {
            dialog id=(self.id()) aria-labelledby=(labelled_by) data-placebo-region data-placebo-component { (content) }
        })
    }

    /// This entire subtree belongs to the browser after initial rendering.
    /// Matching keys retain the existing DOM node; fresh server content under
    /// that key is ignored. Keep validation messages and record versions outside.
    /// Keys are local to one mounted component. Nested local subtrees are rejected.
    pub fn local(&self, key: &str, content: Markup) -> Markup {
        assert!(!key.is_empty(), "local state needs a key");
        html! { div data-placebo-local=(key) { (content) } }
    }
}

/// Reusable POST endpoint. Binding it to an instance is a view-level choice.
pub struct MutationAction<I: FormInput> {
    name: &'static str,
    path: &'static str,
    input: PhantomData<fn() -> I>,
}

impl<I: FormInput> Copy for MutationAction<I> {}
impl<I: FormInput> Clone for MutationAction<I> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I: FormInput> MutationAction<I> {
    pub const fn new(name: &'static str, path: &'static str) -> Self {
        assert!(
            !name.is_empty() && !path.is_empty(),
            "action needs a name and path"
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

    pub fn bind(self, component: &Component) -> MutationBinding<I> {
        MutationBinding {
            action: self,
            target: component.id().to_owned(),
            effects: Vec::new(),
        }
    }

    /// Registers a POST handler behind the mutation request check. It may take
    /// any Axum extractors, such as a session, and must end with [`crate::Input`]
    /// of this action's payload type.
    pub fn route<H, T, S>(self, handler: H) -> axum::routing::MethodRouter<S>
    where
        H: Handler<T, S>,
        T: EndsWithInput<I> + 'static,
        S: Clone + Send + Sync + 'static,
    {
        axum::routing::post(handler)
            // A route layer skips the 405 fallback for other methods.
            .route_layer(axum::middleware::from_fn(require_mutation))
            .layer(axum::middleware::from_fn_with_state(
                self.name,
                crate::diagnostics::request,
            ))
    }
}

/// Runs before the handler's extractors, so refused requests reach no handler code.
async fn require_mutation(request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    match MutationRequest::from_request_parts(&mut parts, &()).await {
        Ok(MutationRequest) => next.run(Request::from_parts(parts, body)).await,
        Err(rejection) => rejection,
    }
}

/// A mutation form and its replies, for one component instance. Build the view's
/// form and the handler's replies from one function that returns the binding,
/// so both agree on `affects`:
///
/// ```
/// use placebo::{Component, FormInput, MutationAction, MutationBinding, VersionedRegion};
/// use maud::html;
/// #[derive(serde::Deserialize, FormInput)]
/// struct Save { title: String }
/// const SAVE: MutationAction<Save> = MutationAction::new("save", "/save");
/// const COUNTS: VersionedRegion = VersionedRegion::new("counts");
/// fn save_binding(id: u64) -> MutationBinding<Save> {
///     SAVE.bind(&Component::new("editor", id)).affects(COUNTS)
/// }
/// // The handler replies through the same binding as the view's form.
/// save_binding(1).reply(html! {}).also_replace(COUNTS, 2, html! { "3 items" });
/// ```
pub struct MutationBinding<I: FormInput> {
    action: MutationAction<I>,
    target: String,
    effects: Vec<String>,
}

impl<I: FormInput> MutationBinding<I> {
    /// Declare additional mounted regions this form's response may update.
    /// Their DOM identities are captured when the request is scheduled.
    pub fn affects(mut self, region: impl RegionTarget) -> Self {
        assert!(!self.effects.iter().any(|id| id == region.id()));
        self.effects.push(region.id().to_owned());
        self
    }

    pub fn form(&self, fields: FormFields<I>) -> Markup {
        let content = fields.into_markup();
        let config = Config {
            version: VERSION,
            action: self.action.name,
            target: &self.target,
            policy: "exclusive",
            operation: "refresh-component",
            input_delay_ms: None,
            effects: self.effects.iter().map(String::as_str).collect(),
        };
        let config = serde_json::to_string(&config).expect("configuration serializes");
        html! { form method="post" action=(self.action.path) data-placebo=(config) { (content) } }
    }

    pub fn reply(&self, content: Markup) -> Applied {
        Applied {
            envelope: self.envelope(StatusCode::OK, content),
            effects: self.effects.clone(),
        }
    }
    pub fn invalid(&self, content: Markup) -> Rejected {
        self.rejected(StatusCode::UNPROCESSABLE_ENTITY, content)
    }
    pub fn conflict(&self, content: Markup) -> Rejected {
        self.rejected(StatusCode::CONFLICT, content)
    }

    fn rejected(&self, status: StatusCode, content: Markup) -> Rejected {
        Rejected {
            envelope: self.envelope(status, content),
            effects: self.effects.clone(),
        }
    }

    fn envelope(&self, status: StatusCode, content: Markup) -> Envelope {
        Envelope::new(
            self.action.name,
            &self.target,
            "refresh-component",
            status,
            content,
        )
    }
}

/// Require the non-simple framework header on mutation requests. Cross-origin
/// forms cannot add it, and cross-origin fetches need CORS permission. This
/// assumes same-origin deployment without permissive CORS; it is not auth.
/// The version string comes from the same constant used by the emitter.
pub struct MutationRequest;

impl<S: Send + Sync> FromRequestParts<S> for MutationRequest {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let version = parts
            .headers
            .get("x-placebo-request")
            .and_then(|h| h.to_str().ok());
        let site = parts
            .headers
            .get("sec-fetch-site")
            .and_then(|h| h.to_str().ok());
        let (title, explanation) = if matches!(site, Some("cross-site" | "same-site")) {
            (
                "Request refused",
                "This form was submitted from another website, so it was not accepted.",
            )
        } else if version.is_none() {
            // A native submission: the runtime had not loaded, or failed to.
            (
                "Your changes were not saved",
                "The page had not finished loading when the form was sent. Use your \
                 browser's Back button (it usually keeps what you typed), wait for the \
                 page to finish loading, and submit again.",
            )
        } else if version != Some(VERSION.to_string().as_str()) {
            (
                "Your changes were not saved",
                "This page is out of date. Copy anything you typed, reload the page, \
                 and submit again.",
            )
        } else {
            return Ok(Self);
        };
        // Readable in the browser for native submissions; the runtime reports
        // enhanced requests through its own http-error diagnostic.
        Err((
            StatusCode::FORBIDDEN,
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
            .into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[derive(serde::Deserialize, crate::FormInput)]
    struct Input {
        title: String,
    }

    #[test]
    fn one_action_addresses_distinct_runtime_instances() {
        let action = MutationAction::<Input>::new("save", "/save");
        for id in [7, 19] {
            let component = Component::new("editor", id);
            let response =
                serde_json::to_value(action.bind(&component).reply(html! {}).envelope).unwrap();
            assert_eq!(response["target"], format!("editor:{id}"));
            assert_eq!(response["action"], "save");
        }
        let input: Input = serde_json::from_str(r#"{"title":"typed"}"#).unwrap();
        assert_eq!(input.title, "typed");
    }

    #[test]
    fn validation_is_an_explicit_non_success_response() {
        let component = Component::new("editor", 1);
        let action = MutationAction::<Input>::new("save", "/save");
        let response = action
            .bind(&component)
            .invalid(html! { p { "Too short" } })
            .into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

use axum::response::IntoResponse;
use axum::{
    Form,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
};
use maud::{Markup, html};
use std::{future::Future, marker::PhantomData};

use crate::{Config, FormFields, FormInput, Region, Update, VERSION};

/// Identity is scoped by component kind and a runtime instance key. This is
/// UI addressing, not authorization to modify the record with that key.
pub struct Component {
    id: String,
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

    pub fn mount(&self, content: Markup) -> Markup {
        html! { div id=(self.id()) data-placebo-region data-placebo-component { (content) } }
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

    pub fn bind(self, component: &Component) -> MutationBinding<'_, I> {
        MutationBinding {
            action: self,
            component,
            effects: Vec::new(),
        }
    }

    /// Connect this payload type to the handler and enforce the mutation header.
    pub fn route<S, H, F, R>(self, handler: H) -> axum::routing::MethodRouter<S>
    where
        S: Clone + Send + Sync + 'static,
        I: Send + 'static,
        H: Fn(S, I) -> F + Clone + Send + Sync + 'static,
        F: Future<Output = R> + Send + 'static,
        R: IntoResponse + 'static,
    {
        axum::routing::post(
            move |State(state): State<S>, _: MutationRequest, Form(input): Form<I>| {
                handler(state, input)
            },
        )
        .layer(axum::middleware::from_fn_with_state(
            self.name,
            crate::diagnostics::request,
        ))
    }
}

pub struct MutationBinding<'a, I: FormInput> {
    action: MutationAction<I>,
    component: &'a Component,
    effects: Vec<Region>,
}

impl<I: FormInput> MutationBinding<'_, I> {
    /// Declare additional mounted regions this form's response may update.
    /// Their DOM identities are captured when the request is scheduled.
    pub fn affects(mut self, region: Region) -> Self {
        assert!(!self.effects.iter().any(|r| r.id() == region.id()));
        self.effects.push(region);
        self
    }

    pub fn form(&self, fields: FormFields<I>) -> Markup {
        let content = fields.into_markup();
        let config = Config {
            version: VERSION,
            action: self.action.name,
            target: self.component.id(),
            policy: "exclusive",
            operation: "refresh-component",
            input_delay_ms: None,
            effects: self.effects.iter().map(|r| r.id()).collect(),
        };
        let config = serde_json::to_string(&config).expect("configuration serializes");
        html! { form method="post" action=(self.action.path) data-placebo=(config) { (content) } }
    }

    pub fn reply(&self, content: Markup) -> Update {
        self.update(StatusCode::OK, content)
    }
    pub fn invalid(&self, content: Markup) -> Update {
        self.update(StatusCode::UNPROCESSABLE_ENTITY, content)
    }
    pub fn conflict(&self, content: Markup) -> Update {
        self.update(StatusCode::CONFLICT, content)
    }

    fn update(&self, status: StatusCode, content: Markup) -> Update {
        Update::new(
            self.action.name,
            self.component.id(),
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
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let version = parts
            .headers
            .get("x-placebo-request")
            .and_then(|h| h.to_str().ok());
        let site = parts
            .headers
            .get("sec-fetch-site")
            .and_then(|h| h.to_str().ok());
        if version != Some(VERSION.to_string().as_str())
            || matches!(site, Some("cross-site" | "same-site"))
        {
            return Err((
                StatusCode::FORBIDDEN,
                "Expected a same-origin Placebo mutation request.",
            ));
        }
        Ok(Self)
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
            let response = serde_json::to_value(action.bind(&component).reply(html! {})).unwrap();
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

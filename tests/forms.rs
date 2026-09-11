use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use maud::html;
use placebo::{Component, Control, FormInput, MutationAction, ReadAction, Region, fields};
use serde::Deserialize;
use tower::ServiceExt;

#[derive(Debug, Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Renamed {
    id: u64,
    #[serde(rename = "display-title")]
    title: String,
    #[serde(default)]
    delay_ms: u64,
}

const SAVE: MutationAction<Renamed> = MutationAction::new("save", "/save");

fn fields() -> placebo::FormFields<Renamed> {
    fields! { Renamed {
        @field id = Control::hidden(42);
        div data-placebo-local="draft" {
            @field title = Control::text("<script>&\"").id("title");
            .network {
                @field delay_ms = Control::select(0, [(0, "None"), (600, "Slow")]);
            }
        }
        button type="submit" { "Save" }
    } }
}

fn builder_fields() -> placebo::FormFields<Renamed> {
    Renamed::fields()
        .with_id(Control::hidden(42))
        .local("draft", |fields| {
            fields
                .with_title(Control::text("<script>&\"").id("title"))
                .group("network", |fields| {
                    fields.with_delay_ms(Control::select(0, [(0, "None"), (600, "Slow")]))
                })
        })
        .markup(html! { button type="submit" { "Save" } })
        .finish()
}

#[test]
fn markup_macro_matches_the_existing_builder_byte_for_byte() {
    let component = Component::new("editor", 42);
    assert_eq!(
        SAVE.bind(&component).form(fields()).into_string(),
        SAVE.bind(&component).form(builder_fields()).into_string(),
    );
}

#[test]
fn controls_evaluate_once_and_layout_keeps_maud_conditions_and_loops() {
    let mut evaluated = Vec::new();
    let __placebo_fields = "A title";
    let __placebo_control_0 = "Surrounding text";
    let fields = fields! { Renamed {
        @if false { p { "hidden" } } @else if true { p { "visible" } } @else { "hidden" }
        @field id = { evaluated.push("id"); Control::hidden(42) };
        fieldset {
            @field title = { evaluated.push("title"); Control::text(__placebo_fields) };
            @field delay_ms = { evaluated.push("delay"); Control::hidden(0) };
        }
        @for text in ["one", "two"] { span { (text) } }
        @match 1 { 1 => { "matched" }, _ => { "hidden" } }
        (__placebo_control_0)
    } };
    assert_eq!(evaluated, ["id", "title", "delay"]);
    let output = SAVE
        .bind(&Component::new("editor", 42))
        .form(fields)
        .into_string();
    assert!(output.contains("<p>visible</p>"));
    assert!(output.contains("value=\"A title\""));
    assert!(output.contains("<span>one</span><span>two</span>matchedSurrounding text"));
}

#[test]
fn raw_field_identifiers_keep_their_serde_wire_name() {
    #[derive(Deserialize, FormInput)]
    struct Input {
        #[serde(rename = "kind")]
        r#type: String,
    }
    let fields = fields! { Input { @field r#type = Control::text("note"); } };
    let input: Input = serde_json::from_str(r#"{"kind":"note"}"#).unwrap();
    assert_eq!(input.r#type, "note");
    let output = MutationAction::<Input>::new("save", "/save")
        .bind(&Component::new("editor", 1))
        .form(fields)
        .into_string();
    assert!(output.contains("name=\"kind\""));
}

async fn save(_: (), input: Renamed) -> String {
    format!("{}:{}:{}", input.id, input.title, input.delay_ms)
}

fn request(body: &str, marker: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/save")
        .header("content-type", "application/x-www-form-urlencoded");
    if marker {
        request = request.header("x-placebo-request", placebo::VERSION.to_string());
    }
    request.body(Body::from(body.to_owned())).unwrap()
}

#[test]
fn generated_controls_use_serde_names_and_escape_values_inside_local_groups() {
    let component = Component::new("editor", 42);
    let form = SAVE.bind(&component).form(fields()).into_string();
    for name in ["id", "display-title", "delay_ms"] {
        assert_eq!(form.matches(&format!("name=\"{name}\"")).count(), 1);
    }
    assert!(!form.contains("name=\"title\""));
    assert!(form.contains("&lt;script&gt;&amp;&quot;"));
    assert!(form.contains("data-placebo-local=\"draft\""));
    assert!(form.contains("class=\"network\""));
}

#[tokio::test]
async fn mutation_adapter_deserializes_the_declared_input_and_keeps_the_request_guard() {
    let app = Router::new().route(SAVE.path(), SAVE.route(save));
    let response = app
        .clone()
        .oneshot(request(
            "id=42&display-title=Hello+world&delay_ms=600",
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await.unwrap(),
        "42:Hello world:600"
    );
    let unmarked = app
        .oneshot(request("id=42&display-title=Hello", false))
        .await
        .unwrap();
    assert_eq!(unmarked.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn crafted_payloads_still_need_and_receive_runtime_validation() {
    let app = Router::new().route(SAVE.path(), SAVE.route(save));
    for body in [
        "id=not-a-number&display-title=Hello",
        "id=42",
        "id=42&title=Wrong+wire+name",
        "id=42&display-title=Hello&surprise=unknown",
        "id=42&display-title=First&display-title=Second",
    ] {
        let response = app.clone().oneshot(request(body, true)).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
    }
}

#[tokio::test]
async fn action_request_ids_are_echoed_on_extractor_errors_and_invalid_ids_are_ignored() {
    let app = Router::new().route(SAVE.path(), SAVE.route(save));
    let mut malformed = request("id=not-a-number", true);
    malformed
        .headers_mut()
        .insert("x-placebo-request-id", "audit-42".parse().unwrap());
    let response = app.clone().oneshot(malformed).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers()["x-placebo-request-id"], "audit-42");
    for id in ["has spaces".to_owned(), "a".repeat(65)] {
        let mut valid = request("id=42&display-title=Hello", true);
        valid
            .headers_mut()
            .insert("x-placebo-request-id", id.parse().unwrap());
        let response = app.clone().oneshot(valid).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("x-placebo-request-id"));
    }
}

#[tokio::test]
async fn read_adapter_and_builder_share_the_input_type() {
    let action = ReadAction::<Renamed>::new("read", "/read");
    let form = action
        .bind(Region::new("results"))
        .form(fields())
        .into_string();
    assert!(form.contains("method=\"get\""));
    let app = Router::new().route(
        action.path(),
        action.route(|state: (), input, _headers| save(state, input)),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/read?id=7&display-title=Query")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await.unwrap(),
        "7:Query:0"
    );
}

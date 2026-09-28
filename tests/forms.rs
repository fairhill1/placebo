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

#[derive(Debug, Deserialize, FormInput, PartialEq)]
#[serde(deny_unknown_fields)]
struct Profile {
    bio: String,
    email: String,
    age: u32,
    rating: Option<f64>,
    newsletter: bool,
    nickname: Option<String>,
    role: Option<u8>,
    #[serde(default)]
    tags: Vec<u8>,
    #[serde(default)]
    days: Vec<String>,
}

const PROFILE: MutationAction<Profile> = MutationAction::new("profile", "/profile");

fn profile_form() -> String {
    let fields = fields! { Profile {
        @field bio = Control::textarea("\nfirst line <b>").rows(4).placeholder("About you");
        @field email = Control::email("ada@example.com").autocomplete("email");
        @field age = Control::number(36).min(0).max(150);
        @field rating = Control::number(Some(4.5)).step(0.5);
        @field newsletter = Control::checkbox(true).id("newsletter");
        @field nickname = Control::text(None);
        @field role = Control::radios(None, [(Some(1), "Owner"), (Some(2), "Editor")]).id("role");
        @field tags = Control::multi_select([2], [(1, "One"), (2, "Two"), (3, "Three")]);
        @field days = Control::checkboxes(["sat".to_owned()], [("fri".to_owned(), "Friday"), ("sat".to_owned(), "Saturday")]);
    } };
    PROFILE
        .bind(&Component::new("profile", 1))
        .form(fields)
        .into_string()
}

#[test]
fn every_control_renders_its_generated_name_and_initial_value() {
    let form = profile_form();
    assert!(form.contains(
        "<textarea name=\"bio\" rows=\"4\" placeholder=\"About you\">\n\nfirst line &lt;b&gt;</textarea>"
    ));
    assert!(form.contains(
        "<input type=\"email\" name=\"email\" value=\"ada@example.com\" autocomplete=\"email\">"
    ));
    assert!(
        form.contains("<input type=\"number\" name=\"age\" value=\"36\" min=\"0\" max=\"150\">")
    );
    assert!(form.contains("<input type=\"number\" name=\"rating\" value=\"4.5\" step=\"0.5\">"));
    assert!(form.contains(
        "<input type=\"checkbox\" name=\"newsletter\" value=\"true\" checked id=\"newsletter\">"
    ));
    assert!(form.contains("<input type=\"text\" name=\"nickname\" value=\"\">"));
    assert!(form.contains("<div role=\"radiogroup\" id=\"role\"><label><input type=\"radio\" name=\"role\" value=\"1\"> Owner</label>"));
    assert!(!form.contains("name=\"role\" value=\"1\" checked"));
    assert!(form.contains("<select multiple name=\"tags\"><option value=\"1\">One</option><option value=\"2\" selected>Two</option>"));
    assert!(
        form.contains("<input type=\"checkbox\" name=\"days\" value=\"sat\" checked> Saturday")
    );
    assert_eq!(form.matches("name=\"days\"").count(), 2);
}

#[test]
fn float_numbers_accept_fractions_and_passwords_are_never_echoed() {
    #[allow(dead_code)]
    #[derive(Deserialize, FormInput)]
    struct Login {
        password: String,
        weight: f32,
    }
    let fields = fields! { Login {
        @field password = Control::password().autocomplete("current-password");
        @field weight = Control::number(1.25);
    } };
    let form = MutationAction::<Login>::new("login", "/login")
        .bind(&Component::new("login", 1))
        .form(fields)
        .into_string();
    assert!(form.contains(
        "<input type=\"password\" name=\"password\" value=\"\" autocomplete=\"current-password\">"
    ));
    assert!(form.contains("<input type=\"number\" name=\"weight\" value=\"1.25\" step=\"any\">"));
}

#[test]
#[should_panic(expected = "every selected value needs exactly one matching option")]
fn multiple_selection_rejects_values_without_options() {
    Control::<Vec<u8>>::checkboxes([9], [(1, "One")]);
}

#[test]
#[should_panic(expected = "radios need an option matching their initial value")]
fn required_radios_need_a_matching_option() {
    Control::<u8>::radios(9, [(1, "One")]);
}

#[test]
#[should_panic(expected = "rows applies to textarea controls")]
fn control_specific_attributes_are_not_silently_ignored() {
    Control::<String>::text("title").rows(3);
}

async fn save_profile(_: (), input: Profile) -> String {
    format!("{input:?}")
}

fn profile(bio: &str) -> Profile {
    Profile {
        bio: bio.into(),
        email: "a@b.c".into(),
        age: 36,
        rating: None,
        newsletter: false,
        nickname: None,
        role: None,
        tags: vec![],
        days: vec![],
    }
}

#[tokio::test]
async fn browser_submissions_decode_with_html_absence_rules() {
    let app = Router::new().route(PROFILE.path(), PROFILE.route(save_profile));
    let cases = [
        // Unchecked checkbox, empty optional inputs, no radio and no selections.
        (
            "bio=Hi&email=a%40b.c&age=36&rating=&nickname=",
            profile("Hi"),
        ),
        (
            "bio=Hi&email=a%40b.c&age=36&rating=2.5&newsletter=true&nickname=Ada&role=2&tags=1&tags=3&days=fri&days=sat",
            Profile {
                rating: Some(2.5),
                newsletter: true,
                nickname: Some("Ada".into()),
                role: Some(2),
                tags: vec![1, 3],
                days: vec!["fri".into(), "sat".into()],
                ..profile("Hi")
            },
        ),
    ];
    for (body, expected) in cases {
        let response = app
            .clone()
            .oneshot(request_to("/profile", body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{body}");
        assert_eq!(
            to_bytes(response.into_body(), 4096).await.unwrap(),
            format!("{expected:?}"),
        );
    }
    for body in [
        "bio=Hi&email=a%40b.c&age=",
        "bio=Hi&email=a%40b.c&age=36&newsletter=true&newsletter=false",
        "bio=Hi&email=a%40b.c&age=36&newsletter=yes",
        "bio=Hi&email=a%40b.c&age=36&tags=one",
        "email=a%40b.c&age=36",
    ] {
        let response = app
            .clone()
            .oneshot(request_to("/profile", body))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
    }
    let mut json = request_to("/profile", "{}");
    json.headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    let response = app.oneshot(json).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn read_queries_use_the_same_decoding_rules() {
    #[allow(dead_code)] // Read through Debug.
    #[derive(Debug, Deserialize, FormInput)]
    struct Filter {
        q: String,
        open: bool,
        #[serde(default)]
        labels: Vec<String>,
        limit: Option<u32>,
    }
    let action = ReadAction::<Filter>::new("filter", "/filter");
    let app = Router::new().route(
        action.path(),
        action.route(|_: (), input: Filter, _| async move { format!("{input:?}") }),
    );
    for (query, expected) in [
        (
            "q=bug",
            r#"Filter { q: "bug", open: false, labels: [], limit: None }"#,
        ),
        (
            "q=bug&open=true&labels=ui&labels=api&limit=5",
            r#"Filter { q: "bug", open: true, labels: ["ui", "api"], limit: Some(5) }"#,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/filter?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{query}");
        assert_eq!(
            to_bytes(response.into_body(), 4096).await.unwrap(),
            expected
        );
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/filter?q=bug&limit=many")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn request_to(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-placebo-request", placebo::VERSION.to_string())
        .body(Body::from(body.to_owned()))
        .unwrap()
}

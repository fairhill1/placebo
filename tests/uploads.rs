//! Typed file fields: multipart decoding, limits, and form rendering.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use placebo::{Component, Control, FormInput, Input, MutationAction, Upload, fields};
use serde::Deserialize;
use tower::ServiceExt;

#[derive(Debug, Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Attach {
    note: String,
    cover: Option<Upload<16>>,
    #[serde(default)]
    files: Vec<Upload<32>>,
}

#[derive(Debug, Deserialize, FormInput)]
struct Required {
    file: Upload,
}

#[derive(Debug, Deserialize, FormInput)]
struct Note {
    note: String,
}

const ATTACH: MutationAction<Attach> = MutationAction::new("attach", "/attach");
const NOTE: MutationAction<Note> = MutationAction::new("note", "/note");
const REQUIRED: MutationAction<Required> = MutationAction::new("required", "/required");

async fn attach(Input(input): Input<Attach>) -> String {
    let files: Vec<String> = input
        .files
        .iter()
        .map(|file| format!("{}={}", file.file_name(), file.len()))
        .collect();
    format!(
        "{:?}|{:?}|{}",
        input.note,
        input.cover.map(|cover| (
            cover.file_name().to_owned(),
            cover.content_type().map(str::to_owned),
            cover.into_bytes()
        )),
        files.join(",")
    )
}

async fn required(Input(input): Input<Required>) -> String {
    input.file.file_name().to_owned()
}

async fn note(Input(input): Input<Note>) -> String {
    input.note
}

fn app() -> Router {
    Router::new()
        .route(ATTACH.path(), ATTACH.route(attach))
        .route(REQUIRED.path(), REQUIRED.route(required))
        .route(NOTE.path(), NOTE.route(note))
}

/// A multipart body: `(name, Some((file name, content)))` is a file part.
fn multipart(path: &str, parts: &[(&str, Option<&str>, &str)]) -> Request<Body> {
    let mut body = String::new();
    for (name, file, value) in parts {
        body.push_str("--XYZ\r\n");
        match file {
            Some(file) => body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file}\"\r\nContent-Type: text/plain\r\n\r\n"
            )),
            None => body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n"
            )),
        }
        body.push_str(value);
        body.push_str("\r\n");
    }
    body.push_str("--XYZ--\r\n");
    Request::post(path)
        .header("x-placebo-request", placebo::VERSION.to_string())
        .header("content-type", "multipart/form-data; boundary=XYZ")
        .body(Body::from(body))
        .unwrap()
}

async fn send(request: Request<Body>) -> (StatusCode, String) {
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 16).await.unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn multipart_bodies_decode_text_and_files() {
    let (status, body) = send(multipart(
        "/attach",
        &[
            ("note", None, "Two\r\nlines"),
            ("cover", Some("C:\\photos\\cover.txt"), "cover!"),
            ("files", Some("a.txt"), "aaaa"),
            ("files", Some("b.txt"), "bb"),
            ("placebo-base", None, "ignored"),
        ],
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        "\"Two\\nlines\"|Some((\"cover.txt\", Some(\"text/plain\"), b\"cover!\"))|a.txt=4,b.txt=2"
    );
}

#[tokio::test]
async fn an_empty_file_input_is_an_absent_file() {
    let (status, body) = send(multipart(
        "/attach",
        &[
            ("note", None, "Hi"),
            ("cover", Some(""), ""),
            ("files", Some(""), ""),
        ],
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "\"Hi\"|None|");
    let (status, _) = send(multipart("/required", &[("file", Some(""), "")])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = send(multipart("/required", &[("file", Some("x.txt"), "x")])).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "x.txt"));
}

#[tokio::test]
async fn files_over_their_limit_are_refused_before_the_handler() {
    // One cover over 16 bytes, or attachments over 32 bytes together.
    for parts in [
        vec![
            ("note", None, "Hi"),
            ("cover", Some("c.txt"), "0123456789abcdefg"),
        ],
        vec![
            ("note", None, "Hi"),
            ("files", Some("a.txt"), "0123456789abcdef"),
            ("files", Some("b.txt"), "0123456789abcdefg"),
        ],
    ] {
        let (status, body) = send(multipart("/attach", &parts)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body.contains("the most this form accepts"), "{body}");
    }
}

#[tokio::test]
async fn files_and_text_must_arrive_in_their_own_fields() {
    for (parts, path) in [
        // Text where a file belongs.
        (
            vec![("note", None, "Hi"), ("cover", None, "not a file")],
            "/attach",
        ),
        // A file where text belongs.
        (vec![("note", Some("n.txt"), "Hi")], "/attach"),
        // A file for no declared field.
        (
            vec![("note", None, "Hi"), ("other", Some("o.txt"), "o")],
            "/attach",
        ),
    ] {
        let (status, _) = send(multipart(path, &parts)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{parts:?}");
    }
    // An urlencoded body cannot name a file.
    let (status, _) = send(
        Request::post("/required")
            .header("x-placebo-request", placebo::VERSION.to_string())
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("file=placebo-upload%3A0"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn only_payloads_with_file_fields_read_multipart() {
    let (status, body) = send(multipart("/note", &[("note", None, "Hi")])).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(body.contains("no file fields"), "{body}");
}

#[tokio::test]
async fn a_multipart_body_is_limited_as_a_whole() {
    // Many empty parts stay under the text limit but not under the body's:
    // the files' limits, the text limit, and room for framing.
    let mut parts = vec![("n", None, ""); 30_000];
    parts.insert(0, ("note", None, "Hi"));
    let (status, body) = send(multipart("/attach", &parts)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(body.contains("the most this form's fields accept together"), "{body}");
}

#[test]
fn file_fields_make_the_form_multipart_and_render_their_limits() {
    let fields = fields! { Attach {
        @field note = Control::text("");
        @field cover = Control::file().accept("image/*").required();
        @field files = Control::file();
    } };
    let form = ATTACH
        .bind(&Component::new("board", 1))
        .form(fields)
        .into_string();
    assert!(form.contains("enctype=\"multipart/form-data\""));
    assert!(form.contains(
        "<input type=\"file\" name=\"cover\" accept=\"image/*\" required data-placebo-field=\"cover\" data-placebo-max-bytes=\"16\">"
    ));
    assert!(form.contains(
        "<input type=\"file\" name=\"files\" multiple data-placebo-field=\"files\" data-placebo-max-bytes=\"32\">"
    ));
    // Files cannot be shown again, so they take no part in the edit hash.
    assert!(!form.contains("cover%3D") && !form.contains("files%3D"));
}

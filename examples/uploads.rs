//! File uploads: an optional cover image and any number of attachments, with
//! size limits in the payload type. State is in memory.
use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormInput, Input, MutationAction, Upload, fields};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
#[allow(dead_code)]
mod support;

const ATTACH: MutationAction<Attach> = MutationAction::new("attach", "/actions/attach");

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Attach {
    note: String,
    // At most 64 KB. The runtime refuses a larger file before sending it.
    cover: Option<Upload<{ 64 * 1024 }>>,
    // At most 256 KB for all attachments together.
    #[serde(default)]
    files: Vec<Upload<{ 256 * 1024 }>>,
}

struct Saved {
    name: String,
    size: usize,
    kind: String,
}

#[derive(Default)]
struct Board {
    note: String,
    cover: Option<Saved>,
    files: Vec<Saved>,
}
type Store = Arc<Mutex<Board>>;

fn saved<const N: usize>(upload: Upload<N>) -> Saved {
    Saved {
        name: upload.file_name().to_owned(),
        size: upload.len(),
        kind: upload.content_type().unwrap_or("unknown").to_owned(),
    }
}

fn board(board: &Board, note: &str, feedback: &str) -> Markup {
    let fields = fields! { Attach {
        label for="note" { "Note" }
        @field note = Control::text(note).id("note").described_by("feedback")
            .invalid(feedback.starts_with("Write"));
        label for="cover" { "Cover image (up to 64 KB)" }
        @field cover = Control::file().id("cover").accept("image/*");
        label for="files" { "Attachments (up to 256 KB together)" }
        @field files = Control::file().id("files");
        p #feedback role="status" { (feedback) }
        button type="submit" { "Save" }
    } };
    html! {
        h2 #saved-note { (board.note) }
        @if let Some(cover) = &board.cover {
            p #saved-cover { "Cover: " (cover.name) " (" (cover.size) " bytes, " (cover.kind) ")" }
        }
        ul #saved-files {
            @for file in &board.files { li { (file.name) " (" (file.size) " bytes)" } }
        }
        (ATTACH.bind(&Component::new("board", 1)).form(fields))
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let saved = store.lock().unwrap();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Placebo / Uploads" }
                script type="module" src="/placebo.js" {}
            }
            body {
                main {
                    h1 { "Uploads" }
                    (Component::new("board", 1).mount(board(&saved, &saved.note, "")))
                }
            }
        }
    }
}

async fn attach(State(store): State<Store>, Input(input): Input<Attach>) -> Response {
    let binding = ATTACH.bind(&Component::new("board", 1));
    let mut saved = store.lock().unwrap();
    let note = input.note.trim();
    if !(3..=80).contains(&note.chars().count()) {
        // The runtime keeps a chosen file across this reply; a page rendered
        // without JavaScript cannot, so name the files that need choosing again.
        let chosen: Vec<&str> = input
            .cover
            .iter()
            .map(Upload::file_name)
            .chain(input.files.iter().map(Upload::file_name))
            .collect();
        let feedback = if chosen.is_empty() {
            "Write a note of 3 to 80 characters.".to_owned()
        } else {
            format!(
                "Write a note of 3 to 80 characters. Nothing was saved, including {}.",
                chosen.join(", ")
            )
        };
        return binding
            .invalid(board(&saved, &input.note, &feedback))
            .into_response();
    }
    saved.note = note.to_owned();
    if let Some(cover) = input.cover {
        saved.cover = Some(self::saved(cover));
    }
    saved.files.extend(input.files.into_iter().map(self::saved));
    binding
        .reply(board(&saved, &saved.note, "Saved."))
        .into_response()
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(Board {
        note: "A board for files".into(),
        ..Board::default()
    }));
    let app = Router::new()
        .route("/", get(home))
        .route(ATTACH.path(), ATTACH.route(attach))
        .route("/placebo.js", get(placebo::runtime))
        .with_state(store);
    // Forms submitted before the runtime loads, or without JavaScript.
    let app = placebo::native_forms(app);
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4321".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

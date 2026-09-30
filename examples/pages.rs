//! Links between pages. A click shows the next page without a document load,
//! and Back and Forward return to where each page was scrolled. The list links
//! to each note; a note's save goes back to the list. The last links load as
//! usual: one opts out, one is a text file, and one page loads another
//! stylesheet.
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormInput, Input, MutationAction, fields};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

mod support;

struct Note {
    id: u64,
    title: String,
}
type Store = Arc<Mutex<Vec<Note>>>;

#[derive(Deserialize, FormInput)]
struct SaveNote {
    id: u64,
    title: String,
}

const SAVE: MutationAction<SaveNote> = MutationAction::new("save-note", "/notes/save");

fn layout(title: &str, stylesheet: bool, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                @if stylesheet {
                    link rel="stylesheet" href="/pages.css";
                }
                script type="module" src="/placebo.js" {}
            }
            body { (body) }
        }
    }
}

async fn list(State(store): State<Store>) -> Markup {
    let notes = store.lock().unwrap();
    layout(
        "Notes",
        false,
        html! {
            h1 { "Notes" }
            nav {
                @for note in notes.iter() {
                    a href=(format!("/notes/{}", note.id)) { (note.title) } " "
                }
                a href="#end" { "To the end" }
            }
            // Enough of a page to scroll.
            @for line in 1..=80 {
                p { "Line " (line) }
            }
            p id="end" {
                a href="/notes/1" data-placebo-reload { "First note, loaded" } " "
                a href="/notes.txt" { "As text" } " "
                a href="/styled" { "Styled page" }
            }
        },
    )
}

fn editor(note: &Note, feedback: &str) -> Markup {
    let fields = fields! { SaveNote {
        @field id = Control::hidden(note.id);
        label for="title" { "Title" }
        @field title = Control::text(&note.title).id("title");
        p role="status" { (feedback) }
        button type="submit" { "Save" }
    } };
    SAVE.bind(&Component::new("note", note.id)).form(fields)
}

async fn note(State(store): State<Store>, Path(id): Path<u64>) -> Response {
    let notes = store.lock().unwrap();
    let Some(note) = notes.iter().find(|note| note.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    layout(
        &note.title,
        false,
        html! {
            a href="/" { "All notes" }
            h1 { (note.title) }
            (Component::new("note", note.id).mount(editor(note, "")))
        },
    )
    .into_response()
}

async fn save(State(store): State<Store>, Input(input): Input<SaveNote>) -> Response {
    let mut notes = store.lock().unwrap();
    let Some(note) = notes.iter_mut().find(|note| note.id == input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let binding = SAVE.bind(&Component::new("note", note.id));
    if input.title.trim().is_empty() {
        return binding
            .invalid(editor(note, "Give the note a title."))
            .into_response();
    }
    note.title = input.title.trim().to_owned();
    binding
        .reply(editor(note, "Saved."))
        .navigate("/")
        .into_response()
}

async fn styled() -> Markup {
    layout(
        "Styled",
        true,
        html! { h1 { "Styled" } a href="/" { "All notes" } },
    )
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(vec![
        Note {
            id: 1,
            title: "First note".into(),
        },
        Note {
            id: 2,
            title: "Second note".into(),
        },
    ]));
    let app = Router::new()
        .route("/", get(list))
        .route("/notes/{id}", get(note))
        .route(SAVE.path(), SAVE.route(save))
        .route("/notes.txt", get(async || "First note\nSecond note\n"))
        .route("/styled", get(styled))
        .route(
            "/pages.css",
            get(async || support::asset("pages.css", "text/css", include_str!("static/pages.css"))),
        )
        .route("/placebo.js", get(placebo::runtime))
        .with_state(store);
    let app = placebo::native_forms(app);
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4324".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

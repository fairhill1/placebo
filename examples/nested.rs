//! Nested components: a checklist whose contents mount its entries and a
//! notes dialog, each a component with its own form. Saving the checklist
//! refreshes the entries inside it without taking their drafts. State is in
//! memory.
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormInput, Input, MutationAction, fields};
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
#[allow(dead_code)]
mod support;

const SAVE_LIST: MutationAction<SaveList> = MutationAction::new("save-list", "/actions/save-list");
const SAVE_ENTRY: MutationAction<SaveEntry> =
    MutationAction::new("save-entry", "/actions/save-entry");
const SAVE_NOTES: MutationAction<SaveNotes> =
    MutationAction::new("save-notes", "/actions/save-notes");

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct SaveList {
    name: String,
    locked: bool,
    // Keep only the first n entries; lets the tests remove a nested component.
    keep: u32,
}

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct SaveEntry {
    id: u32,
    text: String,
    delay_ms: u64,
}

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct SaveNotes {
    notes: String,
}

struct Entry {
    id: u32,
    text: String,
}

struct Checklist {
    name: String,
    locked: bool,
    notes: String,
    entries: Vec<Entry>,
}
type Store = Arc<Mutex<Checklist>>;

fn entry(entry: &Entry, locked: bool, feedback: &str) -> Markup {
    let fields = fields! { SaveEntry {
        @field id = Control::hidden(entry.id);
        label for=(format!("entry-{}", entry.id)) { "Entry " (entry.id) }
        @field text = Control::text(&entry.text).id(&format!("entry-{}", entry.id));
        @field delay_ms = Control::select(0, [(0, "Now"), (700, "Slowly")])
            .id(&format!("entry-delay-{}", entry.id));
        button type="submit" disabled[locked] { "Save entry" }
        span data-feedback role="status" { (feedback) }
    } };
    SAVE_ENTRY
        .bind(&Component::new("entry", entry.id))
        .form(fields)
}

fn notes(checklist: &Checklist, feedback: &str) -> Markup {
    let fields = fields! { SaveNotes {
        h2 #notes-heading { "Notes for " (checklist.name) }
        @field notes = Control::textarea(&checklist.notes).id("notes").rows(3);
        p role="status" { (feedback) }
        button type="submit" { "Save notes" }
        button type="button" command="close" commandfor="notes:1" { "Close" }
    } };
    SAVE_NOTES.bind(&Component::new("notes", 1)).form(fields)
}

// The checklist's contents mount its nested components. Every reply renders
// them again; the browser refreshes each one by its own rules.
fn checklist(checklist: &Checklist, feedback: &str) -> Markup {
    let fields = fields! { SaveList {
        label for="name" { "Checklist name" }
        @field name = Control::text(&checklist.name).id("name");
        label { @field locked = Control::checkbox(checklist.locked).id("locked"); " Locked" }
        label for="keep" { "Keep entries" }
        @field keep = Control::number(checklist.entries.len() as u32).id("keep").min(0);
        p #list-feedback role="status" { (feedback) }
        button type="submit" { "Save checklist" }
    } };
    html! {
        h2 #list-name { (checklist.name) @if checklist.locked { " (locked)" } }
        (SAVE_LIST.bind(&Component::new("checklist", 1)).form(fields))
        ul #entries {
            @for item in &checklist.entries {
                li { (Component::new("entry", item.id).mount(entry(item, checklist.locked, ""))) }
            }
        }
        button type="button" command="show-modal" commandfor="notes:1" { "Notes" }
        (Component::new("notes", 1).mount_dialog("notes-heading", notes(checklist, "")))
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let list = store.lock().unwrap();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Placebo / Nested components" }
                script type="module" src="/placebo.js" {}
            }
            body {
                main { (Component::new("checklist", 1).mount(checklist(&list, ""))) }
            }
        }
    }
}

async fn save_list(State(store): State<Store>, Input(input): Input<SaveList>) -> Response {
    let mut list = store.lock().unwrap();
    list.name = input.name.trim().to_owned();
    list.locked = input.locked;
    list.entries.truncate(input.keep as usize);
    SAVE_LIST
        .bind(&Component::new("checklist", 1))
        .reply(checklist(&list, "Saved."))
        .into_response()
}

async fn save_entry(State(store): State<Store>, Input(input): Input<SaveEntry>) -> Response {
    tokio::time::sleep(Duration::from_millis(input.delay_ms.min(1500))).await;
    let mut list = store.lock().unwrap();
    let locked = list.locked;
    let Some(item) = list.entries.iter_mut().find(|item| item.id == input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let binding = SAVE_ENTRY.bind(&Component::new("entry", item.id));
    if locked {
        return binding
            .conflict(entry(item, locked, "The checklist is locked."))
            .into_response();
    }
    item.text = input.text.trim().to_owned();
    binding.reply(entry(item, locked, "Saved.")).into_response()
}

async fn save_notes(State(store): State<Store>, Input(input): Input<SaveNotes>) -> Response {
    let mut list = store.lock().unwrap();
    list.notes = input.notes;
    SAVE_NOTES
        .bind(&Component::new("notes", 1))
        .reply(notes(&list, "Saved."))
        .into_response()
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(Checklist {
        name: "Before the trip".into(),
        locked: false,
        notes: String::new(),
        entries: (1..=3)
            .map(|id| Entry {
                id,
                text: format!("Thing {id}"),
            })
            .collect(),
    }));
    let app = Router::new()
        .route("/", get(home))
        .route(SAVE_LIST.path(), SAVE_LIST.route(save_list))
        .route(SAVE_ENTRY.path(), SAVE_ENTRY.route(save_entry))
        .route(SAVE_NOTES.path(), SAVE_NOTES.route(save_notes))
        .route("/placebo.js", get(placebo::runtime))
        .with_state(store);
    // Forms submitted before the runtime loads, or without JavaScript.
    let app = placebo::native_forms(app);
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4322".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

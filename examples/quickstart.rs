use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormInput, MutationAction, fields};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

struct Item {
    id: u64,
    title: String,
    version: u64,
}
type Store = Arc<Mutex<Vec<Item>>>;

#[derive(Deserialize, FormInput)]
struct SaveTitle {
    id: u64,
    title: String,
    version: u64,
}

const SAVE: MutationAction<SaveTitle> = MutationAction::new("save-title", "/save");

// Render the component's CONTENTS, including its form and feedback.
// Both the initial page and save responses reuse this function.
fn editor(item: &Item, feedback: &str) -> Markup {
    let component = Component::new("editor", item.id);
    let title_id = format!("title-{}", item.id);
    let feedback_id = format!("feedback-{}", item.id);
    let fields = fields! { SaveTitle {
        // IDs and versions belong to the server and must refresh on each reply.
        @field id = Control::hidden(item.id);
        @field version = Control::hidden(item.version);
        div data-placebo-local="draft" {
            label for=(title_id) { "Title" }
            @field title = Control::text(&item.title)
                .id(&title_id).described_by(&feedback_id);
        }
        p id=(feedback_id) role="status" { (feedback) }
        button type="submit" { "Save" }
    } };
    html! {
        article {
            h2 { (item.title) }
            (SAVE.bind(&component).form(fields))
        }
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let items = store.lock().unwrap();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "My Placebo app" }
                script type="module" src="/placebo.js" {}
            }
            body {
                h1 { "Two editors" }
                @for item in items.iter() {
                    // Only the initial page adds the component's outer mount.
                    (Component::new("editor", item.id).mount(editor(item, "")))
                }
            }
        }
    }
}

// SAVE.route supplies the state and deserialized SaveTitle directly.
async fn save(store: Store, input: SaveTitle) -> Response {
    let mut items = store.lock().unwrap();
    let Some(item) = items.iter_mut().find(|item| item.id == input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let component = Component::new("editor", item.id);
    let binding = SAVE.bind(&component);
    let title = input.title.trim();
    if !(3..=80).contains(&title.chars().count()) {
        return binding
            .invalid(editor(item, "Use 3–80 characters."))
            .into_response();
    }
    if input.version != item.version {
        return binding
            .conflict(editor(
                item,
                "Changed in another tab. Review the saved title and retry.",
            ))
            .into_response();
    }
    // The version check and write happen under the same lock.
    item.title = title.to_owned();
    item.version += 1;
    binding
        .reply(editor(item, "Saved."))
        .reset_local("draft")
        .into_response()
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(vec![
        Item {
            id: 1,
            title: "First item".into(),
            version: 1,
        },
        Item {
            id: 2,
            title: "Second item".into(),
            version: 1,
        },
    ]));
    let app = Router::new()
        .route("/", get(home))
        .route("/placebo.js", get(placebo::runtime))
        .route(SAVE.path(), SAVE.route(save))
        .with_state(store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("Open http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}

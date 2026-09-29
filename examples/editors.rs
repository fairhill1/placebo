//! Two instances, one save action. State is in memory and resets on restart.
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
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
mod support;

const SAVE: MutationAction<SaveTitle> = MutationAction::new("save-title", "/actions/save-title");

#[derive(Clone)]
struct Item {
    id: u64,
    title: String,
    version: u64,
}
type Store = Arc<Mutex<BTreeMap<u64, Item>>>;

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct SaveTitle {
    id: u64,
    title: String,
    version: u64,
    #[serde(default)]
    delay_ms: u64,
}

enum Feedback<'a> {
    Idle,
    Saved,
    Invalid(&'a str),
    Conflict,
}

fn editor(item: &Item, draft: &str, delay_ms: u64, feedback: Feedback<'_>) -> Markup {
    let component = Component::new("editor", item.id);
    let input_id = format!("title-{}", item.id);
    let error_id = format!("feedback-{}", item.id);
    let fields = fields! { SaveTitle {
        @field id = Control::hidden(item.id);
        @field version = Control::hidden(item.version);
        div {
            label for=(input_id) { "Title" }
            @field title = Control::text(draft).id(&input_id).described_by(&error_id).autocomplete("off")
                .invalid(matches!(feedback, Feedback::Invalid(_)));
            .network {
                label for=(format!("delay-{}", item.id)) { "Response delay" }
                @field delay_ms = Control::select(delay_ms, [(0, "None"), (600, "600 ms")]).id(&format!("delay-{}", item.id));
            }
        }
        p .feedback id=(error_id) role="status" aria-live="polite" {
            @match feedback {
                Feedback::Idle => { "3–80 characters. Your draft stays here until you save." }
                Feedback::Saved => { "Saved. Any newer draft is still yours to edit." }
                Feedback::Invalid(message) => { span .error { (message) } }
                Feedback::Conflict => { span .error { "This item changed elsewhere. Review the saved title above, then save again to apply your draft." } }
            }
        }
        button type="submit" { span .idle-label { "Save title" } span .busy-label { "Saving…" } }
    } };
    html! {
        article .editor data-item=(item.id) {
            .card-heading {
                p .eyebrow { "ITEM " (format!("{:02}", item.id)) }
                span .version { "Version " (item.version) }
            }
            h2 .saved-title { (item.title) }
            (SAVE.bind(&component).form(fields))
        }
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let items: Vec<_> = store.lock().unwrap().values().cloned().collect();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Placebo / Two editors" }
                link rel="stylesheet" href="/demo.css";
                link rel="stylesheet" href="/editors.css";
                script type="module" src="/placebo.js" {}
                script type="module" src="/demo.js" {}
            }
            body {
                main {
                    header {
                        a .wordmark href="/" { "placebo" span { " / lab 002" } }
                        span .badge { "Shared action · separate state" }
                    }
                    .intro {
                        p .eyebrow { "A component has a life." }
                        h1 { "The page changes." br; "Your draft stays." }
                        p .lede { "Edit either title. Try a short one to trigger validation, or add a delay and keep typing while a save is in flight. Each card keeps its own draft." }
                    }
                    .panels {
                        @for item in &items {
                            (Component::new("editor", item.id).mount(editor(item, &item.title, 0, Feedback::Idle)))
                        }
                    }
                    details .trace {
                        summary { "Interaction trace" }
                        p { "Both cards use the same action. Duplicate submissions wait for you to try again after the current save settles." }
                        ol #trace role="log" aria-live="polite" {}
                    }
                    footer { "Experiment 002 · In-memory edits reset when the server restarts" }
                }
            }
        }
    }
}

async fn save(State(store): State<Store>, Input(input): Input<SaveTitle>) -> Response {
    tokio::time::sleep(Duration::from_millis(input.delay_ms.min(1500))).await;
    let mut items = store.lock().unwrap();
    let Some(item) = items.get_mut(&input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let component = Component::new("editor", item.id);
    let binding = SAVE.bind(&component);
    let title = input.title.trim();
    if !(3..=80).contains(&title.chars().count()) {
        return binding
            .invalid(editor(
                item,
                &input.title,
                input.delay_ms,
                Feedback::Invalid("Use between 3 and 80 characters."),
            ))
            .into_response();
    }
    // Compare and update under one lock. The browser's exclusive policy only
    // covers one mounted instance; this protects against writes from other tabs.
    if input.version != item.version {
        return binding
            // Render the saved record: the browser keeps the fields this
            // person edited and shows the other tab's values in the rest.
            .conflict(editor(
                item,
                &item.title,
                input.delay_ms,
                Feedback::Conflict,
            ))
            .into_response();
    }
    item.title = title.to_owned();
    item.version += 1;
    binding
        .reply(editor(item, &input.title, input.delay_ms, Feedback::Saved))
        .into_response()
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(
        [
            (
                1,
                Item {
                    id: 1,
                    title: "A quiet workspace".into(),
                    version: 1,
                },
            ),
            (
                2,
                Item {
                    id: 2,
                    title: "An afternoon outside".into(),
                    version: 1,
                },
            ),
        ]
        .into(),
    ));
    let app = Router::new()
        .route("/", get(home))
        .route(SAVE.path(), SAVE.route(save))
        .route("/placebo.js", get(placebo::runtime))
        .route(
            "/demo.css",
            get(async || support::asset("demo.css", "text/css", include_str!("static/demo.css"))),
        )
        .route(
            "/editors.css",
            get(async || {
                support::asset(
                    "editors.css",
                    "text/css",
                    include_str!("static/editors.css"),
                )
            }),
        )
        .route(
            "/demo.js",
            get(async || {
                support::asset("demo.js", "text/javascript", include_str!("static/demo.js"))
            }),
        )
        .with_state(store);
    // Forms submitted before the runtime loads, or without JavaScript.
    let app = placebo::native_forms(app);
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4318".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

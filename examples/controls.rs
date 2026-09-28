//! One profile form using every typed control. State is in memory.
use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormEnum, FormInput, MutationAction, fields};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
#[allow(dead_code)]
mod support;

const SAVE: MutationAction<SaveProfile> =
    MutationAction::new("save-profile", "/actions/save-profile");

#[derive(Clone, Debug, Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct SaveProfile {
    name: String,
    email: String,
    nickname: Option<String>,
    bio: String,
    age: Option<u32>,
    height_m: f64,
    newsletter: bool,
    plan: Plan,
    role: Option<u8>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    days: Vec<u8>,
    birthday: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, FormEnum)]
#[serde(rename_all = "lowercase")]
enum Plan {
    Free,
    Pro,
}

type Store = Arc<Mutex<SaveProfile>>;
const DAYS: [(u8, &str); 3] = [(5, "Friday"), (6, "Saturday"), (7, "Sunday")];

fn editor(saved: &SaveProfile, draft: &SaveProfile, feedback: Option<&str>) -> Markup {
    let component = Component::new("profile", 1);
    let d = draft.clone();
    let fields = fields! { SaveProfile {
        div data-placebo-local="draft" {
            label for="name" { "Name" }
            @field name = Control::text(d.name).id("name").autocomplete("name");
            label for="email" { "Email" }
            @field email = Control::email(d.email).id("email");
            label for="nickname" { "Nickname (optional)" }
            @field nickname = Control::text(d.nickname).id("nickname");
            label for="bio" { "Bio" }
            @field bio = Control::textarea(d.bio).id("bio").rows(3);
            label for="age" { "Age (optional)" }
            @field age = Control::number(d.age).id("age").min(0).max(150);
            label for="height" { "Height (m)" }
            @field height_m = Control::number(d.height_m).id("height").min(0.5).max(2.5);
            label for="birthday" { "Birthday (optional)" }
            @field birthday = Control::date(d.birthday).id("birthday");
            label { @field newsletter = Control::checkbox(d.newsletter).id("newsletter"); " Newsletter" }
            label for="plan" { "Plan" }
            @field plan = Control::select(d.plan, [(Plan::Free, "Free"), (Plan::Pro, "Pro")]).id("plan");
            fieldset {
                legend { "Role" }
                @field role = Control::radios(d.role, [(Some(1), "Owner"), (Some(2), "Editor")]).id("role");
            }
            label for="topics" { "Topics" }
            @field topics = Control::multi_select(d.topics, [
                ("rust".to_owned(), "Rust"), ("web".to_owned(), "Web"), ("ops".to_owned(), "Ops"),
            ]).id("topics");
            fieldset {
                legend { "Available" }
                @field days = Control::checkboxes(d.days, DAYS).id("days");
            }
        }
        p #feedback role="status" { @if let Some(message) = feedback { (message) } }
        button type="submit" { "Save profile" }
    } };
    html! {
        pre #saved { (format!("{saved:?}")) }
        (SAVE.bind(&component).form(fields))
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let saved = store.lock().unwrap().clone();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { "Placebo / Controls" }
                script type="module" src="/placebo.js" {}
            }
            body {
                main { (Component::new("profile", 1).mount(editor(&saved, &saved, None))) }
            }
        }
    }
}

async fn save(store: Store, mut input: SaveProfile) -> Response {
    let component = Component::new("profile", 1);
    let binding = SAVE.bind(&component);
    let mut saved = store.lock().unwrap();
    input.name = input.name.trim().to_owned();
    if input.name.is_empty() {
        return binding
            .invalid(editor(&saved, &input, Some("Enter a name.")))
            .into_response();
    }
    *saved = input;
    binding
        .reply(editor(&saved, &saved, Some("Saved.")))
        .reset_local("draft")
        .into_response()
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(SaveProfile {
        name: "Ada".into(),
        email: "ada@example.com".into(),
        nickname: None,
        bio: "Writes programs.".into(),
        age: Some(36),
        height_m: 1.65,
        newsletter: true,
        plan: Plan::Free,
        role: None,
        topics: vec!["rust".into()],
        days: vec![6],
        birthday: None,
    }));
    let app = Router::new()
        .route("/", get(home))
        .route(SAVE.path(), SAVE.route(save))
        .route("/placebo.js", get(placebo::runtime))
        .with_state(store);
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4320".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

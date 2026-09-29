//! A small task list: coordinated fragments, revisioned summaries, dialogs,
//! and a keyed list whose rows can be added, deleted, and reordered.
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{
    Component, Control, FormEnum, FormInput, Input, List, MutationAction, MutationBinding,
    Position, VersionedRegion, fields,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
mod support;

const LIST: List = List::new("tasks");
const SUMMARY: VersionedRegion = VersionedRegion::new("task-count");
const ADD: MutationAction<AddTask> = MutationAction::new("add-task", "/actions/add-task");
const SAVE: MutationAction<SaveTask> = MutationAction::new("save-task", "/actions/save-task");
const DELETE: MutationAction<DeleteTask> =
    MutationAction::new("delete-task", "/actions/delete-task");
const MOVE: MutationAction<MoveTask> = MutationAction::new("move-task", "/actions/move-task");

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct AddTask {
    title: String,
}

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct SaveTask {
    id: u64,
    version: u64,
    title: String,
    done: bool,
    delay_ms: u64,
}

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct DeleteTask {
    id: u64,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, FormEnum)]
#[serde(rename_all = "lowercase")]
enum Direction {
    Up,
    Down,
}

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct MoveTask {
    id: u64,
    direction: Direction,
}

struct Task {
    id: u64,
    title: String,
    done: bool,
    version: u64,
}
struct Tasks {
    items: BTreeMap<u64, Task>,
    order: Vec<u64>,
    revision: u64,
    next_id: u64,
}

impl Tasks {
    fn in_order(&self) -> impl Iterator<Item = &Task> {
        self.order.iter().map(|id| &self.items[id])
    }
}
type Store = Arc<Mutex<Tasks>>;

fn row_summary(task: &Task) -> VersionedRegion {
    VersionedRegion::keyed("task-summary", task.id)
}

// The view's forms and the handlers' replies share these bindings, so the
// regions a form declares are the ones its replies may patch.
fn save_binding(task: &Task) -> MutationBinding<SaveTask> {
    SAVE.bind(&Component::new("task", task.id))
        .affects(row_summary(task))
        .affects(SUMMARY)
}

fn delete_binding(id: u64) -> MutationBinding<DeleteTask> {
    DELETE
        .bind(&Component::new("task", id))
        .affects(LIST)
        .affects(SUMMARY)
}

fn move_binding(id: u64) -> MutationBinding<MoveTask> {
    MOVE.bind(&Component::new("task-order", id)).affects(LIST)
}

fn add_binding() -> MutationBinding<AddTask> {
    ADD.bind(&Component::new("composer", "new"))
        .affects(LIST)
        .affects(SUMMARY)
}

fn count(tasks: &Tasks) -> Markup {
    let completed = tasks.items.values().filter(|task| task.done).count();
    html! {
        p .count aria-live="polite" { strong { (completed) } " of " (tasks.items.len()) " complete" }
        progress value=(completed) max=(tasks.items.len().max(1)) aria-label="Completed tasks" {}
    }
}

fn summary(task: &Task) -> Markup {
    html! {
        .task-overview {
            span .status-mark data-done=(task.done) aria-hidden="true" { @if task.done { "✓" } @else { "○" } }
            .task-copy {
                p .task-title { (&task.title) }
                p .task-meta { @if task.done { "Complete" } @else { "To do" } " · Task " (task.id) }
            }
            button .secondary type="button" data-dialog-open { "Edit" span .sr-only { " task " (task.id) } }
        }
    }
}

fn edit_form(task: &Task, draft: &str, done: bool, feedback: &str) -> Markup {
    let title_id = format!("title-{}", task.id);
    let feedback_id = format!("feedback-{}", task.id);
    let fields = fields! { SaveTask {
        @field id = Control::hidden(task.id);
        @field version = Control::hidden(task.version);
        label for=(title_id) { "Task title" }
        @field title = Control::text(draft).id(&title_id).described_by(&feedback_id).autocomplete("off");
        .field {
            label for=(format!("done-{}", task.id)) { "Status" }
            @field done = Control::select(done, [(false, "To do"), (true, "Complete")]).id(&format!("done-{}", task.id));
        }
        .network {
            label for=(format!("delay-{}", task.id)) { "Simulate a slow save" }
            @field delay_ms = Control::select(0, [(0, "Off"), (700, "700 ms")]).id(&format!("delay-{}", task.id));
        }
        p .feedback id=(feedback_id) role="status" aria-live="polite" { (feedback) }
        .form-actions {
            button type="submit" { span .idle-label { "Save changes" } span .busy-label { "Saving…" } }
            button .secondary type="button" data-dialog-close { "Cancel" }
        }
    } };
    let delete = fields! { DeleteTask {
        @field id = Control::hidden(task.id);
        button .danger type="submit" { "Delete task" }
    } };
    html! {
        (save_binding(task).form(fields))
        (delete_binding(task.id).form(delete))
    }
}

fn order_controls(id: u64) -> Markup {
    let button = |direction: Direction, label: &str, name: &str| {
        let fields = fields! { MoveTask {
            @field id = Control::hidden(id);
            @field direction = Control::hidden(direction);
            button .secondary .move type="submit" aria-label=(format!("{name} task {id}")) { (label) }
        } };
        move_binding(id).form(fields)
    };
    html! {
        (button(Direction::Up, "↑", "Move up"))
        (button(Direction::Down, "↓", "Move down"))
    }
}

fn row(task: &Task) -> placebo::MountedItem {
    let component = Component::new("task", task.id);
    LIST.item(task.id).mount(html! {
        article .task-row data-placebo-behavior="dialog" data-owner=(component.id()) data-task=(task.id) {
            (row_summary(task).mount(task.version, summary(task)))
            (Component::new("task-order", task.id).class("task-order").mount(order_controls(task.id)))
            dialog aria-labelledby=(format!("dialog-title-{}", task.id)) {
                .dialog-heading {
                    .eyebrow { "TASK " (format!("{:02}", task.id)) }
                    h2 id=(format!("dialog-title-{}", task.id)) { "Make it yours." }
                    p { "Save updates the list. Cancel keeps your draft for later." }
                }
                (component.mount(edit_form(task, &task.title, task.done, "Use 3–80 characters.")))
            }
        }
    })
}

fn add_form(draft: &str, feedback: &str) -> Markup {
    let fields = fields! { AddTask {
        label for="new-title" { "Task title" }
        @field title = Control::text(draft).id("new-title").described_by("new-feedback").autocomplete("off");
        p #new-feedback .feedback role="status" aria-live="polite" { (feedback) }
        .form-actions {
            button type="submit" { span .idle-label { "Add task" } span .busy-label { "Adding…" } }
            button .secondary type="button" data-dialog-close { "Cancel" }
        }
    } };
    html! {
        .dialog-heading { p .eyebrow { "A FRESH START" } h2 #add-heading { "What’s next?" } p { "Give it a name. You can work out the rest later." } }
        (add_binding().form(fields))
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let tasks = store.lock().unwrap();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Placebo / A little room to focus" }
                link rel="stylesheet" href="/demo.css";
                link rel="stylesheet" href="/tasks.css";
                script type="module" src="/tasks.js" {}
                script type="module" src="/demo.js" {}
            }
            body {
                main {
                    header {
                        a .wordmark href="/" { "placebo" span { " / lab 003" } }
                        span .badge { "Small steps, shared progress" }
                    }
                    .intro {
                        p .eyebrow { "A LITTLE ROOM TO FOCUS" }
                        h1 { "One thing" br; "at a time." }
                        p .lede { "A short list for the day. Make a little progress, leave a thought unfinished, and pick it up when you’re ready." }
                    }
                    section .workspace aria-label="Today's tasks" {
                        .list-heading {
                            div { p .eyebrow { "YOUR DAY" } h2 { "The small things" } }
                            section data-placebo-behavior="dialog" data-owner="composer:new" {
                                button #add-task type="button" data-dialog-open { "+ Add task" }
                                (Component::new("composer", "new").mount_dialog("add-heading", add_form("", "Use 3–80 characters.")))
                            }
                        }
                        (SUMMARY.mount(tasks.revision, count(&tasks)))
                        (LIST.mount(html! { @for task in tasks.in_order() { (row(task)) } }))
                    }
                    p .hint { "Tip: Cancel keeps an unfinished edit. Save accepts the cleaned-up title unless you’ve already started typing something newer." }
                    details .trace { summary { "Interaction trace" } p { "Follow requests and applied updates while you try the list." } ol #trace role="log" aria-label="Interaction events" {} }
                    footer { "Experiment 003 · In-memory tasks reset when the server restarts" }
                }
            }
        }
    }
}

fn normalized(title: &str) -> Option<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (3..=80).contains(&title.chars().count()).then_some(title)
}

async fn add(State(store): State<Store>, Input(input): Input<AddTask>) -> Response {
    let binding = add_binding();
    let Some(title) = normalized(&input.title) else {
        return binding
            .invalid(add_form(&input.title, "Use between 3 and 80 characters."))
            .into_response();
    };
    let mut tasks = store.lock().unwrap();
    let id = tasks.next_id;
    tasks.next_id += 1;
    let task = Task {
        id,
        title,
        done: false,
        version: 1,
    };
    let new_row = row(&task);
    tasks.items.insert(id, task);
    tasks.order.push(id);
    tasks.revision += 1;
    binding
        .reply(add_form("", "Ready for the next task."))
        .also_insert(new_row, Position::End)
        .also_replace(SUMMARY, tasks.revision, count(&tasks))
        .into_response()
}

async fn save(State(store): State<Store>, Input(input): Input<SaveTask>) -> Response {
    tokio::time::sleep(Duration::from_millis(input.delay_ms.min(1500))).await;
    let mut tasks = store.lock().unwrap();
    let Some(task) = tasks.items.get_mut(&input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let binding = save_binding(task);
    let Some(title) = normalized(&input.title) else {
        return binding
            .invalid(edit_form(
                task,
                &input.title,
                input.done,
                "Use between 3 and 80 characters.",
            ))
            .into_response();
    };
    if input.version != task.version {
        // The saved task: fields this person edited keep their edits.
        let response = binding.conflict(edit_form(task, &task.title, task.done,
            "This task changed elsewhere. Your draft is safe. Review the current task in the list before saving again."))
            .also_replace(row_summary(task), task.version, summary(task));
        return response
            .also_replace(SUMMARY, tasks.revision, count(&tasks))
            .into_response();
    }
    task.title = title;
    task.done = input.done;
    task.version += 1;
    let response = binding
        .reply(edit_form(
            task,
            &task.title,
            task.done,
            "Saved. Any newer draft is still yours to edit.",
        ))
        .also_replace(row_summary(task), task.version, summary(task));
    tasks.revision += 1;
    response
        .also_replace(SUMMARY, tasks.revision, count(&tasks))
        .into_response()
}

async fn delete(State(store): State<Store>, Input(input): Input<DeleteTask>) -> Response {
    let mut tasks = store.lock().unwrap();
    let binding = delete_binding(input.id);
    // Deleting twice, from two tabs, removes a row that is already gone.
    if tasks.items.remove(&input.id).is_some() {
        tasks.order.retain(|id| *id != input.id);
        tasks.revision += 1;
    }
    binding
        .reply(html! { p { "Deleted." } })
        .also_remove(&LIST.item(input.id))
        .also_replace(SUMMARY, tasks.revision, count(&tasks))
        .into_response()
}

async fn move_task(State(store): State<Store>, Input(input): Input<MoveTask>) -> Response {
    let mut tasks = store.lock().unwrap();
    let binding = move_binding(input.id);
    let Some(from) = tasks.order.iter().position(|id| *id == input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let to = match input.direction {
        Direction::Up => from.saturating_sub(1),
        Direction::Down => (from + 1).min(tasks.order.len() - 1),
    };
    let reply = binding.reply(order_controls(input.id));
    if from == to {
        return reply.into_response();
    }
    tasks.order.swap(from, to);
    // Placing it next to its new neighbour is enough; also_order(&LIST, ...)
    // would send the whole order instead.
    let position = match input.direction {
        Direction::Up => Position::Before(LIST.item(tasks.order[to + 1])),
        Direction::Down => Position::After(LIST.item(tasks.order[to - 1])),
    };
    reply
        .also_move(&LIST.item(input.id), position)
        .into_response()
}

#[tokio::main]
async fn main() {
    let store = Arc::new(Mutex::new(Tasks {
        items: [
            Task {
                id: 1,
                title: "Make room for a good idea".into(),
                done: false,
                version: 1,
            },
            Task {
                id: 2,
                title: "Take the long way home".into(),
                done: false,
                version: 1,
            },
            Task {
                id: 3,
                title: "Write a few lines, just for you".into(),
                done: true,
                version: 1,
            },
        ]
        .into_iter()
        .map(|task| (task.id, task))
        .collect(),
        order: vec![1, 2, 3],
        revision: 1,
        next_id: 4,
    }));
    let app = Router::new()
        .route("/", get(home))
        .route(ADD.path(), ADD.route(add))
        .route(SAVE.path(), SAVE.route(save))
        .route(DELETE.path(), DELETE.route(delete))
        .route(MOVE.path(), MOVE.route(move_task))
        .route("/placebo.js", get(placebo::runtime))
        .route(
            "/tasks.js",
            get(async || {
                support::asset(
                    "tasks.js",
                    "text/javascript",
                    include_str!("static/tasks.js"),
                )
            }),
        )
        .route(
            "/tasks.css",
            get(async || support::asset("tasks.css", "text/css", include_str!("static/tasks.css"))),
        )
        .route(
            "/demo.css",
            get(async || support::asset("demo.css", "text/css", include_str!("static/demo.css"))),
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
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4319".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

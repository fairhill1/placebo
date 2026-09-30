//! The starter's demo: a task list and a page per task. Its seeded tasks are a
//! tour of what Placebo does. AGENTS.md says how to remove it.
use crate::{App, Failed, layout};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{Markup, html};
use placebo::{Component, Control, FormInput, Input, MutationAction, fields, icon};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(sqlx::FromRow)]
struct Task {
    id: i64,
    title: String,
    notes: String,
    done: bool,
    version: i64,
}

#[derive(Deserialize, FormInput)]
struct AddTask {
    title: String,
}

#[derive(Deserialize, FormInput)]
struct MarkTask {
    id: i64,
    done: bool,
}

#[derive(Deserialize, FormInput)]
struct DeleteTask {
    id: i64,
}

#[derive(Deserialize, FormInput)]
struct SaveTask {
    id: i64,
    version: i64,
    title: String,
    notes: String,
}

const ADD: MutationAction<AddTask> = MutationAction::new("add-task", "/tasks/add");
const MARK: MutationAction<MarkTask> = MutationAction::new("mark-task", "/tasks/mark");
const DELETE: MutationAction<DeleteTask> = MutationAction::new("delete-task", "/tasks/delete");
const SAVE: MutationAction<SaveTask> = MutationAction::new("save-task", "/tasks/save");

pub fn routes() -> Router<App> {
    Router::new()
        .route("/", get(list))
        .route("/tasks/{id}", get(page))
        .route(ADD.path(), ADD.route(add))
        .route(MARK.path(), MARK.route(mark))
        .route(DELETE.path(), DELETE.route(delete))
        .route(SAVE.path(), SAVE.route(save))
}

async fn task(db: &PgPool, id: i64) -> Result<Option<Task>, sqlx::Error> {
    sqlx::query_as("SELECT id, title, notes, done, version FROM tasks WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// A title with its spaces tidied, when it has 1 to 80 characters.
fn tidy(title: &str) -> Option<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (1..=80).contains(&title.chars().count()).then_some(title)
}

// The list page

async fn list(State(app): State<App>, headers: HeaderMap) -> Result<Markup, Failed> {
    let tasks: Vec<Task> =
        sqlx::query_as("SELECT id, title, notes, done, version FROM tasks ORDER BY id")
            .fetch_all(&app.db)
            .await?;
    let done = tasks.iter().filter(|task| task.done).count();
    Ok(layout(
        &headers,
        "Tasks",
        // Every save renders the page again, so this count follows.
        html! { (tasks.len() - done) " open · " (done) " done" },
        html! {
            (app.live.mount())
            section .card {
                (Component::new("composer", "new").mount(add_form("", "", false)))
            }
            @if tasks.is_empty() {
                p .notice { (icon!("list-todo")) "Nothing to do. Add a task above." }
            } @else {
                section .card {
                    ul .stack role="list" style="--stack-space: var(--space-xs)" {
                        @for task in &tasks {
                            li .cluster .cluster-between {
                                div .cluster {
                                    (Component::new("mark", task.id).mount(mark_form(task.id, task.done)))
                                    @if task.done {
                                        a .muted href=(format!("/tasks/{}", task.id)) { (task.title) }
                                    } @else {
                                        a href=(format!("/tasks/{}", task.id)) { (task.title) }
                                    }
                                }
                                (Component::new("delete", task.id).mount(delete_form(task)))
                            }
                        }
                    }
                }
            }
        },
    ))
}

fn add_form(draft: &str, feedback: &str, invalid: bool) -> Markup {
    let fields = fields! { AddTask {
        div .stack style="--stack-space: var(--space-sm)" {
            div .field {
                label for="new-task" { "New task" }
                @field title = Control::text(draft).id("new-task")
                    .described_by("new-task-feedback").autocomplete("off").invalid(invalid);
            }
            div .cluster {
                button .btn type="submit" { (icon!("plus")) "Add task" }
                p .muted #new-task-feedback role="status" { (feedback) }
            }
        }
    } };
    ADD.bind(&Component::new("composer", "new")).form(fields)
}

async fn add(State(app): State<App>, Input(input): Input<AddTask>) -> Result<Response, Failed> {
    let binding = ADD.bind(&Component::new("composer", "new"));
    let Some(title) = tidy(&input.title) else {
        return Ok(binding
            .invalid(add_form(&input.title, "Give the task a name.", true))
            .into_response());
    };
    sqlx::query("INSERT INTO tasks (title) VALUES ($1)")
        .bind(title)
        .execute(&app.db)
        .await?;
    app.live.changed();
    Ok(binding.reply(add_form("", "", false)).into_response())
}

// A round button that marks the task done, or open again.
fn mark_form(id: i64, done: bool) -> Markup {
    let fields = fields! { MarkTask {
        @field id = Control::hidden(id);
        @field done = Control::hidden(!done);
        button .btn .btn-ghost .btn-icon type="submit" aria-pressed=(done) {
            @if done { (icon!("circle-check-big")) } @else { (icon!("circle")) }
            span .visually-hidden { "Done" }
        }
    } };
    MARK.bind(&Component::new("mark", id)).form(fields)
}

async fn mark(State(app): State<App>, Input(input): Input<MarkTask>) -> Result<Response, Failed> {
    // Marking leaves the version alone: it cannot clash with an edit.
    sqlx::query("UPDATE tasks SET done = $2 WHERE id = $1")
        .bind(input.id)
        .bind(input.done)
        .execute(&app.db)
        .await?;
    app.live.changed();
    Ok(MARK
        .bind(&Component::new("mark", input.id))
        .reply(mark_form(input.id, input.done))
        .into_response())
}

fn delete_form(task: &Task) -> Markup {
    let fields = fields! { DeleteTask {
        @field id = Control::hidden(task.id);
        button .btn .btn-ghost .btn-icon type="submit" {
            (icon!("trash"))
            span .visually-hidden { "Delete " (task.title) }
        }
    } };
    DELETE.bind(&Component::new("delete", task.id)).form(fields)
}

async fn delete(
    State(app): State<App>,
    Input(input): Input<DeleteTask>,
) -> Result<Response, Failed> {
    // Deleting twice, from two tabs, deletes nothing the second time.
    let deleted = sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(input.id)
        .execute(&app.db)
        .await?;
    if deleted.rows_affected() > 0 {
        app.live.changed();
    }
    // The row is gone, so the reply shows the list again.
    Ok(DELETE
        .bind(&Component::new("delete", input.id))
        .reply(html! {})
        .navigate("/")
        .into_response())
}

// A task's page

async fn page(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, Failed> {
    let Some(task) = task(&app.db, id).await? else {
        let missing = html! {
            (app.live.mount())
            p .notice { (icon!("list-todo")) "This task was deleted." }
        };
        let back = html! { a href="/" { (icon!("arrow-left")) " All tasks" } };
        return Ok((
            StatusCode::NOT_FOUND,
            layout(&headers, "Not found", back, missing),
        )
            .into_response());
    };
    Ok(layout(
        &headers,
        &task.title,
        html! {
            a href="/" { (icon!("arrow-left")) " All tasks" }
            " · " @if task.done { "Done" } @else { "Open" }
        },
        html! {
            (app.live.mount())
            section .card {
                (Component::new("editor", task.id).mount(editor(&task, "", false)))
            }
        },
    )
    .into_response())
}

// Render the component's CONTENTS, including its form and feedback.
// The page and every save reply use this function.
fn editor(task: &Task, feedback: &str, invalid: bool) -> Markup {
    let title_id = format!("title-{}", task.id);
    let notes_id = format!("notes-{}", task.id);
    let feedback_id = format!("feedback-{}", task.id);
    let fields = fields! { SaveTask {
        @field id = Control::hidden(task.id);
        @field version = Control::hidden(task.version);
        div .stack style="--stack-space: var(--space-sm)" {
            div .field {
                label for=(title_id) { "Title" }
                @field title = Control::text(&task.title)
                    .id(&title_id).described_by(&feedback_id).invalid(invalid);
            }
            div .field {
                label for=(notes_id) { "Notes" }
                @field notes = Control::textarea(&task.notes).id(&notes_id).rows(6);
            }
            div .cluster {
                button .btn type="submit" { (icon!("check")) "Save" }
                p .muted id=(feedback_id) role="status" { (feedback) }
            }
        }
    } };
    SAVE.bind(&Component::new("editor", task.id)).form(fields)
}

async fn save(State(app): State<App>, Input(input): Input<SaveTask>) -> Result<Response, Failed> {
    let Some(current) = task(&app.db, input.id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let binding = SAVE.bind(&Component::new("editor", input.id));
    let Some(title) = tidy(&input.title) else {
        let submitted = Task {
            title: input.title,
            notes: input.notes,
            ..current
        };
        return Ok(binding
            .invalid(editor(&submitted, "Give the task a name.", true))
            .into_response());
    };
    // The version check and the write are one statement, so two saves of the
    // same version cannot both succeed.
    let saved: Option<Task> = sqlx::query_as(
        "UPDATE tasks SET title = $1, notes = $2, version = version + 1
         WHERE id = $3 AND version = $4
         RETURNING id, title, notes, done, version",
    )
    .bind(title)
    .bind(&input.notes)
    .bind(input.id)
    .bind(input.version)
    .fetch_optional(&app.db)
    .await?;
    let Some(saved) = saved else {
        // The saved task: fields this person edited keep their edits.
        let Some(current) = task(&app.db, input.id).await? else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        return Ok(binding
            .conflict(editor(
                &current,
                "Someone saved this task first. Your changes are still here; save again to keep them.",
                false,
            ))
            .into_response());
    };
    app.live.changed();
    Ok(binding
        .reply(editor(&saved, "Saved.", false))
        .into_response())
}

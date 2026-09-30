//! A Placebo app on Postgres. AGENTS.md has the rules for building on it.
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormEnum, FormInput, Input, MutationAction, fields, icon};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::services::ServeDir;

#[derive(sqlx::FromRow)]
struct Item {
    id: i64,
    title: String,
    version: i64,
}

#[derive(Deserialize, FormInput)]
struct SaveTitle {
    id: i64,
    title: String,
    version: i64,
}

const SAVE: MutationAction<SaveTitle> = MutationAction::new("save-title", "/save");

/// The kit's colour scheme, kept per browser in a cookie. The page renders it
/// as `data-theme` on `<html>`.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, FormEnum)]
#[serde(rename_all = "lowercase")]
enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    /// Each theme with its cookie value and its button's label.
    const ALL: [(Theme, &str, &str); 3] = [
        (Theme::System, "system", "System"),
        (Theme::Light, "light", "Light"),
        (Theme::Dark, "dark", "Dark"),
    ];

    fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(theme, ..)| *theme == self)
            .unwrap()
            .1
    }

    fn from_cookies(headers: &HeaderMap) -> Self {
        let chosen = headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|cookies| cookies.split(';'))
            .find_map(|cookie| cookie.trim().strip_prefix("theme="));
        Self::ALL
            .into_iter()
            .find(|(_, name, _)| Some(*name) == chosen)
            .map_or(Theme::System, |(theme, ..)| theme)
    }
}

#[derive(Deserialize, FormInput)]
struct SetTheme {
    theme: Theme,
}

const SET_THEME: MutationAction<SetTheme> = MutationAction::new("set-theme", "/theme");

/// A failed query. It answers 500 and logs the cause on the server.
struct Failed(sqlx::Error);

impl From<sqlx::Error> for Failed {
    fn from(error: sqlx::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for Failed {
    fn into_response(self) -> Response {
        eprintln!("database error: {}", self.0);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

async fn item(db: &PgPool, id: i64) -> Result<Option<Item>, sqlx::Error> {
    sqlx::query_as("SELECT id, title, version FROM items WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

// Render the component's CONTENTS, including its form and feedback.
// Both the page and save replies reuse this function.
fn editor(item: &Item, feedback: &str, invalid: bool) -> Markup {
    let component = Component::new("editor", item.id);
    let title_id = format!("title-{}", item.id);
    let feedback_id = format!("feedback-{}", item.id);
    let fields = fields! { SaveTitle {
        @field id = Control::hidden(item.id);
        @field version = Control::hidden(item.version);
        div .stack style="--stack-space: var(--space-sm)" {
            div .field {
                label for=(title_id) { "Title" }
                @field title = Control::text(&item.title)
                    .id(&title_id).described_by(&feedback_id).invalid(invalid);
            }
            div .cluster {
                button .btn type="submit" { "Save" }
                p .muted id=(feedback_id) role="status" { (feedback) }
            }
        }
    } };
    html! {
        article .card .stack {
            h2 { (item.title) }
            (SAVE.bind(&component).form(fields))
        }
    }
}

// One small form per theme, so each button saves its own choice.
fn theme_picker(current: Theme) -> Markup {
    let component = Component::new("theme", "picker");
    html! {
        div .cluster role="group" aria-label="Theme" style="--cluster-space: var(--space-2xs)" {
            @for (theme, _, label) in Theme::ALL {
                @let symbol = match theme {
                    Theme::System => icon!("monitor"),
                    Theme::Light => icon!("sun"),
                    Theme::Dark => icon!("moon"),
                };
                @let fields = fields! { SetTheme {
                    @field theme = Control::hidden(theme);
                    @if theme == current {
                        button .btn .btn-sm type="submit" aria-pressed="true" { (symbol) (label) }
                    } @else {
                        button .btn .btn-sm .btn-ghost type="submit" aria-pressed="false" { (symbol) (label) }
                    }
                } };
                (SET_THEME.bind(&component).form(fields))
            }
        }
    }
}

async fn set_theme(Input(input): Input<SetTheme>) -> Response {
    let cookie = format!(
        "theme={}; Path=/; Max-Age=31536000; SameSite=Lax",
        input.theme.name()
    );
    let component = Component::new("theme", "picker");
    (
        [(header::SET_COOKIE, cookie)],
        SET_THEME.bind(&component).reply(theme_picker(input.theme)),
    )
        .into_response()
}

async fn home(State(db): State<PgPool>, headers: HeaderMap) -> Result<Markup, Failed> {
    let items: Vec<Item> = sqlx::query_as("SELECT id, title, version FROM items ORDER BY id")
        .fetch_all(&db)
        .await?;
    let theme = Theme::from_cookies(&headers);
    Ok(html! {
        (DOCTYPE)
        html lang="en" data-theme=(theme.name()) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Items" }
                link rel="stylesheet" href="/static/app.css";
                script type="module" src="/placebo.js" {}
            }
            body {
                main .wrapper .page .stack style="--stack-space: var(--space-xl)" {
                    div .cluster .cluster-between {
                        div .stack style="--stack-space: var(--space-2xs)" {
                            h1 { "Items" }
                            p .muted { "Rename an item and save. Open a second tab to see a conflict." }
                        }
                        (Component::new("theme", "picker").mount(theme_picker(theme)))
                    }
                    div .grid {
                        @for item in &items {
                            (Component::new("editor", item.id).mount(editor(item, "", false)))
                        }
                    }
                }
            }
        }
    })
}

async fn save(
    State(db): State<PgPool>,
    Input(input): Input<SaveTitle>,
) -> Result<Response, Failed> {
    let Some(current) = item(&db, input.id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let component = Component::new("editor", input.id);
    let binding = SAVE.bind(&component);
    let title = input.title.trim();
    if !(3..=80).contains(&title.chars().count()) {
        let submitted = Item {
            title: input.title,
            ..current
        };
        return Ok(binding
            .invalid(editor(&submitted, "Use 3–80 characters.", true))
            .into_response());
    }
    // The version check and the write are one statement, so two saves of the
    // same version cannot both succeed.
    let saved: Option<Item> = sqlx::query_as(
        "UPDATE items SET title = $1, version = version + 1
         WHERE id = $2 AND version = $3
         RETURNING id, title, version",
    )
    .bind(title)
    .bind(input.id)
    .bind(input.version)
    .fetch_optional(&db)
    .await?;
    let Some(saved) = saved else {
        let Some(current) = item(&db, input.id).await? else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        return Ok(binding
            .conflict(editor(
                &current,
                "Changed in another tab. Review the saved title and retry.",
                false,
            ))
            .into_response());
    };
    Ok(binding
        .reply(editor(&saved, "Saved.", false))
        .into_response())
}

async fn database() -> PgPool {
    // Each app gets its own database, placebo_<package name>, unless
    // DATABASE_URL names another. It signs in as PGUSER or the shell's user:
    // sqlx's own lookup of the user answers "anonymous" in some sandboxes.
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        let database = format!("placebo_{}", env!("CARGO_PKG_NAME").replace('-', "_"));
        match ["PGUSER", "USER", "USERNAME"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok())
        {
            Some(user) => format!("postgres:///{database}?user={user}"),
            None => format!("postgres:///{database}"),
        }
    });
    #[cfg(debug_assertions)]
    {
        use sqlx::{Postgres, migrate::MigrateDatabase};
        if !Postgres::database_exists(&url)
            .await
            .expect("reach Postgres")
        {
            Postgres::create_database(&url)
                .await
                .expect("create the database");
        }
    }
    let db = PgPool::connect(&url)
        .await
        .expect("connect to the database");
    sqlx::migrate!().run(&db).await.expect("run migrations");
    db
}

#[tokio::main]
async fn main() {
    let db = database().await;
    let app = Router::new()
        .route("/", get(home))
        .route("/placebo.js", get(placebo::runtime))
        .route(SAVE.path(), SAVE.route(save))
        .route(SET_THEME.path(), SET_THEME.route(set_theme))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(db);
    // Saves also work before the runtime loads, or without JavaScript.
    let app = placebo::native_forms(app);
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = placebo::dev::watch(["static"]).expect("watch static files");
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    // PLACEBO_ADDR runs a second app beside this one, as in 127.0.0.1:3001.
    let addr = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("Open http://{addr}");
    axum::serve(listener, app).await.unwrap();
}

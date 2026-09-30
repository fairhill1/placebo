//! A Placebo app on Postgres. AGENTS.md has the rules for building on it.
//!
//! This file is the app's shell: the database, the page layout, and the theme.
//! The starter's task demo is `tasks.rs`; AGENTS.md says how to remove it.
use axum::{
    Router,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, Feed, FormEnum, FormInput, Input, MutationAction, fields, icon};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_http::services::ServeDir;

mod tasks;

/// The app's name, shown in the header and the tab.
const APP: &str = env!("CARGO_PKG_NAME");

#[derive(Clone)]
struct App {
    db: PgPool,
    /// Pages that mount this feed read themselves again after `changed()`.
    live: Feed,
}

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

/// Every page: the header with the app's name and theme, then the content.
fn layout(headers: &HeaderMap, title: &str, content: Markup) -> Markup {
    let theme = Theme::from_cookies(headers);
    html! {
        (DOCTYPE)
        html lang="en" data-theme=(theme.name()) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · " (APP) }
                link rel="stylesheet" href="/static/app.css";
                script type="module" src="/placebo.js" {}
            }
            body {
                main .wrapper .page .stack style="--stack-space: var(--space-xl)" {
                    header .cluster .cluster-between {
                        a href="/" { strong { (APP) } }
                        (Component::new("theme", "picker").mount(theme_picker(theme)))
                    }
                    (content)
                }
            }
        }
    }
}

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
    /// Each theme with its cookie value and its name.
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

    fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(theme, ..)| *theme == self)
            .unwrap()
            .2
    }

    /// The theme a press switches to: System, Light, Dark, and round again.
    fn next(self) -> Self {
        match self {
            Theme::System => Theme::Light,
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::System,
        }
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

// One icon button showing the current theme; each press saves the next one.
fn theme_picker(current: Theme) -> Markup {
    let next = current.next();
    let symbol = match current {
        Theme::System => icon!("monitor"),
        Theme::Light => icon!("sun"),
        Theme::Dark => icon!("moon"),
    };
    let label = format!("Theme: {}. Switch to {}", current.label(), next.label());
    let fields = fields! { SetTheme {
        @field theme = Control::hidden(next);
        button .btn .btn-ghost .btn-icon type="submit" title=(label) {
            (symbol)
            span .visually-hidden { (label) }
        }
    } };
    SET_THEME
        .bind(&Component::new("theme", "picker"))
        .form(fields)
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

async fn database() -> PgPool {
    // Each app gets its own database, placebo_<package name>, unless
    // DATABASE_URL names another. It signs in as PGUSER or the shell's user:
    // sqlx's own lookup of the user answers "anonymous" in some sandboxes.
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        let database = format!("placebo_{}", APP.replace('-', "_"));
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
    let live = Feed::new("live", "/live");
    let state = App {
        db: database().await,
        live: live.clone(),
    };
    let app = Router::new()
        .merge(tasks::routes())
        .route(live.path(), live.route())
        .route("/placebo.js", get(placebo::runtime))
        .route(SET_THEME.path(), SET_THEME.route(set_theme))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);
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

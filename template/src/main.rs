//! A Placebo app on Postgres. AGENTS.md has the rules for building on it.
//!
//! This file is the app's shell (the database, the sidebar, the theme) and its
//! first two pages, Home and Settings.
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use placebo::{Component, Control, FormEnum, FormInput, Input, MutationAction, fields, icon};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

/// The app's name, shown in the sidebar and the tab. Rename it to the app's.
const APP: &str = "Placebo";

#[derive(Clone)]
struct App {
    db: PgPool,
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

// The shell

/// The sidebar's pages: path, name, and icon. A new page adds its row here.
fn pages() -> [(&'static str, &'static str, PreEscaped<&'static str>); 2] {
    [
        ("/", "Home", icon!("house")),
        ("/settings", "Settings", icon!("settings")),
    ]
}

/// Every page: the sidebar, then the page's heading, an optional line under
/// it, and its content. `path` marks the sidebar's current page; `title`
/// names the tab.
fn layout(
    headers: &HeaderMap,
    path: &str,
    title: &str,
    heading: Markup,
    lede: Option<Markup>,
    content: Markup,
) -> Markup {
    let theme = Theme::from_cookies(headers);
    html! {
        (DOCTYPE)
        html lang="en" data-theme=(theme.name()) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                title { (title) " · " (APP) }
                link rel="stylesheet" href="/static/app.css";
                script type="module" src="/placebo.js" {}
                script type="module" src="/static/app.js" {}
            }
            body .shell {
                aside .shell-side {
                    a .shell-brand href="/" {
                        span .shell-mark aria-hidden="true" { (icon!("pill")) }
                        (APP)
                    }
                    nav .nav aria-label="Pages" {
                        @for (href, name, symbol) in pages() {
                            a href=(href) aria-current=[(href == path).then_some("page")] { (symbol) (name) }
                        }
                    }
                    // The slider in the sidebar; the button in a phone's top bar.
                    div .shell-foot {
                        div .shell-wide { (theme_slider(Place::Sidebar, theme)) }
                        div .shell-narrow { (Component::new("theme", "picker").mount(theme_picker(theme))) }
                    }
                }
                main .shell-main {
                    div .wrapper .page .stack style="--stack-space: var(--space-xl)" {
                        header .stack style="--stack-space: var(--space-2xs)" {
                            div .cluster style="--cluster-space: var(--space-2xs)" { (heading) }
                            @if let Some(lede) = lede {
                                p .lede { (lede) }
                            }
                        }
                        (content)
                    }
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
        Self::ALL.iter().find(|(theme, ..)| *theme == self).unwrap().1
    }

    fn label(self) -> &'static str {
        Self::ALL.iter().find(|(theme, ..)| *theme == self).unwrap().2
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

    fn icon(self) -> PreEscaped<&'static str> {
        match self {
            Theme::System => icon!("monitor"),
            Theme::Light => icon!("sun"),
            Theme::Dark => icon!("moon"),
        }
    }

    fn cookie(self) -> String {
        format!("theme={}; Path=/; Max-Age=31536000; SameSite=Lax", self.name())
    }
}

#[derive(Deserialize, FormInput)]
struct SetTheme {
    theme: Theme,
}

/// The top bar's button, which cycles through the themes.
const SET_THEME: MutationAction<SetTheme> = MutationAction::new("set-theme", "/theme");

// One icon button showing the current theme; each press saves the next one.
fn theme_picker(current: Theme) -> Markup {
    let next = current.next();
    let symbol = current.icon();
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
    let component = Component::new("theme", "picker");
    (
        [(header::SET_COOKIE, input.theme.cookie())],
        SET_THEME.bind(&component).reply(theme_picker(input.theme)),
    )
        .into_response()
}

// Home

async fn home(State(app): State<App>, headers: HeaderMap) -> Result<Markup, Failed> {
    let (database, version): (String, String) =
        sqlx::query_as("SELECT current_database(), current_setting('server_version')")
            .fetch_one(&app.db)
            .await?;
    let next = [
        (
            icon!("file-plus"),
            "Add a page",
            "A route and a handler in src/main.rs that renders with layout(), and a row in pages().",
        ),
        (
            icon!("database"),
            "Add a table",
            "A numbered file in migrations/, such as 0001_projects.sql. It runs when the app starts.",
        ),
        (
            icon!("palette"),
            "Style it",
            "The kit's components first (static/kit/README.md), and your own in static/components.css.",
        ),
    ];
    Ok(layout(
        &headers,
        "/",
        "Home",
        html! { h1 { "Welcome to " (APP) } },
        Some(html! { "Your app is running. Ask your agent to build the first feature." }),
        html! {
            div .grid {
                @for (symbol, title, text) in next {
                    section .card {
                        div .stack style="--stack-space: var(--space-xs)" {
                            div .cluster style="--cluster-space: var(--space-xs)" { (symbol) h2 { (title) } }
                            p .muted { (text) }
                        }
                    }
                }
            }
            section .card {
                div .stack style="--stack-space: var(--space-md)" {
                    h2 { "This app" }
                    dl .kv {
                        dt { "Database" } dd { (database) }
                        dt { "Postgres" } dd { (version) }
                        dt { "Address" } dd { (headers.get(header::HOST).and_then(|host| host.to_str().ok()).unwrap_or("")) }
                    }
                }
            }
        },
    ))
}

/// Where a theme slider is: Settings has one, and so does the sidebar, which
/// is on Settings too. The reply goes back to the one that was used.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, FormEnum)]
#[serde(rename_all = "lowercase")]
enum Place {
    Sidebar,
    Settings,
}

#[derive(Deserialize, FormInput)]
struct ChooseTheme {
    theme: Theme,
    place: Place,
}

/// The sliders, which save a theme as soon as it is picked.
const CHOOSE_THEME: MutationAction<ChooseTheme> = MutationAction::new("choose-theme", "/theme/choose");

fn slider(place: Place) -> Component {
    Component::new("theme", if place == Place::Sidebar { "sidebar" } else { "settings" })
}

// The themes as a row of icons; the current one is under the thumb.
fn theme_choice(place: Place, current: Theme) -> Markup {
    let fields = fields! { ChooseTheme {
        @field place = Control::hidden(place);
        div role="group" aria-label="Theme" {
            @field theme = Control::radios(current, Theme::ALL.map(|(theme, _, label)| {
                (theme, html! { (theme.icon()) span .visually-hidden { (label) } })
            })).class("segmented");
        }
    } };
    CHOOSE_THEME.bind(&slider(place)).form(fields)
}

/// A slider on a page: it saves when a theme is picked.
fn theme_slider(place: Place, current: Theme) -> Markup {
    html! {
        div data-placebo-behavior="autosave" { (slider(place).mount(theme_choice(place, current))) }
    }
}

async fn choose_theme(Input(input): Input<ChooseTheme>) -> Response {
    let form = theme_choice(input.place, input.theme);
    (
        [(header::SET_COOKIE, input.theme.cookie())],
        CHOOSE_THEME.bind(&slider(input.place)).reply(form),
    )
        .into_response()
}

// Settings

async fn settings(headers: HeaderMap) -> Markup {
    layout(
        &headers,
        "/settings",
        "Settings",
        html! { h1 { "Settings" } },
        None,
        html! {
            section .card {
                div .stack style="--stack-space: var(--space-md)" {
                    h2 { "Appearance" }
                    div .switch-field {
                        div .stack style="--stack-space: var(--space-3xs)" {
                            h3 { "Theme" }
                            p .muted { "System follows your device." }
                        }
                        (theme_slider(Place::Settings, Theme::from_cookies(&headers)))
                    }
                }
            }
        },
    )
}

// Startup

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
    let state = App {
        db: database().await,
    };
    let app = Router::new()
        .route("/", get(home))
        .route("/settings", get(settings))
        .route("/placebo.js", get(placebo::runtime))
        .route(SET_THEME.path(), SET_THEME.route(set_theme))
        .route(CHOOSE_THEME.path(), CHOOSE_THEME.route(choose_theme))
        // no-cache: browsers check for a newer file on every load (a 304 when
        // there is none), so an edited stylesheet shows on the next reload.
        .nest_service(
            "/static",
            ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                ))
                .service(ServeDir::new("static")),
        )
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

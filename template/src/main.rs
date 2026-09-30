//! A Placebo app on Postgres. AGENTS.md has the rules for building on it.
//!
//! This file is the app's shell (the database, the sidebar, the theme) and its
//! first two pages, Home and Settings. src/auth.rs has the accounts: every page
//! but signing in needs someone signed in.
mod auth;

use auth::User;
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

/// The document around every page's body. `title` names the tab.
fn document(headers: &HeaderMap, title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" class=[(Theme::from_cookies(headers) == Theme::Dark).then_some("dark")] {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                title { (title) " · " (APP) }
                link rel="stylesheet" href="/static/app.css";
                script type="module" src="/placebo.js" {}
                script type="module" src="/static/app.js" {}
            }
            (body)
        }
    }
}

/// A sidebar link: a row with its icon in the sidebar, and under 48rem
/// (Tailwind's `md`) a tab in the bar along the bottom, icon over name.
const NAV_LINK: &str = "flex flex-col items-center gap-1 py-2 text-xs font-medium \
    text-muted-foreground hover:text-foreground aria-[current=page]:text-foreground [&>svg]:size-5 \
    md:flex-row md:gap-2 md:rounded-md md:px-2 md:py-1.5 md:text-sm md:hover:bg-sidebar-accent \
    md:aria-[current=page]:bg-sidebar-accent md:[&>svg]:size-4";

/// Every page: the sidebar, then the page's heading, an optional line under
/// it, and its content. `path` marks the sidebar's current page; `title`
/// names the tab. Under 48rem the sidebar is a bar across the top, and its
/// pages a tab bar along the bottom.
fn layout(
    headers: &HeaderMap,
    user: &User,
    path: &str,
    title: &str,
    heading: Markup,
    lede: Option<Markup>,
    content: Markup,
) -> Markup {
    let theme = Theme::from_cookies(headers);
    document(
        headers,
        title,
        html! {
            body ."min-h-dvh md:flex" {
                aside class="sticky top-0 z-10 flex h-14 items-center gap-2 border-b bg-sidebar px-4 \
                    text-sidebar-foreground md:h-dvh md:w-60 md:shrink-0 md:flex-col md:items-stretch \
                    md:gap-4 md:border-e md:border-b-0 md:p-3" {
                    (brand())
                    nav class="fixed inset-x-0 bottom-0 z-10 grid auto-cols-fr grid-flow-col border-t \
                        bg-sidebar md:static md:flex md:flex-col md:gap-1 md:border-t-0" aria-label="Pages" {
                        @for (href, name, symbol) in pages() {
                            a class=(NAV_LINK) href=(href) aria-current=[(href == path).then_some("page")] { (symbol) (name) }
                        }
                    }
                    // The slider in the sidebar and the button in a phone's top bar,
                    // then who is signed in.
                    div ."ms-auto flex items-center gap-2 md:ms-0 md:mt-auto md:flex-col md:items-stretch" {
                        div ."hidden md:block" { (theme_slider(Place::Sidebar, theme)) }
                        div ."md:hidden" { (Component::new("theme", "picker").mount(theme_picker(theme))) }
                        (auth::account_menu(user))
                    }
                }
                main ."min-w-0 flex-1 pb-20 md:pb-0" {
                    div ."mx-auto flex max-w-5xl flex-col gap-8 px-4 py-8 md:px-8 md:py-10" {
                        header ."flex flex-col gap-1" {
                            div ."flex items-center gap-2 text-2xl font-semibold tracking-tight" { (heading) }
                            @if let Some(lede) = lede {
                                p ."text-muted-foreground" { (lede) }
                            }
                        }
                        (content)
                    }
                }
            }
        },
    )
}

/// A page without the sidebar, such as signing in: the app's name over one
/// narrow column in the middle of the window.
fn solo(headers: &HeaderMap, title: &str, content: Markup) -> Markup {
    document(
        headers,
        title,
        html! {
            body ."grid min-h-dvh place-items-center p-4" {
                main ."flex w-full max-w-sm flex-col items-center gap-6" {
                    (brand())
                    (content)
                }
            }
        },
    )
}

/// The app's mark and name, linking home.
fn brand() -> Markup {
    html! {
        a ."flex items-center gap-2 font-semibold md:px-2 md:py-1.5" href="/" {
            span ."flex size-7 items-center justify-center rounded-md bg-primary text-primary-foreground" aria-hidden="true" {
                (icon!("pill"))
            }
            (APP)
        }
    }
}

/// The value of the request's cookie called `name`.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|cookie| cookie.trim().strip_prefix(name)?.strip_prefix('='))
}

/// The colour scheme, kept per browser in a cookie. Dark renders Basecoat's
/// `dark` class on `<html>`.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, FormEnum)]
#[serde(rename_all = "lowercase")]
enum Theme {
    Light,
    Dark,
}

impl Theme {
    /// Each theme with its cookie value and its name.
    const ALL: [(Theme, &str, &str); 2] = [
        (Theme::Light, "light", "Light"),
        (Theme::Dark, "dark", "Dark"),
    ];

    fn name(self) -> &'static str {
        Self::ALL.iter().find(|(theme, ..)| *theme == self).unwrap().1
    }

    fn label(self) -> &'static str {
        Self::ALL.iter().find(|(theme, ..)| *theme == self).unwrap().2
    }

    /// The theme a press switches to.
    fn next(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    fn from_cookies(headers: &HeaderMap) -> Self {
        let chosen = cookie(headers, "theme");
        Self::ALL
            .into_iter()
            .find(|(_, name, _)| Some(*name) == chosen)
            .map_or(Theme::Light, |(theme, ..)| theme)
    }

    fn icon(self) -> PreEscaped<&'static str> {
        match self {
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
        button .btn data-variant="ghost" data-size="icon" type="submit" title=(label) {
            (symbol)
            span .sr-only { (label) }
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

async fn home(State(app): State<App>, user: User, headers: HeaderMap) -> Result<Markup, Failed> {
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
            "The next numbered file in migrations/, such as 0002_projects.sql. It runs when the app starts.",
        ),
        (
            icon!("palette"),
            "Style it",
            "Basecoat's components, laid out with Tailwind's utilities; placebo kit lists them.",
        ),
    ];
    Ok(layout(
        &headers,
        &user,
        "/",
        "Home",
        html! { h1 { "Welcome to " (APP) } },
        Some(html! { "Your app is running. Ask your agent to build the first feature." }),
        html! {
            div ."grid gap-4 md:grid-cols-3" {
                @for (symbol, title, text) in next {
                    section .card {
                        header {
                            h2 ."flex items-center gap-2" { (symbol) (title) }
                            p { (text) }
                        }
                    }
                }
            }
            section .card {
                header { h2 { "This app" } }
                section {
                    dl ."grid grid-cols-3 gap-y-2" {
                        dt ."text-muted-foreground" { "Database" } dd ."col-span-2" { (database) }
                        dt ."text-muted-foreground" { "Postgres" } dd ."col-span-2" { (version) }
                        dt ."text-muted-foreground" { "Address" } dd ."col-span-2" { (headers.get(header::HOST).and_then(|host| host.to_str().ok()).unwrap_or("")) }
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

/// The themes as a row of icons, the chosen one raised: the radios hidden,
/// their labels the buttons.
const THEME_CHOICE: &str = "flex w-fit gap-0.5 rounded-lg bg-muted p-0.5 *:flex *:size-7 \
    *:cursor-pointer *:items-center *:justify-center *:rounded-md *:text-muted-foreground \
    *:hover:text-foreground *:has-checked:bg-background *:has-checked:text-foreground \
    *:has-checked:shadow-sm *:has-focus-visible:ring-3 *:has-focus-visible:ring-ring/50 \
    dark:*:has-checked:bg-input/50 [&_input]:sr-only";

// The themes as a row of icons; the current one is raised.
fn theme_choice(place: Place, current: Theme) -> Markup {
    let fields = fields! { ChooseTheme {
        @field place = Control::hidden(place);
        div role="group" aria-label="Theme" {
            @field theme = Control::radios(current, Theme::ALL.map(|(theme, _, label)| {
                (theme, html! { (theme.icon()) span .sr-only { (label) } })
            })).class(THEME_CHOICE);
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

async fn settings(user: User, headers: HeaderMap) -> Markup {
    layout(
        &headers,
        &user,
        "/settings",
        "Settings",
        html! { h1 { "Settings" } },
        None,
        html! {
            section .card {
                header { h2 { "Appearance" } }
                section ."flex items-center justify-between gap-4" {
                    div {
                        h3 ."font-medium" { "Theme" }
                        p ."text-muted-foreground" { "Light or dark." }
                    }
                    (theme_slider(Place::Settings, Theme::from_cookies(&headers)))
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
        .merge(auth::routes())
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
        .with_state(state.clone());
    // Saves also work before the runtime loads, or without JavaScript. The
    // session goes around them, so a save's page render reuses its lookup.
    let app = placebo::native_forms(app)
        .layer(axum::middleware::from_fn_with_state(state, auth::session));
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

//! Reads that start themselves: a section filled in after the page loads, a
//! clock polled every second, and a list that loads more entries as it
//! scrolls into view. Each form also works as a plain link-like form without
//! JavaScript.
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Control, FormInput, Input, List, Position, ReadAction, Region, UPDATE_TYPE, fields};
use serde::Deserialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
#[allow(dead_code)]
mod support;

const STATS: Region = Region::new("stats");
const CLOCK: Region = Region::new("clock");
const MORE: Region = Region::new("more");
const ENTRIES: List = List::new("entries");
const LOAD_STATS: ReadAction<NoInput> = ReadAction::new("load-stats", "/stats");
const TICK: ReadAction<NoInput> = ReadAction::new("tick", "/clock");
const LOAD_MORE: ReadAction<More> = ReadAction::new("load-more", "/entries");
const TOTAL: u32 = 45;
const PAGE: u32 = 10;

// These reads take no input.
#[derive(Deserialize, FormInput)]
struct NoInput {}

#[derive(Default, Deserialize, FormInput)]
struct More {
    #[serde(default)]
    shown: u32,
}

#[derive(Clone, Default)]
struct Counters {
    ticks: Arc<AtomicU64>,
}

fn wants_update(headers: &HeaderMap) -> bool {
    headers.get(header::ACCEPT).and_then(|h| h.to_str().ok()) == Some(UPDATE_TYPE)
}

// A form inside its own region: the reply replaces it with the statistics.
fn stats_binding() -> placebo::ReadBinding<NoInput> {
    LOAD_STATS.bind(STATS).on_load()
}

fn stats_placeholder() -> Markup {
    let fields = fields! { NoInput {
        p { "Counting…" }
        button type="submit" { "Show statistics" }
    } };
    stats_binding().form(fields)
}

fn stats() -> Markup {
    html! { p #stats-total { (TOTAL) " entries, " (TOTAL / PAGE + 1) " pages" } }
}

// The clock's form stays outside its region, so polling keeps the same form.
fn clock_binding() -> placebo::ReadBinding<NoInput> {
    TICK.bind(CLOCK).every(1000)
}

fn clock(ticks: u64) -> Markup {
    html! { p #ticks { "Server ticks: " (ticks) } }
}

// The "load more" form is inside its own region at the end of the list. Its
// reply inserts the next entries and replaces the form with the next one.
fn more_binding() -> placebo::ReadBinding<More> {
    LOAD_MORE.bind(MORE).on_reveal().affects(ENTRIES)
}

fn more_form(shown: u32) -> Markup {
    if shown >= TOTAL {
        return html! { p #end { "That's everything." } };
    }
    let fields = fields! { More {
        @field shown = Control::hidden(shown);
        button type="submit" { "Load more" }
    } };
    more_binding().form(fields)
}

fn entry(number: u32) -> placebo::MountedItem {
    ENTRIES
        .item(number)
        .mount(html! { article .entry { "Entry " (number) } })
}

fn page(shown: u32, ticks: u64) -> Markup {
    let shown = shown.clamp(PAGE, TOTAL);
    let tick_fields = fields! { NoInput {} };
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Placebo / Read triggers" }
                style { ".entry { padding: 1.5rem 0; border-bottom: 1px solid #ddd; }" }
                script type="module" src="/placebo.js" {}
            }
            body {
                main {
                    h1 { "Read triggers" }
                    (clock_binding().form(tick_fields))
                    (CLOCK.mount(clock(ticks)))
                    (STATS.mount(stats_placeholder()))
                    (ENTRIES.mount(html! { @for number in 1..=shown { (entry(number)) } }))
                    (MORE.mount(more_form(shown)))
                }
            }
        }
    }
}

async fn home(State(counters): State<Counters>, Input(more): Input<More>) -> Markup {
    page(more.shown, counters.ticks.load(Ordering::Relaxed))
}

async fn load_stats(
    State(counters): State<Counters>,
    headers: HeaderMap,
    Input(_): Input<NoInput>,
) -> Response {
    // A slow part of the page, left out of the first render.
    tokio::time::sleep(Duration::from_millis(300)).await;
    if !wants_update(&headers) {
        return page(PAGE, counters.ticks.load(Ordering::Relaxed)).into_response();
    }
    stats_binding().reply(stats()).into_response()
}

async fn tick(
    State(counters): State<Counters>,
    headers: HeaderMap,
    Input(_): Input<NoInput>,
) -> Response {
    let ticks = counters.ticks.fetch_add(1, Ordering::Relaxed) + 1;
    if !wants_update(&headers) {
        return page(PAGE, ticks).into_response();
    }
    clock_binding().reply(clock(ticks)).into_response()
}

async fn load_more(
    State(counters): State<Counters>,
    headers: HeaderMap,
    Input(more): Input<More>,
) -> Response {
    let next = (more.shown + PAGE).min(TOTAL);
    // Without JavaScript the form navigates here: show the longer page.
    if !wants_update(&headers) {
        return page(next, counters.ticks.load(Ordering::Relaxed)).into_response();
    }
    let mut reply = more_binding().reply(more_form(next));
    for number in more.shown + 1..=next {
        reply = reply.also_insert(entry(number), Position::End);
    }
    reply.into_response()
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(home))
        .route(LOAD_STATS.path(), LOAD_STATS.route(load_stats))
        .route(TICK.path(), TICK.route(tick))
        .route(LOAD_MORE.path(), LOAD_MORE.route(load_more))
        .route("/placebo.js", get(placebo::runtime))
        .with_state(Counters::default());
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4323".into());
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

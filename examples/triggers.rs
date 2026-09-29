//! Reads that start themselves: a list that loads more entries as its end
//! scrolls into view, and a page that polls the server every second. Both
//! read this page again: "load more" at a longer query, polling at the same
//! one. Without JavaScript the "load more" form loads the longer page.
use axum::{Router, extract::State, routing::get};
use maud::{DOCTYPE, Markup, html};
use placebo::{Control, FormInput, Input, Read, fields};
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[allow(dead_code)]
mod support;

const TOTAL: u32 = 45;
const PAGE: u32 = 10;

/// The page's query: how many entries it shows.
#[derive(Default, Deserialize, FormInput)]
struct Shown {
    #[serde(default)]
    shown: u32,
}

#[derive(Clone, Default)]
struct Counters {
    renders: Arc<AtomicU64>,
}

// At the end of the list: asks for the page with the next entries too.
fn more_form(shown: u32) -> Markup {
    if shown >= TOTAL {
        return html! { p #end { "That's everything." } };
    }
    let fields = fields! { Shown {
        @field shown = Control::hidden((shown + PAGE).min(TOTAL));
        button type="submit" { "Load more" }
    } };
    Read::new().on_reveal().form(fields)
}

fn page(shown: u32, renders: u64, poll: bool) -> Markup {
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
                    // Polling runs while this element is on the page.
                    @if poll { (placebo::refresh_every(1000)) }
                    p #ticks { "Server renders: " (renders) }
                    section #entries {
                        @for number in 1..=shown {
                            article .entry id=(format!("entry-{number}")) { "Entry " (number) }
                        }
                    }
                    (more_form(shown))
                }
            }
        }
    }
}

// `/quiet` is the same page without polling, for "load more" alone.
async fn home(
    State(counters): State<Counters>,
    uri: axum::http::Uri,
    Input(Shown { shown }): Input<Shown>,
) -> Markup {
    let renders = counters.renders.fetch_add(1, Ordering::Relaxed) + 1;
    let poll = !uri.path().starts_with("/quiet");
    page(shown.clamp(PAGE, TOTAL), renders, poll)
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(home))
        .route("/quiet", get(home))
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

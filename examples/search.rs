use axum::{
    Router,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Control, FormInput, Input, ReadAction, Region, UPDATE_TYPE, fields};
use serde::Deserialize;
use std::time::Duration;
mod support;

const BOOKS: Region = Region::new("book-results");
const PLACES: Region = Region::new("place-results");
const SEARCH_BOOKS: ReadAction<Search> = ReadAction::new("search-books", "/search/books");
const SEARCH_PLACES: ReadAction<Search> = ReadAction::new("search-places", "/search/places");
const BOOK_TITLES: &[&str] = &[
    "Rust in Action",
    "The Rust Programming Language",
    "A Philosophy of Software Design",
    "Designing Data-Intensive Applications",
];
const PLACE_NAMES: &[&str] = &[
    "Oslo, Norway",
    "Bergen, Norway",
    "Copenhagen, Denmark",
    "Kyoto, Japan",
];

#[derive(Default, Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Search {
    #[serde(default)]
    q: String,
    #[serde(default)]
    delay_ms: u64,
}

fn results(items: &[&str], q: &str) -> Markup {
    let needle = q.to_lowercase();
    let matches: Vec<_> = items
        .iter()
        .filter(|item| item.to_lowercase().contains(&needle))
        .collect();
    html! {
        p .result-count { (matches.len()) " results" }
        @if matches.is_empty() {
            p .empty { "No matches for “" (q) "”. Try another search." }
        } @else {
            ul {
                @for item in matches {
                    li { span { (item) } span .arrow aria-hidden="true" { "↗" } }
                }
            }
        }
    }
}

fn panel(
    title: &str,
    id: &str,
    action: ReadAction<Search>,
    region: Region,
    items: &[&str],
    q: &str,
) -> Markup {
    let fields = fields! { Search {
        label for=(id) { "Search " (title.to_lowercase()) }
        .search {
            @field q = Control::search(q).id(id).placeholder("Start typing…").autocomplete("off");
            button type="submit" { "Search" }
        }
        .network {
            label for=(format!("{id}-delay")) { "Response delay" }
            @field delay_ms = Control::select(0, [(0, "None"), (800, "800 ms")]).id(&format!("{id}-delay"));
        }
    } };
    html! {
        section .panel {
            h2 { (title) }
            (action.bind(region.clone()).on_input(120).form(fields))
            (region.mount(results(items, q)))
        }
    }
}

fn page(books_q: &str, places_q: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Placebo / Interaction experiment" }
                link rel="stylesheet" href="/demo.css";
                script type="module" src="/placebo.js" {}
                script type="module" src="/demo.js" {}
            }
            body {
                main {
                    header {
                        a .wordmark href="/" { "placebo" span { " / lab 001" } }
                        span .badge { "Rust + browser" }
                    }
                    .intro {
                        p .eyebrow { "HTML, with an agreement." }
                        h1 { "Small updates." br; "Clear boundaries." }
                        p .lede { "Two independent searches, rendered on the server. Keep typing while a response is pending. The latest search gets the final word." }
                    }
                    .panels {
                        (panel("Books", "books-query", SEARCH_BOOKS, BOOKS, BOOK_TITLES, books_q))
                        (panel("Places", "places-query", SEARCH_PLACES, PLACES, PLACE_NAMES, places_q))
                    }
                    aside .notes {
                        label for="draft" { "A little local state" }
                        textarea #draft placeholder="Leave a note here. Search updates won't touch it." {}
                    }
                    details .trace {
                        summary { "Interaction trace" }
                        p { "Scheduled → request → applied. Superseded work is discarded. Protocol errors are reported here and in the console." }
                        ol #trace role="log" aria-live="polite" {}
                    }
                    footer { "Experiment 001 · HTML fragments + explicit update contracts" }
                }
            }
        }
    }
}

async fn home() -> Markup {
    page("", "")
}

async fn search_books(headers: HeaderMap, Input(query): Input<Search>) -> Response {
    search(query, headers, SEARCH_BOOKS, BOOK_TITLES, true).await
}

async fn search_places(headers: HeaderMap, Input(query): Input<Search>) -> Response {
    search(query, headers, SEARCH_PLACES, PLACE_NAMES, false).await
}

async fn search(
    query: Search,
    headers: HeaderMap,
    action: ReadAction<Search>,
    items: &[&str],
    books: bool,
) -> Response {
    tokio::time::sleep(Duration::from_millis(query.delay_ms.min(1500))).await;
    if headers.get(header::ACCEPT).and_then(|h| h.to_str().ok()) == Some(UPDATE_TYPE) {
        action
            .bind(if books { BOOKS } else { PLACES })
            .reply(results(items, &query.q))
            .into_response()
    } else if books {
        page(&query.q, "").into_response()
    } else {
        page("", &query.q).into_response()
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(home))
        .route(SEARCH_BOOKS.path(), SEARCH_BOOKS.route(search_books))
        .route(SEARCH_PLACES.path(), SEARCH_PLACES.route(search_places))
        .route("/placebo.js", get(placebo::runtime))
        .route(
            "/demo.css",
            get(async || support::asset("demo.css", "text/css", include_str!("static/demo.css"))),
        )
        .route(
            "/demo.js",
            get(async || {
                support::asset("demo.js", "text/javascript", include_str!("static/demo.js"))
            }),
        );
    #[cfg(all(feature = "dev", debug_assertions))]
    let reload = support::reload();
    #[cfg(all(feature = "dev", debug_assertions))]
    let app = app.layer(reload.layer());
    let address = std::env::var("PLACEBO_ADDR").unwrap_or_else(|_| "127.0.0.1:4317".into());
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .expect("bind demo address");
    println!(
        "Placebo experiment: http://{}",
        listener.local_addr().unwrap()
    );
    axum::serve(listener, app).await.unwrap();
}

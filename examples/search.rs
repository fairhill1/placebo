//! A live search that reads its own page: typing reads `/?q=…`, the address
//! follows, and the page morphs in with the results. Adding a book answers
//! with the page at the current query, so a match shows up at once.
use axum::{Router, extract::State, response::IntoResponse, routing::get};
use maud::{DOCTYPE, Markup, html};
use placebo::{
    Component, Control, FormInput, Input, MutationAction, MutationBinding, Read, fields,
};
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
mod support;

const ADD_BOOK: MutationAction<AddBook> = MutationAction::new("add-book", "/actions/add-book");
const BOOK_TITLES: &[&str] = &[
    "Rust in Action",
    "The Rust Programming Language",
    "A Philosophy of Software Design",
    "Designing Data-Intensive Applications",
];

type Books = Arc<Mutex<Vec<String>>>;

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct AddBook {
    title: String,
}

/// The page's query: the page renders from it, and the search form sends it.
#[derive(Default, Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Search {
    #[serde(default)]
    q: String,
    // A slower answer, to watch the newest search win.
    #[serde(default)]
    delay_ms: u64,
}

fn results(books: &[String], q: &str) -> Markup {
    let needle = q.to_lowercase();
    let matches: Vec<_> = books
        .iter()
        .filter(|book| book.to_lowercase().contains(&needle))
        .collect();
    html! {
        div #book-results .results {
            p .result-count { (matches.len()) " results" }
            @if matches.is_empty() {
                p .empty { "No matches for “" (q) "”. Try another search." }
            } @else {
                ul {
                    @for book in matches {
                        li { span { (book) } span .arrow aria-hidden="true" { "↗" } }
                    }
                }
            }
        }
    }
}

fn search_form(search: &Search) -> Markup {
    // The select shows the query's delay; any other value reads as none.
    let delay = if search.delay_ms >= 800 { 800 } else { 0 };
    let fields = fields! { Search {
        label for="books-query" { "Search books" }
        .search {
            @field q = Control::search(search.q.as_str()).id("books-query").placeholder("Start typing…").autocomplete("off");
            button type="submit" { "Search" }
        }
        .network {
            label for="books-delay" { "Response delay" }
            @field delay_ms = Control::select(delay, [(0, "None"), (800, "800 ms")]).id("books-delay");
        }
    } };
    Read::new().on_input(120).form(fields)
}

fn add_binding() -> MutationBinding<AddBook> {
    ADD_BOOK.bind(&Component::new("add-book", 1))
}

fn add_form(feedback: &str) -> Markup {
    let fields = fields! { AddBook {
        label for="new-book" { "Add a book" }
        .search {
            @field title = Control::text("").id("new-book").required().autocomplete("off");
            button type="submit" { "Add" }
        }
        p #add-feedback role="status" { (feedback) }
    } };
    add_binding().form(fields)
}

fn page(books: &[String], search: &Search) -> Markup {
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
                        h1 { "One page." br; "Always current." }
                        p .lede { "A search reads this page again at its query, rendered on the server. Keep typing while a response is pending. The latest search gets the final word." }
                    }
                    .panels {
                        section .panel {
                            h2 { "Books" }
                            (search_form(search))
                            (results(books, &search.q))
                        }
                        section .panel {
                            h2 { "Add a book" }
                            p { "A new book shows up in the results if it matches the current search." }
                            (Component::new("add-book", 1).mount(add_form("")))
                        }
                    }
                    aside .notes {
                        label for="draft" { "A little local state" }
                        textarea #draft placeholder="Leave a note here. Search updates won't touch it." {}
                    }
                    details .trace {
                        summary { "Interaction trace" }
                        p { "Scheduled → request → applied. Superseded work is discarded. Protocol errors are reported here and in the console." }
                        // The browser writes the trace, so the page keeps it through every morph.
                        ol #trace role="log" aria-live="polite" data-placebo-local="trace" {}
                    }
                    footer { "Experiment 001 · whole pages, morphed" }
                }
            }
        }
    }
}

// Every render of the page, a search's included, comes from its query.
async fn home(State(books): State<Books>, Input(search): Input<Search>) -> Markup {
    tokio::time::sleep(Duration::from_millis(search.delay_ms.min(1500))).await;
    page(&books.lock().unwrap(), &search)
}

// The reply's page renders the results for the query in the address.
async fn add_book(State(books): State<Books>, Input(input): Input<AddBook>) -> impl IntoResponse {
    let title = input.title.trim().to_owned();
    if title.is_empty() {
        return add_binding()
            .invalid(add_form("Enter a title."))
            .into_response();
    }
    books.lock().unwrap().push(title);
    add_binding().reply(add_form("Added.")).into_response()
}

#[tokio::main]
async fn main() {
    let books: Books = Arc::new(Mutex::new(
        BOOK_TITLES.iter().map(|title| title.to_string()).collect(),
    ));
    let app = Router::new()
        .route("/", get(home))
        .route(ADD_BOOK.path(), ADD_BOOK.route(add_book))
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
        )
        .with_state(books);
    // Forms submitted before the runtime loads, or without JavaScript.
    let app = placebo::native_forms(app);
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

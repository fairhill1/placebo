use axum::{
    Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{
    Component, Control, FormInput, Input, MutationAction, MutationBinding, ReadAction, Region,
    UPDATE_TYPE, fields,
};
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
mod support;

const BOOKS: Region = Region::new("book-results");
const PLACES: Region = Region::new("place-results");
const SEARCH_BOOKS: ReadAction<Search> = ReadAction::new("search-books", "/search/books");
const SEARCH_PLACES: ReadAction<Search> = ReadAction::new("search-places", "/search/places");
const ADD_BOOK: MutationAction<AddBook> = MutationAction::new("add-book", "/actions/add-book");
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

type Books = Arc<Mutex<Vec<String>>>;

#[derive(Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct AddBook {
    title: String,
}

#[derive(Default, Deserialize, FormInput)]
#[serde(deny_unknown_fields)]
struct Search {
    #[serde(default)]
    q: String,
    #[serde(default)]
    delay_ms: u64,
}

fn results(items: &[impl AsRef<str>], q: &str) -> Markup {
    let needle = q.to_lowercase();
    let matches: Vec<_> = items
        .iter()
        .map(AsRef::as_ref)
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
    binding: placebo::ReadBinding<Search>,
    region: Region,
    items: &[impl AsRef<str>],
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
            (binding.form(fields))
            (region.mount(results(items, q)))
        }
    }
}

// The Books search follows history, so the page renders its query too.
fn books_binding() -> placebo::ReadBinding<Search> {
    SEARCH_BOOKS.bind(BOOKS).on_input(120).history()
}

fn add_binding() -> MutationBinding<AddBook> {
    ADD_BOOK.bind(&Component::new("add-book", 1)).affects(BOOKS)
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

fn page(books: &[String], books_q: &str, places_q: &str) -> Markup {
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
                        (panel("Books", "books-query", books_binding(), BOOKS, books, books_q))
                        (panel("Places", "places-query", SEARCH_PLACES.bind(PLACES).on_input(120), PLACES, PLACE_NAMES, places_q))
                    }
                    section .panel {
                        p { "A new book shows up in the Books results if it matches the current search." }
                        (Component::new("add-book", 1).mount(add_form("")))
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

async fn home(State(books): State<Books>, Input(query): Input<Search>) -> Markup {
    page(&books.lock().unwrap(), &query.q, "")
}

async fn search_books(
    State(books): State<Books>,
    headers: HeaderMap,
    Input(query): Input<Search>,
) -> Response {
    tokio::time::sleep(Duration::from_millis(query.delay_ms.min(1500))).await;
    let books = books.lock().unwrap().clone();
    if wants_update(&headers) {
        books_binding()
            .reply(results(&books, &query.q))
            .into_response()
    } else {
        page(&books, &query.q, "").into_response()
    }
}

async fn search_places(
    State(books): State<Books>,
    headers: HeaderMap,
    Input(query): Input<Search>,
) -> Response {
    tokio::time::sleep(Duration::from_millis(query.delay_ms.min(1500))).await;
    if wants_update(&headers) {
        SEARCH_PLACES
            .bind(PLACES)
            .reply(results(PLACE_NAMES, &query.q))
            .into_response()
    } else {
        page(&books.lock().unwrap(), "", &query.q).into_response()
    }
}

fn wants_update(headers: &HeaderMap) -> bool {
    headers.get(header::ACCEPT).and_then(|h| h.to_str().ok()) == Some(UPDATE_TYPE)
}

// The reply does not know what the Books search currently shows; asking the
// browser to run it again keeps the filter the person typed.
async fn add_book(State(books): State<Books>, Input(input): Input<AddBook>) -> Response {
    let title = input.title.trim().to_owned();
    if title.is_empty() {
        return add_binding()
            .invalid(add_form("Enter a title."))
            .into_response();
    }
    books.lock().unwrap().push(title);
    add_binding()
        .reply(add_form("Added."))
        .also_refetch(&BOOKS)
        .into_response()
}

#[tokio::main]
async fn main() {
    let books: Books = Arc::new(Mutex::new(
        BOOK_TITLES.iter().map(|title| title.to_string()).collect(),
    ));
    let app = Router::new()
        .route("/", get(home))
        .route(SEARCH_BOOKS.path(), SEARCH_BOOKS.route(search_books))
        .route(SEARCH_PLACES.path(), SEARCH_PLACES.route(search_places))
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

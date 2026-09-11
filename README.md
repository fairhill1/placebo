# Placebo

An experimental framework for Rust + Axum server-rendered apps. Rust renders
HTML with Maud; Placebo updates the page while preserving browser-owned drafts.
Maud is the current renderer, and the framework's name and API are provisional.

**Build forms with `fields!`, bind them to actions, and register handlers with
`action.route(handler)`.** These APIs connect your Rust payload, HTML controls,
and handler types. They generate the request configuration and response format
for you. Normal application code should not construct `data-placebo` JSON or
write protocol headers by hand.

## Create an app

From this checkout, generate the recommended typed starter:

```sh
cargo run --features dev --bin placebo -- new ../my-app
cd ../my-app
cargo dev
```

The starter includes two editors, validation and version checks, development
rebuild/reload, and `AGENTS.md` instructions for LLM coding. The dev loop checks
for detectable API bypasses before each build. Use these commands in CI too:

```sh
cargo check-placebo
cargo test
```

`placebo check` flags handwritten request configuration and recognized Placebo
actions registered outside their typed adapters. Intentional custom integrations
need a scoped exception with a reason, which stays visible in the check output.
Ordinary Axum endpoints are unaffected. This is a source check with documented
limits, not full Rust name resolution or proof of browser correctness. See
[project checks and exceptions](docs/project-checks.md).

## Build your first app

This complete app has two independent editors sharing one save action. It
includes server validation, draft preservation, normalized titles, and version
checks for conflicting saves from another tab. No custom JavaScript is needed.
It is also a complete manual setup if you prefer to build without the generator;
run `placebo check` separately in that app after installing the development CLI.

Placebo is not published to crates.io. From this checkout, create a sibling app:

```sh
cargo new --bin ../my-app
cd ../my-app
```

Replace `Cargo.toml` with the following. The path assumes the Placebo checkout
is named `placebo`; adjust it if yours is elsewhere.

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"

[dependencies]
placebo = { path = "../placebo" }
axum = "0.8.9"
maud = { version = "0.27.0", features = ["axum"] }
serde = { version = "1.0.229", features = ["derive"] }
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "net"] }
```

Replace `src/main.rs` with this entire file:

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use maud::{html, Markup, DOCTYPE};
use placebo::{fields, Component, Control, FormInput, MutationAction};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

struct Item {
    id: u64,
    title: String,
    version: u64,
}
type Store = Arc<Mutex<Vec<Item>>>;

#[derive(Deserialize, FormInput)]
struct SaveTitle {
    id: u64,
    title: String,
    version: u64,
}

const SAVE: MutationAction<SaveTitle> = MutationAction::new("save-title", "/save");

// Render the component's CONTENTS, including its form and feedback.
// Both the initial page and save responses reuse this function.
fn editor(item: &Item, feedback: &str) -> Markup {
    let component = Component::new("editor", item.id);
    let title_id = format!("title-{}", item.id);
    let feedback_id = format!("feedback-{}", item.id);
    let fields = fields! { SaveTitle {
        // IDs and versions belong to the server and must refresh on each reply.
        @field id = Control::hidden(item.id);
        @field version = Control::hidden(item.version);
        div data-placebo-local="draft" {
            label for=(title_id) { "Title" }
            @field title = Control::text(&item.title)
                .id(&title_id).described_by(&feedback_id);
        }
        p id=(feedback_id) role="status" { (feedback) }
        button type="submit" { "Save" }
    } };
    html! {
        article {
            h2 { (item.title) }
            (SAVE.bind(&component).form(fields))
        }
    }
}

async fn home(State(store): State<Store>) -> Markup {
    let items = store.lock().unwrap();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "My Placebo app" }
                script type="module" src="/placebo.js" {}
            }
            body {
                h1 { "Two editors" }
                @for item in items.iter() {
                    // Only the initial page adds the component's outer mount.
                    (Component::new("editor", item.id).mount(editor(item, "")))
                }
            }
        }
    }
}

// SAVE.route supplies the state and deserialized SaveTitle directly.
async fn save(store: Store, input: SaveTitle) -> Response {
    let mut items = store.lock().unwrap();
    let Some(item) = items.iter_mut().find(|item| item.id == input.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let component = Component::new("editor", item.id);
    let binding = SAVE.bind(&component);
    let title = input.title.trim();
    if !(3..=80).contains(&title.chars().count()) {
        return binding
            .invalid(editor(item, "Use 3–80 characters."))
            .into_response();
    }
    if input.version != item.version {
        return binding
            .conflict(editor(item, "Changed in another tab. Review the saved title and retry."))
            .into_response();
    }
    // The version check and write happen under the same lock.
    item.title = title.to_owned();
    item.version += 1;
    binding
        .reply(editor(item, "Saved."))
        .reset_local("draft")
        .into_response()
}

#[tokio::main]
async fn main() {
    let store: Store = Arc::new(Mutex::new(vec![
        Item { id: 1, title: "First item".into(), version: 1 },
        Item { id: 2, title: "Second item".into(), version: 1 },
    ]));
    let app = Router::new()
        .route("/", get(home))
        .route("/placebo.js", get(placebo::runtime))
        .route(SAVE.path(), SAVE.route(save))
        .with_state(store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Open http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
```

Run `cargo run` and open <http://127.0.0.1:3000>. Try a title shorter than three
characters, save while keeping an unsaved draft in the other editor, or open
two tabs and save the same item in both. After a conflict, the saved heading
and hidden version update while your draft remains available for review/retry.
Data lives in memory and resets on restart. Saving requires the browser runtime;
this example does not implement a separate no-JavaScript POST flow.

## The application API

| Responsibility | API used above |
|---|---|
| Connect a payload to its controls | `#[derive(FormInput)]` and `fields! { SaveTitle { ... } }` |
| Define one reusable endpoint | `MutationAction<SaveTitle>` |
| Address a runtime record ID | `Component::new("editor", item.id)` |
| Generate the form and request configuration | `SAVE.bind(&component).form(fields)` |
| Deserialize the payload and check the mutation request header | `SAVE.route(save)` |
| Render the initial component wrapper | `component.mount(editor(...))` |
| Refresh that component's contents | `binding.reply(editor(...))`, `.invalid(...)`, or `.conflict(...)` |

Dynamic record IDs work with the typed APIs. Adding a dialog does not require
replacing them either: keep the typed form and add a local browser behavior for
opening and closing the dialog. See [coordinated updates and dialogs](docs/interactions.md)
and the complete [task example](examples/tasks.rs).

**Mount on the page; reply with contents.** A refresh keeps the existing outer
component element. Passing `component.mount(...)` into `reply`, `invalid`, or
`conflict` creates an unsupported nested component. All three responses should
render the complete component contents, including the form and feedback;
returning only an error paragraph would remove the form and its draft.

**Keep drafts local; keep feedback and versions outside.** A matching
`data-placebo-local="draft"` subtree retains its existing DOM, values, and
listeners during a refresh. Incoming server markup inside it is ignored.
`.reset_local("draft")` accepts normalized server values after a successful save
only if the user has not edited that draft since submitting. Without that reset,
the saved heading can change while the input retains its old value. Validation
and conflict replies retain the draft. Local keys are scoped to each component;
nested or duplicate local keys are rejected.

`fields!` requires every payload field exactly once, with the right value type.
Missing/renamed fields, duplicate controls, the wrong form schema, and mismatched
typed handlers fail compilation. Ordinary Maud handles layout; `@field` declares
payload controls. Supported controls are text/search, hidden values, and typed
selects. See [typed forms](docs/typed-forms.md) for serde support and restrictions.

These guarantees depend on using the APIs together. Raw named inputs bypass the
form checks; manually registering `post(save)` bypasses the action's handler
adapter and mutation-header check. Deriving `FormInput` alone does not check
handwritten HTML. Rust also cannot prove that a component is mounted, that its
response markup has the right structure, or that a user may edit a record.
Applications still own runtime validation, authentication, and authorization.

For server search, use `ReadAction<Input>`, bind it to a `Region`, and register
its handler with `action.route(handler)`. `.on_input(120)` adds debounced search;
the runtime prevents older responses from overwriting newer results. Read
handlers receive `(state, input, headers)` so they can return a full page for a
normal GET or an update for an enhanced request. See the [search example](examples/search.rs).

For updates to summaries/counts alongside an editor, declare destinations with
`.affects(region)` and reply with `.also_replace(...)` or `.also_append(...)`.
[Coordinated updates](docs/interactions.md) explains revisions and ownership.

## Development rebuild and reload

From your app directory, install the optional development CLI:

```sh
cargo install --path ../placebo --features dev --locked
```

Add this section to your app's `Cargo.toml`:

```toml
[features]
dev = ["placebo/dev"]
```

Then run `placebo dev --bin my-app --features dev`. Rust edits run the project
check, then trigger a Cargo rebuild and application restart. A failed check or
build leaves the previous server running while the error is reported. Ctrl-C stops the supervisor,
build, and application; on Unix it also signals their process groups.

For browser reload after a restart or a static-file edit, create a `static`
directory and insert this after constructing `app`, before starting the listener:

```rust
#[cfg(all(feature = "dev", debug_assertions))]
let reload = placebo::dev::watch(["static"]).expect("watch static files");
#[cfg(all(feature = "dev", debug_assertions))]
let app = app.layer(reload.layer());
// Keep `reload` alive until axum::serve finishes.
```

If your app serves static assets, serve their current bytes from disk in dev;
watching a file does not update bytes compiled into `include_str!` or
`include_bytes!`. The examples demonstrate disk serving in [their support module](examples/support/mod.rs).
Reload refreshes the whole page and does not preserve unsaved drafts.

The CLI watches Rust files, Cargo manifests/lockfiles, and `.cargo/config.toml`
under the current directory, ignoring build outputs. External path dependencies,
workspace package selection, arbitrary build commands, and application
readiness/rollback checks are not implemented yet. Bacon and direct Cargo use
remain available.

The reload helper is excluded from release application builds, even with `dev`
enabled. Explicitly enabling that feature can still compile its optional
dependencies; production builds should omit it. The CLI itself can be built in
release mode.

## Run this repository's demos

These aliases run the examples inside the Placebo checkout; they are not commands
installed into your new app:

```sh
cargo dev    # Two editors, with rebuild/reload: http://127.0.0.1:4318
cargo tasks  # Tasks, counts, and dialogs: http://127.0.0.1:4319

# Search, with rebuild/reload: http://127.0.0.1:4317
cargo run --features dev --bin placebo -- dev --example search --features dev

# Run an example directly, without the development supervisor:
cargo run --example editors
```

`PLACEBO_ADDR` overrides example listening addresses. Example data is in memory.
The [editors](examples/editors.rs) and [tasks](examples/tasks.rs) are reference
applications you can read and adapt; the quickstart above needs neither file.

## Diagnose failures

Unexpected runtime failures log console errors by default, including a request
ID, action, endpoint/status when available, and whether the update was applied
or the server write's outcome is unknown. Debug action routes log matching IDs
on the server. Compile success alone does not verify DOM structure or browser
behavior: exercise validation, successful saves, and conflicting tabs in a browser.

Enable optional tracing in the browser console:

```js
(await import('/placebo.js')).trace(true);
```

To trace startup too, set `localStorage.setItem('placebo:trace', 'true')` and
reload. See [diagnostics](docs/diagnostics.md) for error context, events, and
behavior loading.

The [protocol reference](docs/protocol.md) documents the generated wire format
for runtime/contributor work. Application forms and replies should use the Rust
APIs above. The Rust and browser runtime versions must match; `placebo::runtime`
serves the runtime supplied by the crate.

## Goals

Make Rust + Axum SSR a coherent development experience: simple authoring,
clear server/browser ownership, useful compiler checks, explainable failures,
and an integrated development loop that works well for humans and LLMs.
Evaluate the complete experience against Maud/HTMX/Alpine, Datastar, and broader
SSR frameworks, including authoring effort, reliability, and resource cost.

**Silent failure is a design defect.** Catch what can be checked at compile
time; make runtime failures visible and actionable in the browser console.
Developers should be able to explain why an interaction did or did not apply.

[GOALS.md](GOALS.md) records the design criteria, open implementation choices,
and current gaps. Judge new features against those goals.

## Verification

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo build --features dev --bin placebo --examples
npm install
npx playwright install chromium
PLACEBO_TEST_DEV=1 npm test

# Also verify production isolation with the dev feature explicitly enabled:
cargo build --release --features dev --example editors
PLACEBO_TEST_DEV=1 PLACEBO_TEST_RELEASE=1 npm test
```

Rust tests include nine deliberately uncompilable form/handler examples, a
working typed example, and request/response checks for renamed fields and
malformed payloads. The browser tests use Chromium and real local Axum servers.
They cover adverse request
ordering, remounts, independent instances, draft/focus preservation, validation,
conflicts, static reload, Rust rebuild, compile-error recovery, and supervisor
cleanup. Task tests additionally cover coordinated updates, reversed response
delivery, guarded resets, append mounting, malformed batches, remounted extra
targets, dialog focus, and behavior teardown/restart. The dev-loop test creates
and removes a temporary application.
IME tests dispatch composition events; they do not drive an OS input method.
An existing Playwright installation can be selected with `PLAYWRIGHT_MODULE`.

## Still open

More control types and generated protocol definitions; richer state ownership;
idempotency and recovery after uncertain mutations; nested components;
navigation/history; streaming; general morphing; and an
authoring layer evaluated against the Maud baseline. Current verification is
on macOS/Chromium, not a browser/platform compatibility claim.

## References

- [Axum response integration](https://docs.rs/axum/latest/axum/response/index.html)
- [DOM child replacement](https://developer.mozilla.org/en-US/docs/Web/API/Element/replaceChildren)
- [Focus restoration](https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/focus)
- [tower-livereload](https://docs.rs/tower-livereload/0.10.3/tower_livereload/)

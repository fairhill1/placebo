# Placebo

**Type-checked HTML over the wire for Rust and Axum.**

An experimental framework for Rust + Axum server-rendered apps. Rust renders
HTML with Maud; Placebo updates the page while preserving browser-owned drafts.
Maud is the current renderer, and the framework's name and API are provisional.

**Build forms with `fields!`, bind them to actions, and register handlers with
`action.route(handler)`.** These APIs connect your Rust payload, HTML controls,
and handler types. They generate the request configuration and response format
for you. Normal application code should not construct `data-placebo` JSON or
write protocol headers by hand.

## Quickstart

Placebo is not published to crates.io yet. Next to this checkout, create an app:

```sh
cargo new ../my-app
cd ../my-app
```

Add the dependencies to `Cargo.toml` (adjust the path if your checkout is elsewhere):

```toml
[dependencies]
placebo = { path = "../placebo" }
axum = "0.8.9"
maud = { version = "0.27.0", features = ["axum"] }
serde = { version = "1.0.229", features = ["derive"] }
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "net"] }
```

Replace `src/main.rs` with this complete app. It has two editors sharing one
save action, with validation, draft preservation, and version conflict checks:

```rust
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use maud::{DOCTYPE, Markup, html};
use placebo::{Component, Control, FormInput, MutationAction, fields};
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
            .conflict(editor(
                item,
                "Changed in another tab. Review the saved title and retry.",
            ))
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
        Item {
            id: 1,
            title: "First item".into(),
            version: 1,
        },
        Item {
            id: 2,
            title: "Second item".into(),
            version: 1,
        },
    ]));
    let app = Router::new()
        .route("/", get(home))
        .route("/placebo.js", get(placebo::runtime))
        .route(SAVE.path(), SAVE.route(save))
        .with_state(store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("Open http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
```

Run `cargo run` and open <http://127.0.0.1:3000>. Try a title shorter than three
characters, keep an unsaved draft in one editor while saving the other, or save
the same item from two tabs. Data lives in memory. This file is
[`examples/quickstart.rs`](examples/quickstart.rs); a test keeps the two identical.

<!-- rules:start (generated from docs/rules.md; a test keeps them identical) -->
## Rules for building with Placebo

These apply to people and coding agents alike.

- **Forms:** derive `FormInput` on the payload struct and write the form with
  `fields!` and typed `Control` values. Render it with
  `ACTION.bind(&component).form(fields)` for mutations or
  `ACTION.bind(region).form(fields)` for reads. Don't write `data-placebo`
  attributes, named inputs for payload fields, or protocol headers by hand.
- **Routes:** register every action with its adapter:
  `.route(ACTION.path(), ACTION.route(handler))`. A plain Axum route such as
  `post(save)` skips payload decoding and the mutation request check; the
  browser reports it as `unadapted-route`. Unrelated pages, assets, and JSON
  endpoints are ordinary Axum routes.
- **Search:** use `ReadAction` with `.on_input(ms)` for live server search. Don't
  rebuild it with `fetch`, `DOMParser`, or manual DOM replacement.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount.
- **Drafts:** wrap user-editable controls in `data-placebo-local="draft"`. Keep
  record IDs, versions, and feedback outside it so every reply refreshes them.
  Add `.reset_local("draft")` to a successful reply to show normalized values.
- **Dialogs:** make the dialog the component root with `mount_dialog`, or keep
  it outside the refreshed component. Listen for `placebo:applied` on `document`.
- **Shared counts and summaries:** use `VersionedRegion`, mount it with
  `region.mount(revision, contents)`, declare it with `.affects(region)`, and
  reply with `.also_replace(region, revision, contents)`. Increment the revision
  with the data under the same lock or transaction. Use a plain `Region` for
  read results and `.also_append(...)` collections.
- **Verify in a browser:** compiling proves the Rust side agrees. Before calling
  a change done, run the app and exercise the changed flows: valid saves,
  invalid input, independent drafts, conflicts, and any dialog or search. Check
  the browser console: Placebo logs every failure as `[placebo:<code>]` with a
  next step. Fix the cause rather than working around it.
<!-- rules:end -->

## The application API

The quickstart uses the following form, action, and component APIs.

| Responsibility | API |
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
`conflict` now fails to compile: mounting returns `MountedComponent`, while replies
accept `Markup`. Wrapping a mount in arbitrary `html!` erases that distinction;
the browser still rejects nested components. All three responses should
render the complete component contents, including the form and feedback;
returning only an error paragraph would remove the form and its draft.

**Keep the dialog root persistent.** Use
`component.mount_dialog("heading-id", contents)` to make the native dialog the
component root, or put `component.mount(contents)` inside a dialog. The browser
rejects dialogs nested inside replaceable component contents with
`unstable-dialog`, before sending a mutation or applying a malformed response.
This also applies to dialogs inside local subtrees. Opening/closing is still
local application behavior; see [the dialog recipe](docs/interactions.md).

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
payload controls. Controls cover text-like inputs, textareas, numbers,
checkboxes, radios, selects, multiple selections, and hidden values, with
`Option` and `Vec` fields. See [typed forms](docs/typed-forms.md) for the
control table, how absent/empty values decode, and restrictions.

These guarantees depend on using the APIs together. Raw named inputs bypass the
form checks, and deriving `FormInput` alone does not check handwritten HTML.
Registering `post(save)` instead of `SAVE.route(save)` bypasses the adapter's
payload decoding and mutation-header check; the browser rejects its responses
with an `unadapted-route` error. Rust also cannot prove that a component is mounted, that its
response markup has the right structure, or that a user may edit a record.
Applications still own runtime validation, authentication, and authorization.

For server search, use `ReadAction<Input>`, bind it to a `Region`, and register
its handler with `action.route(handler)`. `.on_input(120)` adds debounced search;
the runtime prevents older responses from overwriting newer results. Read
handlers receive `(state, input, headers)` so they can return a full page for a
normal GET or an update for an enhanced request. See the [search example](examples/search.rs).

For shared summaries/counts, declare a `VersionedRegion`, mount it with
`counts.mount(revision, contents)`, declare `.affects(counts)`, and reply with
`.also_replace(counts, revision, contents)`. A plain `Region` cannot be passed to
`also_replace`; use plain regions for reads or `.also_append(...)` collections.
The server must increment the snapshot revision with each corresponding change.
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

Then run `placebo dev --bin my-app --features dev`. Rust edits trigger a Cargo
rebuild and application restart. A failed build leaves the previous server
running while the error is reported. Ctrl-C stops the supervisor,
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
applications you can read and adapt; the quickstart needs neither file.

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

Rust tests include deliberately uncompilable form/handler examples, working
typed examples, and request/response checks for renamed fields, browser
absence rules, and malformed payloads. The browser tests use Chromium and real local Axum servers.
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

File uploads, enum-valued fields, and generated protocol definitions; richer state ownership;
idempotency and recovery after uncertain mutations; nested components;
navigation/history; streaming; general morphing; and an
authoring layer evaluated against the Maud baseline. Current verification is
on macOS/Chromium, not a browser/platform compatibility claim.

## References

- [Axum response integration](https://docs.rs/axum/latest/axum/response/index.html)
- [DOM child replacement](https://developer.mozilla.org/en-US/docs/Web/API/Element/replaceChildren)
- [Focus restoration](https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/focus)
- [tower-livereload](https://docs.rs/tower-livereload/0.10.3/tower_livereload/)


See the [corrected Issue Desk comparison](docs/comparison.md) for current evidence
against Datastar: both repaired apps pass 14/14, with important scope limits.

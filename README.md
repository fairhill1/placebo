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
use placebo::{Component, Control, FormInput, Input, MutationAction, fields};
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
        // IDs and versions belong to the server and refresh on each reply.
        @field id = Control::hidden(item.id);
        @field version = Control::hidden(item.version);
        // A reply never overwrites what someone typed and has not saved yet.
        label for=(title_id) { "Title" }
        @field title = Control::text(&item.title)
            .id(&title_id).described_by(&feedback_id);
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

// SAVE.route requires `Input<SaveTitle>` last. Other Axum extractors, such as
// a session for authorization, go before it.
async fn save(State(store): State<Store>, Input(input): Input<SaveTitle>) -> Response {
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
    binding.reply(editor(item, "Saved.")).into_response()
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
    // Saves also work before the runtime loads, or without JavaScript.
    let app = placebo::native_forms(app);
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
- **Routes:** register every action with `.route(ACTION.path(), ACTION.route(handler))`
  and wrap the finished router with `placebo::native_forms(app)`. The handler
  takes any Axum extractors (state, session), then `Input<Payload>` last, and
  also answers forms sent without JavaScript; don't branch on that. A plain
  route such as `post(save)` skips decoding and the request checks; the browser
  reports it as `unadapted-route`. Other pages and assets are ordinary routes.
- **Reads:** use `ReadAction` with `.on_input(ms)` for live search, and
  `.on_load()`, `.on_reveal()`, or `.every(ms)` for reads that start themselves.
  Don't rebuild them with `fetch` or manual DOM replacement. Add `.history()` to
  keep the query in the URL, and render the page from the same query
  (`Input<Search>` in the page handler) so reloads and bookmarks work.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount. Contents may mount
  other components; each keeps its node and drafts when the outer one refreshes.
- **Drafts:** typed controls keep what the person typed by themselves. A reply
  replaces everything except controls with edits the server has not accepted;
  after a successful save, the submitted controls show the saved values.
  Render the submitted values in `invalid` and the saved record in `conflict`.
  Use `data-placebo-local` only for controls that must stay together as one
  unit or controls a behavior renders.
- **Validation:** mark a rejected control with `.invalid(true)` and link its
  message with `.described_by(id)`. Put feedback in a `role="status"` (or
  `role="alert"`) element; an invalid reply focuses the first invalid control.
  Use `.required()` for fields the browser can check before submitting.
- **Dialogs and local UI:** make a dialog the component root with `mount_dialog`,
  or keep it outside the refreshed component, and open and close it with
  `command`/`commandfor` buttons, which work without JavaScript. Use
  `popovertarget` and `details` for other local UI; with an id, their open state
  survives replies. Use `behavior()` for intent such as closing after a save,
  and listen for `placebo:applied` on `document`.
- **Shared counts and summaries:** use `VersionedRegion`, mount it with
  `region.mount(revision, contents)`, declare it with `.affects(region)`, and
  reply with `.also_replace(region, revision, contents)`. Return the binding
  from one function that the view's form and the handler's reply both use.
  Increment the revision with the data under the same lock or transaction.
- **Lists:** use a `List` when items are added, removed, or reordered. Mount
  each item with `LIST.item(key).mount(contents)`, declare `.affects(LIST)`,
  and reply with `also_insert`, `also_move`, `also_remove`, or `also_order`; a
  read reply may `also_insert` (a "load more" list). To show a new record in
  filtered search results, reply with `.also_refetch(&region)` instead.
- **Other components, pages, and tabs:** refresh another component with
  `.affects(&component)` and `.also_refresh(&component, contents)`. After
  creating or deleting a record, reply with `.navigate("/path")`. To update
  other open pages, publish the same updates on a `Feed` (`feed.push()...send()`
  under the write's lock) that the page mounts with `feed.mount()`. A component
  that other actions or a feed refresh needs `.revision(n)` on every mount and
  binding, from the record rendered.
- **Verify in a browser:** compiling proves the Rust side agrees. Run the app and
  exercise the changed flows: valid saves, invalid input, independent drafts,
  conflicts, and any dialog or search. Placebo logs every failure in the console
  as `[placebo:<code>]` with a next step; fix the cause. A component whose write
  may have committed gets `data-placebo-stale`; say so with CSS, since
  submitting again retries safely.
<!-- rules:end -->

## The application API

The quickstart uses the following form, action, and component APIs.

| Responsibility | API |
|---|---|
| Connect a payload to its controls | `#[derive(FormInput)]` and `fields! { SaveTitle { ... } }` |
| Define one reusable endpoint | `MutationAction<SaveTitle>` |
| Address a runtime record ID | `Component::new("editor", item.id)` |
| Generate the form and request configuration | `SAVE.bind(&component).form(fields)` |
| Deserialize the payload and check that the request is same-origin | `SAVE.route(save)`, with `Input<SaveTitle>` as the handler's last argument |
| Render the initial component wrapper | `component.mount(editor(...))` |
| Refresh that component's contents | `binding.reply(editor(...))`, `.invalid(...)`, or `.conflict(...)` |
| Answer forms submitted without JavaScript with pages | `placebo::native_forms(app)` |

Dynamic record IDs work with the typed APIs. Adding a dialog does not require
replacing them either: keep the typed form, mount the dialog with
`mount_dialog`, and open it with a `command="show-modal"` button. See [coordinated updates and dialogs](docs/interactions.md)
and the complete [task example](examples/tasks.rs).

**Mount on the page; reply with contents.** A refresh keeps the existing outer
component element. Passing `component.mount(...)` into `reply`, `invalid`, or
`conflict` now fails to compile: mounting returns `MountedComponent`, while replies
accept `Markup`. Contents may mount other components: a refresh keeps each
nested component's node and refreshes it by its own rules (see
[nested components](docs/interactions.md#nested-components)). All three responses should
render the complete component contents, including the form and feedback;
returning only an error paragraph would remove the form and its draft.

**Forms work without JavaScript.** A mutation form is a plain HTML form, so a
save submitted before the runtime loads, or with JavaScript off, posts natively
and runs the same handler. A successful reply becomes a redirect back to the
page (or to its `navigate` path), which shows the saved state. An `invalid` or
`conflict` reply becomes that whole page again with the reply's status:
`placebo::native_forms(app)` renders the page the form was on, and the rejected
component shows the reply's contents. Fields the person edited keep what they
typed and the first invalid control takes focus, following the same rule as the
runtime. Reads are GET forms and navigate natively. See the
[protocol](docs/protocol.md) for the redirect, same-origin, and page rules.

**Retrying a save is safe.** Every mutation form carries a fresh idempotency
key. If a response is lost, the component gets `data-placebo-stale`, and
submitting its form again resends the same request: the server replays the reply
it recorded instead of saving twice, or saves for the first time if the first
attempt never arrived. A form submitted twice without JavaScript saves once too,
and a handler runs to its end even if the browser disconnects. A retry older than
the store remembers is refused as unknown rather than saved again.
Replies are recorded in memory by default. With several server processes, or
writes that must survive a restart, implement `ReplayStore` on your database and
install it with `.layer(placebo::replays(store))`. See
[idempotent retries](docs/protocol.md#idempotent-retries).

**Other tabs update live.** A `Feed` pushes the same updates a reply can make
(versioned regions, versioned components, list items) to every page that mounts
it, over Server-Sent Events, ordered against replies by revision. Mount it with
`feed.mount()`, register `feed.route()`, and publish with `feed.push()...send()`
under the write's lock. A reconnecting page gets what it missed, or resyncs by
reading itself again. See [live updates](docs/interactions.md#live-updates-across-tabs);
the task example keeps two tabs in step.

**Keep the dialog root persistent.** Use
`component.mount_dialog("heading-id", contents)` to make the native dialog the
component root, or put `component.mount(contents)` inside a dialog. A
`mount_dialog` component may sit inside another component's contents; it keeps
its node, open state, and modality when the outer one refreshes. The browser
rejects other dialogs inside replaceable component contents with
`unstable-dialog`, before sending a mutation or applying a malformed response.
This also applies to dialogs inside local subtrees. Open and close dialogs with
`command`/`commandfor` buttons, which work without JavaScript; popovers and
`details` with an id keep their open state across replies. See
[local UI state](docs/interactions.md#local-ui-state).

**Edits survive replies; everything else follows the server.** Each typed
control is retained on its own. A control keeps its node, value, focus, and
selection while it differs from the value the server last rendered, that is,
while it holds an edit the server has not accepted. Every other control shows
the reply's markup. After a successful save, the submitted controls show the
saved (normalized) values, unless the person edited them again while the save
was in flight. So render the submitted values in `invalid` and the saved record
in `conflict`: in a conflict, the fields this person changed keep their edits
and the fields they did not touch show what the other person saved. Saving
again then cannot revert those. A kept control still takes the reply's
`aria-invalid`, `aria-describedby`, `disabled`, `readonly`, and `required`.
`data-placebo-local="key"` retains a whole subtree as one unit instead; keys
are scoped to each component, and nested or duplicate keys are rejected.

`fields!` requires every payload field exactly once, with the right value type.
Missing/renamed fields, duplicate controls, the wrong form schema, and mismatched
typed handlers fail compilation. Ordinary Maud handles layout; `@field` declares
payload controls. Controls cover text-like inputs, textareas, numbers,
checkboxes, radios, selects, multiple selections, hidden values, and files, with
`Option`, `Vec`, and `#[derive(FormEnum)]` enum fields. A file field is an
`Upload<MAX_BYTES>`: its form becomes multipart, and the limit is checked in the
browser before sending and on the server before the handler runs. See [typed forms](docs/typed-forms.md) for the
control table, how absent/empty values decode, and restrictions.

These guarantees depend on using the APIs together. Raw named inputs bypass the
form checks, and deriving `FormInput` alone does not check handwritten HTML.
Registering `post(save)` instead of `SAVE.route(save)` bypasses the adapter's
payload decoding and same-origin check; the browser rejects its responses
with an `unadapted-route` error. Rust also cannot prove that a component is mounted, that its
response markup has the right structure, or that a user may edit a record.
Applications still own runtime validation, authentication, and authorization.
Handlers take any Axum extractors before `Input<T>`, so a session extractor can
identify the user and the handler can check what they may change.

For server search, use `ReadAction<Input>`, bind it to a `Region`, and register
its handler with `action.route(handler)`. `.on_input(120)` adds debounced search;
the runtime prevents older responses from overwriting newer results. A read
handler can take `HeaderMap` before `Input<Search>` to return a full page for a
normal GET or an update for an enhanced request. See the [search example](examples/search.rs).

Add `.history()` to a read binding to keep its query in the page URL. Each new
query gets a history entry (keystrokes in one text field share one), Back and
Forward put the entry's values back into the form and read again, and the page
handler renders the same query on reload.

Reads can also start themselves: `.on_load()` fills in a slow section after the
page shows, `.on_reveal()` reads when a form scrolls into view, and `.every(ms)`
polls while the page is visible. A read reply can insert items into a list its
binding declares with `.affects(LIST)`, which makes an infinite list from a
"load more" form. See [reads that start themselves](docs/interactions.md#reads-that-start-themselves)
and the [triggers example](examples/triggers.rs).

For shared summaries/counts, declare a `VersionedRegion`, mount it with
`counts.mount(revision, contents)`, declare `.affects(counts)`, and reply with
`.also_replace(counts, revision, contents)`. A plain `Region` cannot be passed to
`also_replace`; use plain regions for reads or `.also_append(...)` collections.

For collections whose items are added, removed, or reordered, use a `List`:
mount items with `LIST.item(key).mount(contents)`, declare `.affects(LIST)`, and
reply with `also_insert(item, Position::End)`, `also_move(&item, Position::Before(other))`,
`also_remove(&item)`, or `also_order(&LIST, items)`. Existing items keep their
nodes, so drafts, open editors, focus, and behaviors inside them survive. A
missing item or anchor is skipped and reported in the applied event instead of
rejecting a reply whose write already committed. `also_refetch(&region)` runs a
region's read form again with its current input, `also_refresh(&component, contents)`
refreshes another declared component, and `navigate("/path")` goes to another
page after a successful write.
The server must increment the snapshot revision with each corresponding change.
Build the form and the handler's replies from one function that returns the
binding, so both declare the same regions; debug builds panic when a reply
patches a region its binding did not declare.
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
cargo tasks  # Tasks, counts, dialogs, and live updates: http://127.0.0.1:4319

# Search, with rebuild/reload: http://127.0.0.1:4317
cargo run --features dev --bin placebo -- dev --example search --features dev

# Run an example directly, without the development supervisor:
cargo run --example editors
cargo run --example uploads  # File fields: http://127.0.0.1:4321
cargo run --example nested   # Nested components: http://127.0.0.1:4322
cargo run --example triggers # Lazy, polled, and infinite reads: http://127.0.0.1:4323
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
npx playwright install chromium firefox webkit
PLACEBO_TEST_DEV=1 npm run test:browsers

# Also verify production isolation with the dev feature explicitly enabled:
cargo build --release --features dev --example editors
PLACEBO_TEST_DEV=1 PLACEBO_TEST_RELEASE=1 npm test
```

Rust tests include deliberately uncompilable form/handler examples, working
typed examples, and request/response checks for renamed fields, browser
absence rules, and malformed payloads. The browser tests use real local Axum
servers and run in Chromium, Firefox, and WebKit; `npm test` uses Chromium, and
`PLACEBO_BROWSER=firefox` or `webkit` selects another engine. They cover adverse request
ordering, remounts, independent instances, draft/focus preservation, validation,
conflicts, static reload, Rust rebuild, compile-error recovery, and supervisor
cleanup. Task tests additionally cover coordinated updates, reversed response
delivery, guarded refreshes, conflicts that show another tab's values in
untouched fields, list inserts, moves, reorders, and deletes, cross-component
refreshes, navigation, malformed batches, remounted extra targets, dialog and
button focus, live-region identity, and behavior teardown/restart. Search tests
cover history and refetching. Native tests submit saves, validation, conflicts,
moves, and searches with JavaScript disabled or before the runtime loads. Replay
tests lose responses after and before the write, retry, and submit twice.
Upload tests send files through the runtime and natively, keep a chosen file
across a rejected reply, and refuse oversized files in the browser and server.
Nested tests refresh a component around busy, edited, removed, and dialog
components. Push tests keep two tabs in step through saves, drafts, inserts,
moves, and deletes, skip stale snapshots, drop and resume the stream, and
resync a page whose position is gone. Trigger tests load a lazy section,
poll, pause while hidden, stop with the region, and scroll an infinite list.
Local UI tests open dialogs without JavaScript, keep details and popovers across
replies, and report command buttons without a target. The dev-loop test creates
and removes a temporary application.
IME tests dispatch composition events; they do not drive an OS input method.
An existing Playwright installation can be selected with `PLAYWRIGHT_MODULE`.

## Still open

Generated protocol definitions; resumable or streamed uploads; feeds shared by
several server processes (a feed lives in one process) and per-user feeds;
replay claims inside the application's own transaction; richer state ownership;
general morphing; and an authoring layer evaluated against the Maud baseline. Current verification uses
Playwright's Chromium, Firefox, and WebKit builds on macOS and Linux. WebKit there
approximates Safari; real Safari, mobile browsers, and other platforms are untested.

## References

- [Axum response integration](https://docs.rs/axum/latest/axum/response/index.html)
- [DOM child replacement](https://developer.mozilla.org/en-US/docs/Web/API/Element/replaceChildren)
- [Focus restoration](https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/focus)
- [tower-livereload](https://docs.rs/tower-livereload/0.10.3/tower_livereload/)


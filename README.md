# Placebo

**Type-checked HTML over the wire for Rust and Axum.**

An experimental framework for Rust + Axum server-rendered apps. Rust renders
HTML with Maud. Every save answers with the page it came from, rendered again,
and the browser changes only what differs, keeping what the person owns:
unsaved drafts, focus, open dialogs. A handler checks the input, writes, and
returns its component's contents; it never tracks what else on the page shows
the data. Maud is the current renderer, and the framework's name and API are
provisional.

**Build forms with `fields!`, bind them to actions, and register handlers with
`action.route(handler)`.** These APIs connect your Rust payload, HTML controls,
and handler types. They generate the request configuration and response format
for you. Normal application code should not construct `data-placebo` JSON or
write protocol headers by hand.

## Start an app on Postgres

**With Claude Code**, install the Placebo plugin once:

```
/plugin marketplace add fairhill1/placebo
/plugin install placebo@placebo
```

Then, in an empty directory, ask for an app ("build a small CRM with
Placebo"). The plugin's [skill](plugin/skills/placebo/SKILL.md) has Claude
install Rust if it is missing, check Postgres (asking before installing it),
install the CLI, run `placebo new`, and follow the `AGENTS.md` it writes.
**Other coding agents** can follow that skill file's steps directly.

By hand:

```sh
cargo install --git https://github.com/fairhill1/placebo placebo --features dev --locked
mkdir my-app && cd my-app
placebo new
placebo dev
```

`placebo new` sets up the current directory, or the one it is given. The app
is the quickstart below on Postgres, with the kit, the styles test, and an
`AGENTS.md` (read by Claude Code through `CLAUDE.md`) that gives coding agents
this README's rules, the app's commands, and how to change its schema. Its
database is `placebo_my_app` on the local server, created on the first debug
run; `DATABASE_URL` overrides it, and `dropdb placebo_my_app` removes it. An
app made by a CLI installed from GitHub depends on this repository by git; one
made by a CLI built from a clone depends on that clone by path. The template
is [`template/`](template), a workspace member built and tested with this
repository.

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

- **How the screen updates:** every save answers with the page it came from,
  rendered again, and the browser changes only what differs. A handler checks
  the input, writes, and returns its component's contents. It never lists what
  else on the page shows the data: render every page from current data and it
  follows. Each save renders its page again, so keep pages bounded: paginate
  long lists with a read form.
- **Forms:** derive `FormInput` on the payload struct and write the form with
  `fields!` and typed `Control` values. Render it with
  `ACTION.bind(&component).form(fields)` for saves or
  `Read::new().form(fields)` for searches and filters. Don't write
  `data-placebo` attributes, named inputs for payload fields, or protocol
  headers by hand.
- **Routes:** register every action with `.route(ACTION.path(), ACTION.route(handler))`
  and wrap the finished router with `placebo::native_forms(app)`, which renders
  each reply's page. The handler takes any Axum extractors (state, session),
  then `Input<Payload>` last, and also answers forms sent without JavaScript;
  don't branch on that. A plain route such as `post(save)` skips decoding and
  the request checks; the browser reports it as `unadapted-route`. Other pages
  and assets are ordinary routes. Look up the signed-in user in middleware
  around `native_forms`, which the page render reuses, not in an extractor
  that queries on every request.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount. Mount the
  component on the page its form is on, in every render of that page.
- **Drafts:** a control the person changed keeps its value by itself, on the
  whole page. After a successful save, the submitted controls show the saved
  values. Render the submitted values in `invalid` and the saved record in
  `conflict`. Use `data-placebo-local` only for controls that must stay
  together as one unit or controls a behavior renders.
- **Validation:** mark a rejected control with `.invalid(true)` and link its
  message with `.described_by(id)`. Put feedback in a `role="status"` (or
  `role="alert"`) element; an invalid reply focuses the first invalid control.
  Use `.required()` for fields the browser can check before submitting.
- **Reads:** a search or filter is a read form, `Read::new().on_input(ms).form(fields)`,
  which reads the page it is on with the form's fields as the query and puts
  the query in the address. Render every page from its query (`Input<Q>` in
  the page handler) and build each read form on it from that same `Q`,
  rendering the fields it does not change as hidden controls. "Load more" is
  a read form asking for a longer page (`?shown=40`), with `.on_reveal()` to
  read as it scrolls into view. To poll, render `placebo::refresh_every(ms)`
  while there is something to wait for. Don't rebuild these with `fetch` or
  manual DOM replacement.
- **Dialogs and local UI:** make a dialog the component root with `mount_dialog`,
  and open and close it with `command`/`commandfor` buttons, which work without
  JavaScript. Use `popovertarget` and `details` for other local UI; their open
  state is the person's and survives replies. Use `behavior()` for intent such
  as closing after a save, and listen for `placebo:applied` on `document`.
- **Other pages and tabs:** link pages with plain `a href`; a click shows the
  next page without a document load, and Back and Forward work. Put
  `data-placebo-reload` on a link that must load its page as usual. After
  creating or deleting a record, reply with `.navigate("/path")`. To update
  other open pages, call `feed.changed()` after the write, on a `Feed` the
  pages mount with `feed.mount()`; each page reads itself again.
- **Styles:** pages compose the kit's classes, which its README lists. When a
  layout primitive needs other spacing or width, set its custom property to a
  token on the element, such as `style="--stack-space: var(--space-xs)"`.
  Write no other inline styles, no `<style>` elements, and no new CSS: a new
  visual pattern goes into the kit after the person approves it. The styles
  test (`placebo::styles::Check`) fails on what strays, and the console
  reports a class no stylesheet defines as `[placebo:unknown-class]`; fix the
  cause.
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
| Answer with that component's contents | `binding.reply(editor(...))`, `.invalid(...)`, or `.conflict(...)` |
| Render each reply's page, with or without JavaScript | `placebo::native_forms(app)` |

**A save answers with its page.** `native_forms` renders the page the form was
on again, as a GET with the person's cookies, with the replying component
showing the reply's contents. The runtime morphs that page into the document
with [idiomorph](https://github.com/bigskysoftware/idiomorph): nodes, focus,
and scroll stay, and only what differs changes. So a header, a count, or
another panel that shows the saved data follows by itself. A page rendered
before the one already shown never replaces it; a late reply then shows only
in its own component. If the page cannot be rendered (for example, the record
was deleted and its page answers 404), the reply shows in its component alone
and the console says why. See [how pages update](docs/interactions.md).

Dynamic record IDs work with the typed APIs. Adding a dialog does not require
replacing them either: keep the typed form, mount the dialog with
`mount_dialog`, and open it with a `command="show-modal"` button. See the
complete [task example](examples/tasks.rs).

**Mount on the page; reply with contents.** Passing `component.mount(...)` into
`reply`, `invalid`, or `conflict` fails to compile: mounting returns
`MountedComponent`, while replies accept `Markup`. All three responses should
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
writes that must survive a restart, implement `placebo::replay::ReplayStore` on your database and
install it with `.layer(placebo::replays(store))`. See
[idempotent retries](docs/protocol.md#idempotent-retries).

**Other tabs update live.** A `Feed` tells every page that mounts it that
something changed, over Server-Sent Events; each page reads itself again and
morphs it in, by the same rules as a reply. Mount it with `feed.mount()`,
register `feed.route()`, and call `feed.changed()` after a write. The page
that saved skips the signal its own save caused, since the save's reply is
already that page. A page that reconnects after missing a change reads itself
again. `Feeds` gives each person
or document a feed of its own. See
[live updates](docs/interactions.md#live-updates-across-tabs); the task example
keeps two tabs in step.

**Dialogs and disclosures stay as the person left them.** Use
`component.mount_dialog("heading-id", contents)` to make the native dialog the
component root, and open and close it with `command`/`commandfor` buttons,
which work without JavaScript. An open dialog, `details`, or popover stays
open or closed across replies. See [local UI state](docs/interactions.md#local-ui-state).

**Edits survive replies; everything else follows the server.** Each typed
control on the page is retained on its own. A control keeps its node, value,
focus, and selection while it differs from the value the server last rendered,
that is, while it holds an edit the server has not accepted. Every other control
shows the page's markup. After a successful save, the submitted controls show the
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

A search or filter is a read form: `Read::new().on_input(120).form(fields)`.
It has no handler of its own. It reads the page it is on with its fields as
the query, puts the query in the address, and morphs the page in, keeping what
the person is typing; the newest read wins. The page handler takes
`Input<Search>` and renders from it, so a reload, a bookmark, a save's reply,
and a live update show the same results. See the [search example](examples/search.rs).

"Load more" is a read form asking for a longer page, with `.on_reveal()` to
read as it scrolls into view; entries already shown keep their nodes. To poll,
render `placebo::refresh_every(ms)` while there is something to wait for. See
[reads](docs/interactions.md#reads) and the [triggers example](examples/triggers.rs).

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

Then run `placebo dev` in the app directory. It runs the package's binary with
the `dev` feature; `--bin NAME`, `--example NAME`, and `--features LIST` choose
others. Rust edits trigger a Cargo rebuild and application restart. A failed build leaves the previous server
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

## Styles stay in the kit

Pages compose the CSS kit's classes; [kit/README.md](kit/README.md) lists
them. Coding agents drift from it: when something looks off, they invent a
class and write CSS for it, or add an inline style. Maud accepts any class or
attribute, so these checks run instead.

**In the browser**, the runtime reports each class that no stylesheet on the
page defines, once, as `[placebo:unknown-class]`: usually a guess at the
kit's names. A class that only hooks a script or a test belongs in a data
attribute instead.

**In `cargo test`**, `placebo::styles::Check` fails on styles that left the kit:

```rust
// tests/styles.rs
#[test]
fn styles_stay_in_the_kit() {
    placebo::styles::Check::new()
        .kit("static/kit")
        .app_css("static/app.css")
        .views("src")
        .run();
}
```

The vendored kit must match the kit this Placebo ships; re-skin it with tokens
in `@layer tokens` in the app's stylesheet. Every rule in the app's stylesheet
sits in one of the kit's layers, nothing is `!important`, and outside
`@layer tokens` colours are tokens; margins, padding, gaps, font sizes and
weights, and radii are tokens or 0; and nothing is in px but 1px. The views
write no `<style>` element, and no `style=` attribute but one that sets custom
properties: to tokens when written out, as in
`style="--stack-space: var(--space-xs)"`, or to a value from data, as in
`style=(format!("--badge-bg: {}", tag.colour))`.

**Ask before CSS edits.** With this rule in the app's `.claude/settings.json`,
Claude Code asks the person before it edits or writes any stylesheet with its
file tools, in every permission mode:

```json
{
  "permissions": {
    "ask": ["Edit(**/*.css)"]
  }
}
```

`Edit` rules cover every built-in tool that changes files, `Write` included; a
`Write(...)` path rule is accepted but never consulted. The rule does not see a
stylesheet written through the shell or a script, which is why the test backs
it. See [Claude Code permissions](https://code.claude.com/docs/en/permissions).

## Run this repository's demos

These aliases run the examples inside the Placebo checkout; they are not commands
installed into your new app:

```sh
cargo dev    # Two editors, with rebuild/reload: http://127.0.0.1:4318
cargo tasks  # Tasks, a count, dialogs, and live updates: http://127.0.0.1:4319

# Search, with rebuild/reload: http://127.0.0.1:4317
cargo run --features dev --bin placebo -- dev --example search --features dev

# Run an example directly, without the development supervisor:
cargo run --example editors
cargo run --example uploads  # File fields: http://127.0.0.1:4321
cargo run --example nested   # Nested components: http://127.0.0.1:4322
cargo run --example triggers # Polling and "load more": http://127.0.0.1:4323
cargo run --example pages    # Links between pages: http://127.0.0.1:4324
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
cleanup. Task tests cover whole-page replies: the row, count, and dialogs
following a save, replies arriving out of order, conflicts that show another
tab's values in untouched fields, adding, moving, and deleting rows,
navigation, dialog and button focus, and behavior teardown/restart. Search
tests cover latest-wins ordering, the address, and saves racing a search. Native tests submit saves, validation,
conflicts, moves, and searches with JavaScript disabled or before the runtime
loads. Replay tests lose responses after and before the write, retry, and
submit twice. Upload tests send files through the runtime and natively, keep a
chosen file across a rejected reply, and refuse oversized files in the browser
and server. Nested tests reply around busy, edited, and dialog components. Push
tests keep two tabs in step through saves, drafts, adds, moves, and deletes,
and drop and resume the stream. Trigger tests poll, pause while hidden, stop
when the poll element goes, and scroll an infinite list. Navigation tests
follow links, go Back and Forward to where each page was scrolled, leave
links with a hash, a modifier key, or another page's head to the browser,
drop what was typed on the page left, and show a slow page's progress bar. Local UI tests
open dialogs without JavaScript, keep details and popovers across replies, and
report command buttons without a target. The dev-loop test creates and removes
a temporary application.
IME tests dispatch composition events; they do not drive an OS input method.
An existing Playwright installation can be selected with `PLAYWRIGHT_MODULE`.

## Still open

Generated protocol definitions; resumable or streamed uploads; feeds shared by
several server processes (a feed lives in one process);
replay claims inside the application's own transaction; richer state ownership;
and an authoring layer evaluated against the Maud baseline. Current verification uses
Playwright's Chromium, Firefox, and WebKit builds on macOS and Linux. WebKit there
approximates Safari; real Safari, mobile browsers, and other platforms are untested.

## References

- [Axum response integration](https://docs.rs/axum/latest/axum/response/index.html)
- [idiomorph](https://github.com/bigskysoftware/idiomorph), vendored in `client/idiomorph.js`
- [Focus restoration](https://developer.mozilla.org/en-US/docs/Web/API/HTMLElement/focus)
- [tower-livereload](https://docs.rs/tower-livereload/0.10.3/tower_livereload/)


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

## Create an app

From this checkout, generate the recommended typed starter:

```sh
cargo run --features dev --bin placebo -- new ../my-app
cd ../my-app
cargo dev
```

The starter includes two editors, validation and version checks, development
rebuild/reload, a source-contract test, and `AGENTS.md` instructions for LLM
coding. The dev loop checks for detectable API bypasses before each build.
Use these commands in CI too:

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

## Start from the generated application

For a new app, run `placebo new` and adapt its files. Keep its `.cargo` aliases,
`AGENTS.md`, and `tests/placebo_contract.rs`. This is the supported starting point
for humans and coding agents. Use `cargo dev` while working and `cargo test`
before reporting completion; fix reported errors and verify the changed browser
flows. Do not substitute compilation or a source-check pass for browser testing.

The generated contract test runs `cargo check-placebo`, so an ordinary `cargo test`
also detects recognized raw action routes and handwritten request configuration.
It invokes the checkout's CLI and may compile its development dependencies on the
first test run. It adds no dependencies to release application builds.
`cargo build` and `cargo run` alone still do not execute source checks.

The starter's `src/main.rs` is a complete working example. For an existing Axum
application or an explicitly chosen custom integration, see [manual setup](docs/manual-setup.md).
Do not replace the generated setup just to avoid a check.

## The application API

The starter uses the following form, action, and component APIs.

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
applications you can read and adapt; the generated quickstart needs neither file.

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


See the [corrected Issue Desk comparison](docs/comparison.md) for current evidence
against Datastar: both repaired apps pass 14/14, with important scope limits.

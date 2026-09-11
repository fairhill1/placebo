# Checked application workflow

Placebo applications should use typed forms and action route adapters. Ordinary
Axum pages, assets, and unrelated JSON endpoints remain ordinary Axum routes.

## Start with the typed app

From the Placebo checkout:

```sh
cargo run --features dev --bin placebo -- new ../my-app
cd ../my-app
cargo dev
```

Or install the CLI with `cargo install --path . --features dev --locked`, then
use `placebo new ../my-app`. Placebo is not published yet; generated apps use a
relative path dependency on the CLI's source checkout. `--placebo-path DIR`
selects a different checkout, and `--name NAME` overrides the package name.
The destination must not exist and its parent directory must exist. Generation
does not download dependencies, start a server, or overwrite existing files.

The starter supplies two typed editors, validation, version-conflict handling,
draft retention, normalized saves, and development rebuild/reload. CSS is read
from disk in dev and embedded otherwise. `PLACEBO_ADDR` changes the listening
address; `127.0.0.1:0` selects an available port. Data is in memory.

It also includes `AGENTS.md` with application conventions and verification
instructions. Those instructions are guidance; enforcement of the detectable
bypasses comes from the source check below.

## Run the check locally and in CI

Generated apps have these Cargo aliases:

```sh
cargo check-placebo
cargo test
```

The aliases invoke the CLI from the path dependency; no global installation is
required. Existing apps can use an installed `placebo check` in their package
directory, or `placebo check --path path/to/app`.

`placebo check` returns exit code 1 for detected bypasses, invalid/stale
exceptions, unreadable or invalid Rust source, or an unsupported source layout.
It returns 0 when the selected source passes, printing any explicit exceptions.
Cargo compilation remains a separate check. A CI job for a generated app should
require both steps (after checking out Placebo at the relative dependency path):

```yaml
- name: Check Placebo API use
  run: cargo check-placebo
- name: Check Rust
  run: cargo test
```

Both commands run from the app directory. CI provisioning of the unpublished
path dependency is the application's responsibility; this is a steps excerpt,
not a complete checkout workflow.

`placebo dev` runs the source check before every build, so its normal development
loop also rejects detected bypasses. If the check fails, it prints the findings,
keeps the last working server, and waits for a Rust edit. `cargo run`,
`cargo build`, and `cargo test` alone do not run this check. Configure CI to
require the separate check if it is part of your project's contract.

## What it flags

`raw-config` flags direct Maud `data-placebo=(...)` assignments and HTML string
literals containing `data-placebo=...`. Use `fields!` and
`action.bind(...).form(fields)` to generate that configuration. Supported local
ownership markup such as `data-placebo-local="draft"` is not a bypass.

`untyped-route` flags `.route(...)` registrations for recognized Placebo action
paths when the handler expression is not a visible matching typed adapter:

```rust
const SAVE: MutationAction<SaveInput> = MutationAction::new("save", "/save");

// Flagged: known Placebo endpoint bypasses the adapter.
Router::new().route("/save", post(save));

// Checked handler/payload relationship, extraction, and mutation header.
Router::new().route(SAVE.path(), SAVE.route(save));
```

The same rule covers `ReadAction` and plain `get(handler)` replacements. Read
adapters check the handler/payload relationship too. Middleware on the typed
adapter (`.layer`, `.route_layer`, `.with_state`) and merges of matching typed
adapters are recognized. Unrelated Axum routes are left alone.

## Explicit custom integrations

Put an exception immediately above the flagged source line, with a reason:

```rust
const SAVE: MutationAction<SaveInput> = MutationAction::new("save", "/save");
let app = Router::new()
    // placebo:allow untyped-route -- Vendor bridge validates the mutation header and payload itself.
    .route(SAVE.path(), post(vendor_bridge));
```

Or, for a renderer that intentionally generates its own protocol configuration:

```rust
// placebo:allow raw-config -- External renderer owns this form's protocol integration.
let form = html! { form data-placebo=(vendor_config) { /* ... */ } };
```

An exception applies only to that rule on the next nonblank line. It does not
disable checks for the function, file, or project. Only `raw-config` and
`untyped-route` can be allowed; missing reasons and unknown rules fail. Stale
exceptions with no matching finding fail too. Every used exception prints its
file, line, rule, and reason even when the command succeeds.

Exceptions acknowledge a site the checker cannot validate. They do not make a
custom integration type-safe or supply a missing mutation-header check. Keep
the reason accurate and verify the integration's own contract. Fix accidental
bypasses rather than suppressing them.

## Limits

This is a conservative source check, not compiler enforcement or a security
boundary. It parses `src/**/*.rs` in one package; `--examples` selects
`examples/**/*.rs` instead (also used by `placebo dev --example`). It does not
follow source symlinks; unsupported layouts fail rather than checking zero files.
Unit-test modules marked exactly `#[cfg(test)]` are excluded. Other configuration
branches are inspected regardless of the active Cargo features.

It recognizes direct `ReadAction::new` / `MutationAction::new` calls, simple
explicit imports renaming those types, const/static/local bindings, literal
endpoint strings, and direct `ACTION.path()` uses. Declarations are collected
across the selected files. This is not full Rust name resolution: shadowed names,
re-exports, action values passed through functions, computed paths, custom macros,
and indirect router builders may not be resolved correctly. Valid helper-based
integrations may need an exception at the registration site.

Macros are not expanded; the checker examines Maud tokens for direct attributes.
It does not inspect generated code, external `include!` sources, JavaScript DOM
mutations, separate HTML files, or configuration assembled at runtime. It cannot
prove that every intended Placebo endpoint was declared, or prevent someone
from removing the check itself. These limits are why the result is called a
source check rather than a guarantee that all possible bypasses are forbidden.

Typed forms still provide their compiler checks. Browser tests still establish
interaction behavior: for example, a valid update can accidentally remove a
dialog, and this check cannot infer that the application needed it to remain.

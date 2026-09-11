Write form layout in one Maud block, with typed controls at their actual positions.

`fields!` adds `@field name = control;` to ordinary Maud markup. Names and value
types come from the input struct; every declared field must appear exactly once.
The completed form and its action share that input type.

```rust
use placebo::{Component, Control, FormInput, MutationAction, fields};
use serde::Deserialize;

#[derive(Deserialize, FormInput)]
struct SaveTitle { id: u64, title: String }

const SAVE: MutationAction<SaveTitle> = MutationAction::new("save", "/save");
let component = Component::new("editor", 42);
let fields = fields! { SaveTitle {
    @field id = Control::hidden(42);
    div data-placebo-local="draft" {
        label for="title" { "Title" }
        @field title = Control::text("A quiet workspace").id("title");
    }
    button type="submit" { "Save" }
} };
let form = SAVE.bind(&component).form(fields);

async fn save(_state: (), input: SaveTitle) -> String {
    format!("{}: {}", input.id, input.title)
}
let route: axum::routing::MethodRouter<()> = SAVE.route(save);
```

Labels, layout, and feedback stay in the same block as their controls. Maud
still renders the HTML; `fields!` connects the controls to the generated typed
builder. It returns `FormFields<SaveTitle>`, ready for `.form(fields)`.

## Compile-time contract

A removed or renamed field makes the old form entry invalid:

```compile_fail,E0599
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { heading: String }
let fields = fields! { SaveTitle { @field title = Control::text("Old name"); } };
```

Missing fields fail compilation:

```compile_fail,E0599
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { id: u64, title: String }
let fields = fields! { SaveTitle { @field id = Control::hidden(42); } };
```

A field cannot appear twice, even in separate layout groups:

```compile_fail
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
let fields = fields! { SaveTitle {
    @field title = Control::text("First");
    div { @field title = Control::text("Second"); }
} };
```

Control values must have the declared field type:

```compile_fail,E0308
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { id: u64 }
let fields = fields! { SaveTitle { @field id = Control::hidden("not a number".to_owned()); } };
```

An action cannot take another input type's form:

```compile_fail,E0308
use placebo::{Component, Control, FormInput, MutationAction, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
#[derive(Deserialize, FormInput)]
struct Search { query: String }
let action = MutationAction::<SaveTitle>::new("save", "/save");
let component = Component::new("editor", 1);
action.bind(&component).form(fields! { Search { @field query = Control::text("query"); } });
```

Registering a mutation handler with a different payload type fails:

```compile_fail,E0631
use placebo::{FormInput, MutationAction};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
#[derive(Deserialize, FormInput)]
struct Other { title: String }
async fn wrong(_state: (), _input: Other) {}
let action = MutationAction::<SaveTitle>::new("save", "/save");
let route: axum::routing::MethodRouter<()> = action.route(wrong);
```

Read handlers obey the same rule:

```compile_fail,E0631
use placebo::{FormInput, ReadAction};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Search { query: String }
#[derive(Deserialize, FormInput)]
struct Other { query: String }
async fn wrong(_state: (), _input: Other, _headers: axum::http::HeaderMap) {}
let action = ReadAction::<Search>::new("search", "/search");
let route: axum::routing::MethodRouter<()> = action.route(wrong);
```

A required field cannot live inside a branch that might omit it:

```compile_fail
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
let fields = fields! { SaveTitle {
    @if false { @field title = Control::text("Never rendered"); }
} };
```

Or inside a loop that could duplicate it:

```compile_fail
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
let fields = fields! { SaveTitle {
    @for title in ["First", "Second"] { @field title = Control::text(title); }
} };
```

## Layout and limits

Use ordinary Maud elements for layout and `data-placebo-local="draft"` to retain
a subtree. Maud `@if`, `@match`, and loops work for surrounding content such as
feedback. Required `@field` entries belong in unconditional markup, outside
branches, loops, and Rust splices. To choose a control dynamically, put the Rust
conditional in its value expression: `@field title = if editing { ... } else { ... };`.

Control expressions run once, in field order, before the surrounding markup
renders. Declare shared variables with ordinary Rust `let` statements before
`fields!`, rather than Maud `@let` alongside fields. The lower-level
`Input::fields().with_*().markup(...).finish()` builder remains available for
programmatic construction and is what the macro uses internally.

Supported controls are string text/search inputs, primitive hidden values, and
typed selects. Every declared field is required, including fields marked
`serde(default)`; defaults still apply to incoming requests. Per-field
`serde(rename = "...")` supplies the wire name. Unsupported serde transforms
such as flatten, skip, rename_all, and custom codecs are rejected.

The implementation requires concrete, nonempty structs with named fields.
Optional values, collections, generic payloads, file uploads, checkboxes, and
custom value codecs are not covered yet. Macro expansion currently expects
the dependency to be named `placebo`.

These checks cover `@field`, the typed builder, and `action.route(handler)`.
They do not validate arbitrary HTML semantics: raw controls, disabled fieldsets,
controls inside templates or nested forms, deliberate use of hidden macro
plumbing, manual Axum route registration, browser DOM changes, and crafted HTTP
requests remain outside the contract. Runtime deserialization, validation,
authorization, and DOM checks remain necessary.

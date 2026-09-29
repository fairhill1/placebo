Write form layout in one Maud block, with typed controls at their actual positions.

`fields!` adds `@field name = control;` to ordinary Maud markup. Names and value
types come from the input struct; every declared field must appear exactly once.
The completed form and its action share that input type.

```rust
use placebo::{Component, Control, FormInput, Input, MutationAction, fields};
use serde::Deserialize;

#[derive(Deserialize, FormInput)]
struct SaveTitle { id: u64, title: String }

const SAVE: MutationAction<SaveTitle> = MutationAction::new("save", "/save");
let component = Component::new("editor", 42);
let fields = fields! { SaveTitle {
    @field id = Control::hidden(42);
    label for="title" { "Title" }
    @field title = Control::text("A quiet workspace").id("title");
    button type="submit" { "Save" }
} };
let form = SAVE.bind(&component).form(fields);

async fn save(Input(input): Input<SaveTitle>) -> String {
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

Handlers take any Axum extractors, such as `State` or a session, followed by
`Input` of the action's payload type. Registering a mutation handler with a
different payload type fails:

```compile_fail,E0631
use placebo::{FormInput, Input, MutationAction};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
#[derive(Deserialize, FormInput)]
struct Other { title: String }
async fn wrong(Input(_input): Input<Other>) {}
let action = MutationAction::<SaveTitle>::new("save", "/save");
let route: axum::routing::MethodRouter<()> = action.route(wrong);
```

So does a handler without the input, or with it before other extractors:

```compile_fail,E0631
use placebo::{FormInput, MutationAction};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct SaveTitle { title: String }
async fn wrong(_headers: axum::http::HeaderMap) {}
let action = MutationAction::<SaveTitle>::new("save", "/save");
let route: axum::routing::MethodRouter<()> = action.route(wrong);
```

Read handlers obey the same rule:

```compile_fail,E0631
use placebo::{FormInput, Input, ReadAction};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Search { query: String }
#[derive(Deserialize, FormInput)]
struct Other { query: String }
async fn wrong(_headers: axum::http::HeaderMap, Input(_input): Input<Other>) {}
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

## Controls

Each control constructor only accepts the field types it can submit correctly.

| Field type | Controls |
|---|---|
| `String` / `Option<String>` | `text`, `search`, `email`, `url`, `tel`, `date`, `time`, `datetime_local`, `password`, `textarea`, plus `hidden`, `select`, `radios` |
| integers, `f32`, `f64`, and `Option` of those | `number`, `hidden`, `select`, `radios` |
| `bool` | `checkbox`, `hidden`, `select`, `radios` |
| `#[derive(FormEnum)]` enums and `Option` of one | `select`, `radios`, `hidden` |
| `Vec<T>` of any of the scalars above | `multi_select`, `checkboxes` |
| `Upload`, `Option<Upload>`, `Vec<Upload>` | `file` (a `Vec` accepts several files) |

```rust
use placebo::{Control, FormInput, fields};
use serde::Deserialize;

#[derive(Deserialize, FormInput)]
struct Profile {
    bio: String,
    age: Option<u32>,
    newsletter: bool,
    role: u8,
    #[serde(default)]
    days: Vec<String>,
}

let fields = fields! { Profile {
    label for="bio" { "Bio" }
    @field bio = Control::textarea("").id("bio").rows(4);
    label for="age" { "Age" }
    @field age = Control::number(None).id("age").min(0);
    label { @field newsletter = Control::checkbox(false); " Newsletter" }
    fieldset {
        legend { "Role" }
        @field role = Control::radios(2, [(1, "Owner"), (2, "Editor")]);
    }
    fieldset {
        legend { "Available" }
        @field days = Control::checkboxes(Vec::<String>::new(), [
            ("sat".to_owned(), "Saturday"), ("sun".to_owned(), "Sunday"),
        ]);
    }
} };
```

The typed adapters decode requests the way browsers submit them:

- An unchecked checkbox submits nothing; an absent `bool` field is `false`.
- An empty value, or an absent field, is `None` for every `Option` field,
  including `Option<String>`. An empty value for a number that is not an
  `Option` would be a decoding error, so its `number` control renders
  `required` and the browser asks for a value before submitting. Use
  `Option` when the handler should answer an empty number with its own message.
- `Vec` fields collect repeated values. With nothing selected the browser
  submits nothing, so `Vec` fields need `#[serde(default)]`.
- Any other repeated value is a decoding error, not a silent first/last choice.
- Line breaks decode as `\n`. Browsers submit a textarea's line breaks as CRLF
  and the runtime keeps LF; the handler sees the same value either way.

`radios` and `checkboxes` render one `<label>` per option inside a
`radiogroup`/`group` element; `.id()`, `.class()` and `.described_by()` apply to
that group. Put a `fieldset` and `legend` around it for an accessible name.
Selects and required radios must have an option for their initial value, and
every value selected in a multiple selection needs a matching option.
Optional radios may start as `None` with nothing checked.

`.required()` renders `required` (on every button of a radio group), and
`.max_length(n)` renders `maxlength` on text and textarea controls. After the
server rejects a value, `.invalid(true)` renders `aria-invalid="true"`; link the
message with `.described_by(id)`. An invalid reply focuses the first invalid
control. Every control except `hidden` carries `data-placebo-field`, which lets
the browser keep its unsaved edits across replies (see
[what a reply keeps](interactions.md#what-a-reply-keeps)).

Float `number` controls default to `step="any"`; `.min()`, `.max()` and `.step()`
take the field's number type. `password()` always renders empty so a response
never echoes a password into markup. Date and time controls submit strings in
the browser's `YYYY-MM-DD`/`HH:MM` formats; parse and validate them in the handler.
Attributes that don't apply to a control, such as `.rows()` on a text input,
panic during rendering instead of being ignored.

A `Vec` field without a serde default fails to compile:

```compile_fail,E0080
use placebo::FormInput;
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Filter { labels: Vec<String> }
```

So does a field type no control can submit:

```compile_fail,E0277
use placebo::FormInput;
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Save { due: std::time::SystemTime }
```

A control must match its field type:

```compile_fail,E0308
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Save { count: u32 }
let fields = fields! { Save { @field count = Control::checkbox(true); } };
```

## Files

A payload field of type `Upload<MAX_BYTES>`, `Option<Upload<..>>`, or
`Vec<Upload<..>>` is a file field, rendered with `Control::file()`. Its form is
sent as `multipart/form-data` automatically, by the runtime and by the browser
without JavaScript. `MAX_BYTES` defaults to 10 MiB; for a `Vec` it bounds all
the field's files together. `.accept("image/*")` limits what the file picker
offers and `.required()` asks for a file before submitting.

```rust
use placebo::{Control, FormInput, Upload, fields};
use serde::Deserialize;

#[derive(Deserialize, FormInput)]
struct Attach {
    note: String,
    cover: Option<Upload<{ 64 * 1024 }>>,
    #[serde(default)]
    files: Vec<Upload>,
}

let fields = fields! { Attach {
    @field note = Control::text("");
    @field cover = Control::file().accept("image/*");
    @field files = Control::file();
} };
```

The limit is enforced twice from the one type. The runtime checks the chosen
files before sending and, if they are too large, shows the browser's
validation message on the file input and sends nothing (traced as `ignored`
with reason `upload-too-large`). The server stops reading a file past its limit
and answers 413 before the handler runs, with a page that names the file and
the limit; the runtime reports that as `upload-too-large`. A file input with no
file chosen is an absent file: `None`, an empty `Vec`, or a decoding error for
a required `Upload`. The handler gets each file's bytes, its file name, and
its content type, the last two as untrusted text from the browser.

A file input's value cannot be set by a page, which decides how its draft is
kept. With the runtime, a chosen file is an edit: a rejected reply keeps the
input's node, so the chosen file stays and is sent again with the next submit.
A successful reply for the submitted form replaces the input, which is then
empty. Without JavaScript, a rejected page cannot choose the file again for
the person. Say so in the feedback, for example by naming the files the
handler received, as the [uploads example](../examples/uploads.rs) does.
Files take no part in the native edit hash.

`Control::file()` is only for file fields:

```compile_fail,E0277
use placebo::{Control, FormInput, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Attach { note: String }
let fields = fields! { Attach { @field note = Control::file(); } };
```

and a file field takes no other control:

```compile_fail,E0277
use placebo::{Control, FormInput, Upload, fields};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Attach { cover: Upload }
let fields = fields! { Attach { @field cover = Control::hidden(String::new()); } };
```

A read submits its fields in the URL, so its payload cannot have a file field:

```compile_fail,E0080
use placebo::{FormInput, ReadAction, Upload};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Search { q: String, sample: Option<Upload> }
const SEARCH: ReadAction<Search> = ReadAction::new("search", "/search");
```

Several files need `#[serde(default)]`, like other `Vec` fields:

```compile_fail,E0080
use placebo::{FormInput, Upload};
use serde::Deserialize;
#[derive(Deserialize, FormInput)]
struct Attach { files: Vec<Upload> }
```

## Enums

Derive `FormEnum` on an enum whose variants hold no data, next to serde's
`Serialize` and `Deserialize`. Controls submit each variant's serde name, and the
adapter decodes the same name, so `rename` and `rename_all` apply to forms too.
Any other submitted value is rejected with a 422.

```rust
use placebo::{Control, FormEnum, FormInput, fields};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, FormEnum)]
#[serde(rename_all = "lowercase")]
enum Priority { Low, Normal, High }

#[derive(Deserialize, FormInput)]
struct SaveTask { priority: Priority }

let fields = fields! { SaveTask {
    label for="priority" { "Priority" }
    @field priority = Control::select(Priority::Normal, [
        (Priority::Low, "Low"), (Priority::Normal, "Normal"), (Priority::High, "High"),
    ]).id("priority");
} };
```

The derive rejects variants with data and serde attributes that would make the
rendered name differ from the accepted one, such as `untagged`, `skip`, or
separate `rename(serialize = ..., deserialize = ...)` names.

An enum without the derive is not a form field type:

```compile_fail,E0277
use placebo::FormInput;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
enum Priority { Low, High }
#[derive(Deserialize, FormInput)]
struct SaveTask { priority: Priority }
```

## Layout and limits

Use ordinary Maud elements for layout. Controls keep their own unsaved edits;
`data-placebo-local="key"` retains a subtree as one unit when that is needed. Maud `@if`, `@match`, and loops work for surrounding content such as
feedback. Required `@field` entries belong in unconditional markup, outside
branches, loops, and Rust splices. To choose a control dynamically, put the Rust
conditional in its value expression: `@field title = if editing { ... } else { ... };`.

Control expressions run once, in field order, before the surrounding markup
renders. Declare shared variables with ordinary Rust `let` statements before
`fields!`, rather than Maud `@let` alongside fields. The lower-level
`SaveTitle::fields().with_*().markup(...).finish()` builder remains available for
programmatic construction and is what the macro uses internally.

Every declared field needs a control, including fields marked `serde(default)`;
defaults still apply to incoming requests. Per-field
`serde(rename = "...")` supplies the wire name. Names starting with `placebo-`
are reserved for the fields Placebo adds to forms. Unsupported serde transforms
such as flatten, skip, rename_all, and custom codecs are rejected.

The implementation requires concrete, nonempty structs with named fields.
Generic payloads, enums with data, and custom value codecs are not covered yet. Macro expansion currently expects
the dependency to be named `placebo`.

These checks cover `@field`, the typed builder, and `action.route(handler)`.
They do not validate arbitrary HTML semantics: raw controls, disabled fieldsets,
controls inside templates or nested forms, deliberate use of hidden macro
plumbing, manual Axum route registration, browser DOM changes, and crafted HTTP
requests remain outside the contract. Runtime deserialization, validation,
authorization, and DOM checks remain necessary.

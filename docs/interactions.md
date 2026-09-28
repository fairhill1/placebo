# Coordinated updates and local behaviors

Run `cargo tasks` and open <http://127.0.0.1:4319>. The task-list example
exercises a row editor, its saved summary, a shared completed count, and an
add dialog. State is in memory; restarting the server resets the tasks.

## A response can update several declared regions

A mutation form declares additional destinations with `affects()`. Declare
them once, in a function that both the view and the handler call:

```rust
fn save_binding(task: &Task) -> MutationBinding<SaveTask> {
    SAVE.bind(&Component::new("task", task.id))
        .affects(row_summary(task))
        .affects(SUMMARY)
}
// View
save_binding(task).form(fields)
```

The response still refreshes its originating component. It can also replace
shared snapshots or append new elements to a collection:

```rust
// Handler
save_binding(task).reply(editor_markup)
    .reset_local("draft")
    .also_replace(row_summary(task), task.version, summary_markup)
    .also_replace(SUMMARY, tasks.revision, count_markup)
```

The browser rejects a patch its form did not declare. Replies also carry their
binding's declarations, so in debug builds a handler that patches an undeclared
region panics at the server, before the response is sent. `invalid` and
`conflict` replies can refresh snapshots with `also_replace`, but cannot append.

Declare counts with `VersionedRegion::new("task-count")` and row summaries with
`VersionedRegion::keyed("task-summary", task.id)`. Mount them with
`region.mount(revision, markup)`. The revision is a required argument;
`also_replace` accepts only `VersionedRegion`, so accidentally using a plain
`Region` for shared snapshots fails to compile. Their
monotonic revisions come from the server, not the browser's request order.
Migrate old `Region::mount_versioned` sites by changing the declaration to
`VersionedRegion` and the mount call to `mount(revision, markup)`. Do not replace
the initial revision with a fixed value on replies: types do not prove monotonicity
or that a matching target is actually mounted.

The example updates records and revisions under the same lock. A real store
would need an equivalent consistency guarantee.

Suppose task A commits at revision 8, then task B at revision 9, but B's response
arrives first. A's own editor and row summary can still update when its response
arrives; its count at revision 8 is skipped. Revisions are scoped to each region
and encoded as decimal strings so JavaScript does not round large Rust integers.
This is not live synchronization between tabs.

`also_append(LIST, new_row)` adds a row without replacing existing editors or
their local drafts. Append destinations are unversioned regions. Appends with
duplicate/existing element IDs are rejected; this is not an idempotent retry
protocol, and append order follows response delivery.

The browser captures every declared destination's actual DOM node when a form
is scheduled. Remounting the same ID does not transfer response ownership.
Targets must be distinct and cannot contain one another. Additional targets
must be plain regions; replaceable shared snapshots cannot contain regions.
The collection may contain components, but the experiment still rejects
refreshing a component that contains other components.

All patch targets, operations, revisions, and local keys are checked before
the first DOM change. Invalid batches leave the existing DOM intact and emit
a diagnostic. A valid batch applies synchronously, with stale snapshots
skipped deliberately. This does not roll back a database write: the server
may already have committed even if the browser rejects or loses its response.

## Deliberate draft resets

By default, matching `data-placebo-local="draft"` subtrees retain their existing DOM.
Incoming markup under the same key is ignored, including its attributes.

Successful mutations can request `.reset_local("draft")` to accept the server's
new markup. The browser honors this only if the same local node still exists,
has received no input/change/composition edits since submission, and its control
values still match the captured snapshot. Otherwise the newer draft wins.
The guard belongs to the local subtree: changes in another editor do not block
this editor's reset. Programmatic widgets should dispatch input/change events;
the value comparison is also a fallback for ordinary form controls.

Validation and conflict responses preserve local drafts and cannot request
resets. Active IME composition defers the whole response, including shared
patches, until composition ends. At that point revisions and draft changes are
checked again. A reset can restore focus to an incoming control with the same
ID; retained controls keep focus and selection.

The `placebo:applied` event includes `resetLocal` (keys actually reset) and
`skippedRegions` (snapshots skipped because their revision was not newer).
Lifecycle events are dispatched on `document`; register listeners with
`document.addEventListener("placebo:applied", ...)` and filter `detail.target`.
A listener on a component or page container will not receive those events.
The dialog example closes on success only when its draft was actually reset.
If the user is already writing something newer, it stays open.

## Small browser behaviors

`behavior(name, setup)` registers lifecycle-managed JavaScript for elements
with `data-placebo-behavior="name"`:

```js
import { behavior } from "/placebo.js";

const unregister = behavior("example", element => {
  const listeners = new AbortController();
  element.addEventListener("click", handleClick, { signal: listeners.signal });
  return () => listeners.abort();
});
```

Setup runs for initial elements and newly appended/replaced elements. Matching
local nodes moved during a refresh keep their behavior instance. Cleanup runs
when an element is removed, its behavior changes, the registration is removed,
or the runtime stops. `start()` mounts again after `stop()`. DOM moves are
reconciled after the mutation batch, so a move is not a teardown/remount.
Setup and cleanup errors identify their behavior/element and preserve the cause.
Unknown names are diagnosed after initialization; use `lazyBehavior()` to reserve
intentional asynchronous registration. One behavior name is supported per
element. See [diagnostics](diagnostics.md) for tracing and loading behavior.

The example delegates open/close clicks and uses a native `<dialog>` for
modality, keyboard behavior, and focus containment. The dialog and behavior
root stay persistent. The edit dialogs contain a mounted form component. The
add dialog uses `component.mount_dialog("add-heading", add_contents(...))`,
which mounts the dialog itself as the component root. In both cases the native
dialog node survives every reply. A row summary may replace its Edit button; delegation handles the new
button, and close restores focus to the current trigger. There is no reactive
expression language or global client state store in this experiment.

## Persistent dialog recipe

Render the same complete contents on initial mount, validation, conflict and
success. Include the heading, form and feedback each time:

```rust
// Initial page: MountedComponent renders inside Maud's html!.
html! { (component.mount_dialog("add-heading", add_contents("", ""))) }
// Handler: Markup contents only. The dialog root is never in this fragment.
binding.invalid(add_contents(&input.title, "Use 3–80 characters."))
binding.reply(add_contents("", "Saved.")).reset_local("draft")
```

`mount()` and `mount_dialog()` return `MountedComponent`, so passing a mount
directly to `reply`, `invalid` or `conflict` fails to compile. Arbitrary Maud
composition can erase that type distinction; nested component wrappers remain
runtime errors. A dialog anywhere inside replaceable component contents,
including local subtrees, produces `unstable-dialog`. The runtime checks the
mounted shape before sending and the incoming shape before applying any patches.
Keep dialogs at the component root or outside the refreshed component.

This guards native dialog lifetime, not arbitrary application structure. A reply
that omits a heading, button or form can still be valid HTML and wrong UI. Browser
flow tests are still required. Plain read regions have replacement semantics;
these mutation-component checks do not make dialogs inside read regions persistent.

## What this experiment tells us

Shared revisions and explicit local ownership solve different problems. Both
are needed: keeping drafts alone cannot prevent a completed count from moving
backwards, and ordering snapshots alone cannot protect typing during a save.

The behavior hook is sufficient for these dialogs without changing the HTML
renderer. Forms now use `fields!` to keep Maud layout and typed controls in one
block; the typed builder remains underneath for field-completion checks. Nested
component refreshes, uncertain-write recovery, general reactive state,
navigation, and persistent storage remain separate work.

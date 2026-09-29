# Coordinated updates and local behaviors

Run `cargo tasks` and open <http://127.0.0.1:4319>. The task-list example
exercises a row editor, its saved summary, a shared completed count, an add
dialog, and a keyed list whose rows can be deleted and moved. State is in memory; restarting the server resets the tasks.

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
Do not replace
the initial revision with a fixed value on replies: types do not prove monotonicity
or that a matching target is actually mounted.

The example updates records and revisions under the same lock. A real store
would need an equivalent consistency guarantee.

Suppose task A commits at revision 8, then task B at revision 9, but B's response
arrives first. A's own editor and row summary can still update when its response
arrives; its count at revision 8 is skipped. Revisions are scoped to each region
and encoded as decimal strings so JavaScript does not round large Rust integers.
This is not live synchronization between tabs.

## Lists: insert, move, remove, reorder

A `List` holds items the server can add, move, and remove one at a time:

```rust
const LIST: List = List::new("tasks");
// View
LIST.mount(html! { @for task in tasks.in_order() { (LIST.item(task.id).mount(row(task))) } })
// Handlers, with .affects(LIST) on each binding
reply.also_insert(LIST.item(id).mount(row(&task)), Position::End)
reply.also_move(&LIST.item(id), Position::Before(LIST.item(next)))
reply.also_remove(&LIST.item(id))
reply.also_order(&LIST, ids.iter().map(|id| LIST.item(id)))
```

Items keep their DOM nodes. Moving a row keeps an unsaved draft in its dialog,
its behaviors, and focus (`moveBefore` where the browser has it; elsewhere the
runtime refocuses the moved element, but an open modal dialog inside a moved
item loses its modality). Removing an item with focus moves focus to a
neighbouring item. A form inside an item can move or delete it: a list may
contain the replies' other targets.

The server does not know what the browser currently shows, so item updates are
lenient where a strict check would reject a committed write: a missing item is
skipped, an anchor that is gone places the item at the end, and `order-items`
leaves items the server did not list after the listed ones. The applied event
reports `missingItems` and `misplacedItems`. Inserting an id that is already on
the page is still rejected (`duplicate-append`); use `also_move` for an item
that exists. List updates are not versioned: out-of-order replies apply in
delivery order.

`also_append(REGION, markup)` remains for plain regions that only grow.

When the collection is a filtered search result, the server cannot know the
filter the person typed. Declare the results region and reply with
`.also_refetch(&RESULTS)`: the browser runs the region's read form again with
its current input, so a new record appears only if it matches.

## Other components and navigation

`.affects(&component)` and `.also_refresh(&component, contents)` refresh
another component, for example an editor that publishing just locked. Its
controls follow the rejected-reply rule below: edited ones keep their edits.
The refresh is skipped (and reported in `skippedComponents`) while that
component has its own request in flight, since that reply carries its state.
Components have no revisions yet, so an older reply for that component that
arrives later can still overwrite the refresh.

`.navigate("/path")` on a successful reply goes to another page after applying
the batch, such as a record just created or the list after a delete. Only
same-site paths are accepted.

The browser captures every declared destination's actual DOM node when a form
is scheduled. Remounting the same ID does not transfer response ownership.
Targets must be distinct and cannot contain one another. Additional targets
must be plain regions; replaceable shared snapshots cannot contain regions.
A list or plain collection may contain components, but the experiment still
rejects refreshing a component that contains other components.

All patch targets, operations, revisions, and local keys are checked before
the first DOM change. Invalid batches leave the existing DOM intact and emit
a diagnostic. A valid batch applies synchronously, with stale snapshots
skipped deliberately. This does not roll back a database write: the server
may already have committed even if the browser rejects or loses its response.

## What a reply keeps

Every typed control is its own local unit. The browser decides per control,
from what it can observe, without keys or reset lists:

- A control is **edited** when it differs from its defaults, the markup the
  server last rendered for it. Typing and then restoring the old value makes it
  unedited again.
- **Rejected replies** (`invalid`, `conflict`) replace unedited controls and
  keep edited ones. Render the submitted values in `invalid` and the saved
  record in `conflict`; the person's edits stay, and the other fields show the
  current data, so saving again cannot revert another person's change.
- **Successful replies** also replace the edited controls of the submitting
  form, showing the saved and normalized values. Controls of other forms in the
  component keep their edits.
- A control changed **after the request was sent** is always kept.
- A kept control still takes the reply's `aria-invalid`, `aria-describedby`,
  `aria-errormessage`, `disabled`, `readonly`, and `required`.

`data-placebo-local="key"` (or `fields.local(...)`) retains a whole subtree as
one unit under the same rules, for controls that must stay together or that a
behavior renders. A subtree without form controls is always kept, since the
browser cannot tell what changed in it. Programmatic widgets should dispatch
input/change events so a change during a request is noticed.

Active IME composition defers the whole response, including shared patches,
until composition ends. At that point revisions and edits are checked again.

## Focus and announcements

Retained controls keep focus and selection. When the focused element is
replaced, for example the submit button, focus moves to the matching element
in the new markup: the same id, else the same kind of element with the same
text or position. An invalid reply moves focus to the first control marked
`aria-invalid="true"` (use `Control::invalid`), unless the person has moved
elsewhere or kept typing. Live regions (`role="status"`, `role="alert"`,
`aria-live`) outside local units keep their node and take the new contents,
because screen readers announce changes to a live region they already know
about, not a newly inserted one.

The `placebo:applied` event includes `refreshedLocal` (units that took the
reply's markup), `preservedLocal` (units kept, with a reason), and
`skippedRegions` (snapshots skipped because their revision was not newer).
Lifecycle events are dispatched on `document`; register listeners with
`document.addEventListener("placebo:applied", ...)` and filter `detail.target`.
A listener on a component or page container will not receive those events.
The dialog example closes on success only when nothing was preserved. If the
user is already writing something newer, it stays open.

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
binding.reply(add_contents("", "Saved."))
```

`mount()` and `mount_dialog()` return `MountedComponent`, so passing a mount
directly to `reply`, `invalid` or `conflict` fails to compile. Arbitrary Maud
composition can erase that type distinction; nested component wrappers remain
runtime errors. A dialog anywhere inside replaceable component contents,
including local subtrees, produces `unstable-dialog`. The runtime checks the
mounted shape before sending and the incoming shape before applying any patches.
Keep dialogs at the component root or outside the refreshed component.

Without JavaScript, a rejected save renders the page again with its component
showing the reply. A `mount_dialog` root is rendered `open` then, so the person
sees the feedback. A component inside a dialog the application renders cannot
open that dialog, so prefer `mount_dialog` for dialog forms.

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
component revisions, and persistent storage remain separate work.

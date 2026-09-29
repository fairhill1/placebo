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
its behaviors, focus, and an open modal dialog (`moveBefore` where the browser
has it; elsewhere the runtime refocuses the moved element and shows the dialog
modally again, which fires `toggle` but not `close`). Removing an item with focus moves focus to a
neighbouring item. A form inside an item can move or delete it: a list may
contain the replies' other targets.

The server does not know what the browser currently shows, so item updates are
lenient where a strict check would reject a committed write: a missing item is
skipped, an anchor that is gone places the item at the end, and `order-items`
leaves items the server did not list after the listed ones. The applied event
reports `missingItems` and `misplacedItems`. Inserting an item that is already
in the list keeps the existing one and reports it in `existingItems` (a reply
and a push can both insert it); use `also_move` to move an item. An inserted id
that another element on the page already uses is rejected (`duplicate-append`).
List updates are not versioned: out-of-order updates apply in delivery order.

`also_append(REGION, markup)` remains for plain regions that only grow.

When the collection is a filtered search result, the server cannot know the
filter the person typed. Declare the results region and reply with
`.also_refetch(&RESULTS)`: the browser runs the region's read form again with
its current input, so a new record appears only if it matches.

## Reads that start themselves

A read binding can read without the person asking:

```rust
// A slow section, left out of the first render and filled in after it.
LOAD_STATS.bind(STATS).on_load()
// A value that changes on the server's own schedule.
TICK.bind(CLOCK).every(1000)
// The "load more" form at the end of a list, inside its own region.
LOAD_MORE.bind(MORE).on_reveal().affects(ENTRIES)
// Its handler: the next form, and the next entries appended to the list.
more_binding().reply(more_form(next)).also_insert(entry(n), Position::End)
```

A trigger belongs to its form element. `on_load` reads once when the form is
mounted, including forms that a reply inserts later. `on_reveal` reads once
when the form comes within 200px of the viewport. A form inside its own region
is replaced by the reply, so a "load more" form that renders the next one keeps
loading while the end of the list is in view, and stops when the reply renders
no form. `every(ms)` polls while its form and region are on the page: it waits
for a slow read instead of cancelling it, pauses while the page is hidden, reads
once when it is shown again, and stops when its region is removed.

Reads may insert items into a `List` their binding declares with
`.affects(LIST)`, but not move, remove, or replace anything else: a read shows
data, it does not change what other parts of the page own. Inserted items keep
the rules of the list: an item already there keeps its node.

Without JavaScript, a triggered form is a plain GET form: give it a submit
button ("Load more", "Show statistics") and have the read handler render the
whole page for a request that is not an update, as the
[triggers example](../examples/triggers.rs) does.

Poll when the data changes on its own schedule (a clock, a queue fed by
another system) or when holding a connection per page is not wanted. Push with
a `Feed` when the changes come from writes in this application: each page gets
each change once, when it happens, instead of asking every interval.

## Other components and navigation

`.affects(&component)` and `.also_refresh(&component, contents)` refresh
another component, for example an editor that publishing just locked. Its
controls follow the rejected-reply rule below: edited ones keep their edits.
The refresh is skipped (and reported in `skippedComponents`) while that
component has its own request in flight, since that reply carries its state.

A component that other actions or a feed refresh should carry a revision, the
version of the data its contents show: `Component::new("task", id).revision(task.version)`,
on its mount and on every binding. Replies to its own form then apply unless
they are older than what the page shows (an equal revision still applies, so
`invalid` and `conflict` show their feedback), and any other refresh applies
only when newer. Build a successful reply's binding from the record after the
write. Once a component is mounted with a revision, a refresh without one is
rejected with `missing-revision`, since it could not be ordered.

`.navigate("/path")` on a successful reply goes to another page after applying
the batch, such as a record just created or the list after a delete. Only
same-site paths are accepted.

The browser captures every declared destination's actual DOM node when a form
is scheduled. Remounting the same ID does not transfer response ownership.
Targets must be distinct and cannot contain one another. Additional targets
must be plain regions; replaceable shared snapshots cannot contain regions.
A list or plain collection may contain components, and a component may contain
other components (below).

## Nested components

A component's contents may mount other components. Each one is its own
component: it has its own id (`kind:key`, unique on the page), its own forms,
local units, and requests, and replies, `also_refresh`, and pushes address it
by that id. A node belongs to its nearest component.

When the outer component refreshes, its reply renders the inner components'
mounts again, and the browser matches them by id:

- A nested component that is still there **keeps its node**, so its root
  attributes, behaviors, focus, and an open or modal `mount_dialog` survive.
  It takes its new contents by the rule for a refresh from another action:
  controls with edits keep them, everything else shows the reply.
- A nested component with its **own request in flight** keeps its contents
  untouched; its own reply carries its state. The applied event reports it in
  `skippedComponents` with reason `busy`.
- A nested component the reply **leaves out is removed**. A request it had in
  flight is discarded, with the `mutation-interrupted` warning if it was a write.
- A new one is mounted like any other markup.

The outer component's local units and live regions are only its own, so two
nested components can render the same form without a `duplicate-local` clash.
Refreshing a nested component never touches the outer one. A form belongs to
its nearest component: a form inside a nested component that targets the outer
one is rejected with `invalid-component` before sending, and a reply cannot
also refresh a component inside its own target (`overlapping-targets`), since
the target's contents already refresh it. A nested component must keep its
root element (`mount` or `mount_dialog`) across renders, or the reply is
rejected with `nested-component`. The
[nested example](../examples/nested.rs) mounts entries and a notes dialog inside
a checklist.

All patch targets, operations, revisions, and local keys are checked before
the first DOM change. Invalid batches leave the existing DOM intact and emit
a diagnostic. A valid batch applies synchronously, with stale snapshots
skipped deliberately. This does not roll back a database write: the server
may already have committed even if the browser rejects or loses its response.

## Live updates across tabs

A `Feed` sends the same updates as a reply to every page that mounts it, over
Server-Sent Events: versioned region replacements, versioned component
refreshes, and list item operations. The task example keeps every open tab in
step:

```rust
fn live_feed() -> Feed {
    Feed::new("tasks-live", "/live/tasks")
        .affects(LIST)
        .affects(SUMMARY)
        .affects_kind("task-summary") // every VersionedRegion::keyed("task-summary", id)
        .affects_kind("task")         // every Component::new("task", id)
}
// Router
.route(live.path(), live.route())
// Page, under the lock the tasks are read with
(app.live.mount())
// Handler, after the write and under the same lock
app.live.push()
    .replace(row_summary(task), task.version, summary(task))
    .refresh(&task_component(task), edit_form(task, ...))
    .replace(SUMMARY, tasks.revision, count(&tasks))
    .send();
```

Pushes and replies are ordered by revision, whichever arrives first. The tab
that saved gets both its reply and the push: the second one is not newer and
is skipped. A pushed refresh follows the rule for a refresh from another
action, so an open editor keeps its edited fields and shows the rest. While a
component has its own request in flight, a pushed refresh waits for that reply
and then applies only if it is still newer. An insert of an item that is
already there keeps the item (reported in `existingItems`). Declared targets
that this page does not show are skipped (`missingTargets`).

The mount records the feed's position, so updates published after the page
was read are replayed when it connects. Render it under the lock or
transaction the page reads its data with, and publish under the write's lock,
so the feed's order matches the revisions. When the connection drops, the
browser reconnects with its last event id and the feed replays what the page
missed from its last 256 updates. If they are gone, or the server restarted,
the feed tells the page to resync: the runtime reads the page again and takes
each declared target's state from it by the same rules (newer revisions only,
list items inserted, removed, and ordered to match).

A feed broadcasts to every subscriber. Keep per-user data in feeds of their
own and guard a feed's route like any other route. Polling (a read with
`.every(ms)`) suits data that changes on its own schedule or a page that must
not hold a connection; a feed suits changes caused by writes in this app.

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
directly to `reply`, `invalid` or `conflict` fails to compile. A dialog inside
replaceable component contents, including local subtrees, produces
`unstable-dialog`, unless it is the root of a nested `mount_dialog` component,
which keeps its node when the outer component refreshes. The runtime checks the
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
block; the typed builder remains underneath for field-completion checks. General
reactive state and persistent storage remain separate work.

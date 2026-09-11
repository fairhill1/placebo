# Coordinated updates and local behaviors

Run `cargo tasks` and open <http://127.0.0.1:4319>. The task-list example
exercises a row editor, its saved summary, a shared completed count, and an
add dialog. State is in memory; restarting the server resets the tasks.

## A response can update several declared regions

A mutation form declares additional destinations with `affects()`:

```rust
SAVE.bind(&component)
    .affects(row_summary(task))
    .affects(SUMMARY)
    .form(fields)
```

The response still refreshes its originating component. It can also replace
shared snapshots or append new elements to a collection:

```rust
binding.reply(editor_markup)
    .reset_local("draft")
    .also_replace(row_summary(task), task.version, summary_markup)
    .also_replace(SUMMARY, tasks.revision, count_markup)
```

`Region::keyed("task-summary", task.id)` supplies a runtime instance name.
Regions with shared snapshots use `mount_versioned(revision, markup)`. Their
monotonic revisions come from the server, not the browser's request order.
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
root live outside the refreshed form component, so their lifetime remains
stable. A row summary may replace its Edit button; delegation handles the new
button, and close restores focus to the current trigger. There is no reactive
expression language or global client state store in this experiment.

## What this experiment tells us

Shared revisions and explicit local ownership solve different problems. Both
are needed: keeping drafts alone cannot prevent a completed count from moving
backwards, and ordering snapshots alone cannot protect typing during a save.

The behavior hook is sufficient for these dialogs without changing the HTML
renderer. Forms now use `fields!` to keep Maud layout and typed controls in one
block; the typed builder remains underneath for field-completion checks. Nested
component refreshes, uncertain-write recovery, general reactive state,
navigation, and persistent storage remain separate work.

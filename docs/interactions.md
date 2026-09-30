# How pages update

Run `cargo tasks` and open <http://127.0.0.1:4319>. The task-list example has
a row editor in a dialog, a completed count, an add dialog, and rows that can
be deleted and moved. Open it in two tabs to see changes arrive live. State is
in memory; restarting the server resets the tasks. The
[nested](../examples/nested.rs), [uploads](../examples/uploads.rs),
[triggers](../examples/triggers.rs), and [pages](../examples/pages.rs)
examples each focus on one feature.

## A save answers with its page

A handler replies with its component's contents and nothing else:

```rust
async fn save(State(app): State<App>, Input(input): Input<SaveTask>) -> Response {
    let mut tasks = app.tasks.lock().unwrap();
    // ... validate, check the version, write ...
    app.live.changed();
    save_binding(input.id)
        .reply(edit_form(task, &task.title, task.done, "Saved."))
        .into_response()
}
```

`placebo::native_forms(app)` renders the page the form was on again, as a GET
with the person's cookies (and any the handler set), with the replying
component showing those contents. The runtime morphs that page into the
document: an element that matches one already there keeps its node and takes
the new attributes and children, new elements are added, and elements the page
no longer renders are removed. The row's summary, the completed count, and
every other part of the page show the saved data because the page is rendered
from it, not because the handler named them.

`.navigate("/path")` on a successful reply goes to another page instead, such
as a record just created or the list after a delete. Only same-site paths are
accepted.

### What a save costs

A save costs its write plus one render of its page. Placebo adds nothing
measurable to that, but whatever the page costs, every save made from it pays
again. Measured against Postgres, a save answered with a film page took 1.5 ms
where the same save answered with its component alone took 1.0 ms; the
difference was the page's own queries. Three habits keep that small and safe:

- **Keep pages bounded.** Paginate a long list with a read form (`?page=2`, or
  "load more" with `on_reveal`) rather than rendering every row. A form on a
  page of 1,000 rows renders all 1,000 again on each save.
- **Keep page handlers free of writes.** A page handler runs on every save
  made from its page, not only when someone opens it. Counting a view,
  marking a message read, or appending to an audit log there happens again on
  each save; do it in the action that means it.
- **Look up the session once.** The page render is a second request through
  the app, so an extractor that reads the session from the database runs
  twice per save. Do the lookup in middleware around `native_forms` instead.
  The page render gets the save request's extensions, so its handler reads the
  user from there without a query:

```rust
async fn load_session(State(db): State<Db>, mut request: Request, next: Next) -> Response {
    let user = db.user_for_cookie(request.headers()).await;
    request.extensions_mut().insert(user);
    next.run(request).await
}

let app = placebo::native_forms(app)
    .layer(axum::middleware::from_fn_with_state(db, load_session));
```

A handler that signs someone in or out replies with `.navigate(path)`, so no
page is rendered with the session the request came with. The app `placebo new`
makes does all of this in `src/auth.rs`.

### Replies stay in order

Two saves can be in flight at once, and their replies can arrive in either
order. Each page carries the time it began rendering (`X-Placebo-Rendered`),
taken after the handler's write, so a later page shows every write whose page
is earlier. The runtime never lets an earlier page replace a later one: a late
reply then shows only in its own component, and the applied event says
`page: "older"`. A component with its own request in flight is left alone by
other replies; its own reply carries its state.

### When the page cannot be rendered

If the page answers with an error, for example a stale tab saving a record
another tab deleted, whose page now answers 404, the reply shows in its
component alone and the console warns `page-missing`. If the router is not
wrapped with `native_forms`, every reply shows in its component alone and the
console reports `page-error`. If the page renders but does not mount the
replying component, the rest of the page updates and a rejected reply is
reported as `unmounted-target`: mount the component on the page its form is
on, in every render of that page.

## What a reply keeps

Every control on the page is decided on its own, from what the browser can
observe, without keys or reset lists. Typed controls (from `fields!`) are
matched by field name, so one keeps its edits even if it moves; any other
control keeps its value while it is edited.

- A control is **edited** when it differs from its defaults, the markup the
  server last rendered for it. Typing and then restoring the old value makes it
  unedited again. An edited control keeps its node, value, focus, and selection.
- **Rejected replies** (`invalid`, `conflict`) replace unedited controls and
  keep edited ones. Render the submitted values in `invalid` and the saved
  record in `conflict`; the person's edits stay, and the other fields show the
  current data, so saving again cannot revert another person's change.
- **Successful replies** also replace the edited controls of the submitting
  form, showing the saved and normalized values. A read form's controls take
  the page read for them the same way. Controls anywhere else on the page keep
  their edits.
- A form with an edited control also keeps its **hidden controls**, such as a
  record's version, through a refresh from another action or a live update.
  Its next save is then checked against the version the person started from,
  so it gets a conflict instead of overwriting a change they never saw. The
  form's own replies update them.
- A control changed **after the request was sent** is always kept.
- A kept control still takes the page's `aria-invalid`, `aria-describedby`,
  `aria-errormessage`, `disabled`, `readonly`, and `required`.

`data-placebo-local="key"` (or `fields.local(...)`) retains a whole subtree as
one unit under the same rules, for controls that must stay together or that a
behavior renders. A subtree without form controls is always kept, since the
browser cannot tell what changed in it. Programmatic widgets should dispatch
input/change events so a change during a request is noticed.

The person also owns what they opened: an open `dialog`, `details`, or popover
stays as they left it. An active IME composition defers the whole page until
composition ends.

## Focus and announcements

The morph keeps the focused element when it stays on the page. When it is
removed, for example a button the reply no longer renders, focus moves to the
matching element in the new markup: the same id, else the same kind of element
with the same text or position. An invalid reply moves focus to the first
control marked `aria-invalid="true"` (use `Control::invalid`), unless the person
has moved elsewhere or kept typing. A live region (`role="status"`,
`role="alert"`, `aria-live`) that stays in place keeps its node and takes the
new text, so screen readers announce it; give it an id to be sure it matches.

The `placebo:applied` event includes `refreshedLocal` (units that took the
reply's markup), `preservedLocal` (units kept, with a reason),
`refreshedComponents`, `skippedComponents` (with reason `busy`), `refetched`
(reads run again), and `page` (`whole`, `older`, or `missing`). Lifecycle
events are dispatched on `document`; register listeners with
`document.addEventListener("placebo:applied", ...)` and filter
`detail.target`. The dialog example closes on success only when nothing was
preserved. If the person is already writing something newer, it stays open.

## Nested components

A component's contents may mount other components. Each one has its own id
(`kind:key`, unique on the page), its own forms, local units, and requests. A
node belongs to its nearest component, and a form belongs to its nearest
component: a form inside a nested component that targets the outer one is
rejected with `invalid-component` before sending. Two nested components can
render the same form without a `duplicate-local` clash.

Every component on the page follows the same rules when a page arrives: a
component with its own request in flight is left alone, and the others take the
page's contents with edited controls kept. A nested component must keep its
root element (`mount` or `mount_dialog`) across renders, or the reply is
rejected with `nested-component`, and cannot sit inside a `data-placebo-local`
subtree (`local-component`). The [nested example](../examples/nested.rs) mounts
entries and a notes dialog inside a checklist.

Every node, local key, and root element is checked before the first DOM change.
An invalid page leaves the existing DOM intact and emits a diagnostic. This does
not roll back a database write: the server may already have committed even if
the browser rejects or loses its response.

## Reads

A search or filter is a read form. It has no action of its own: it reads the
page it is on, with its fields as the query.

```rust
Read::new().on_input(120).form(fields! { Search {
    @field q = Control::search(&search.q).id("film-q");
    @field status = Control::select(search.status, statuses);
} })
```

When the person types (after the pause) or submits, the runtime puts the query
in the address, replacing the current history entry, fetches the page at it,
and morphs it in like any refresh: the search box keeps what is being typed,
focus and scroll stay, and everything else on the page shows the new query.
The newest read wins; an older response is dropped even if the network
delivers it. Without JavaScript the browser loads the same URL.

So the page handler is the search handler. Render every page from its query
with `Input<Q>`, and build each read form on the page from the same `Q`.
A form replaces the whole query, so a form renders the fields it does not
change as hidden controls: a "load more" form carries the search, and a
search form leaves the count out to start from the first page again. A
reload, a bookmark, a save's reply, and a live update all render the same
query, since it is in the address.

A save sent before a read and answered after it was rendered for the old
query. Its component shows the reply, and the page is read again at the new
query, so the rest shows the write too.

### Reads that start themselves

```rust
// The "load more" form at the end of a list: asks for the next entries too.
Read::new().on_reveal().form(fields! { Shown {
    @field shown = Control::hidden(shown + PAGE);
    button type="submit" { "Load more" }
} })
// Read the page every second, while a job is running.
@if job.running { (placebo::refresh_every(1000)) }
```

`on_reveal` reads when the form comes within 200px of the viewport. The page
it reads renders the longer list and the next form, whose fields ask for more,
so it watches again; entries already shown keep their nodes and the scroll
position stays. It stops when the page renders no form, and a read that failed
does not repeat by itself (the button is still there).

`refresh_every(ms)` reads the page again on its interval while the element is
on the page. It lets a slow read finish instead of starting another, pauses
while the page is hidden, and reads once when it is shown again. Render it only
while there is something to wait for: when the page stops rendering it,
polling stops.

Poll when the data changes on its own schedule (a clock, a queue fed by
another system) or when holding a connection per page is not wanted. Use a
`Feed` when the changes come from writes in this application.

## Links between pages

A click on a link within the site reads the linked page and shows it without
a document load, so the browser shows no loading bar and the runtime, its
feeds, and the page's scripts carry on. The new page replaces the old one
whole, as a load would: nothing typed on the page left carries over, even
into a field of the same name on the next record. The address and title
change, the page starts at the top (or at the link's `#fragment`), and focus
moves to the `autofocus` element or the first `h1`, which a screen reader
then reads. A page that takes more than 300 ms shows a thin bar along the top
in the theme's primary colour (`--primary`).

Back and Forward read their page again, so it shows current data, and return
to where it was scrolled. A save that replies with `.navigate(path)` goes
there the same way.

The browser loads the page as usual for a link to another site, one with
`target` or `download`, one clicked with a modifier key (to open a new tab),
a link to a `#fragment` on the same page, and a link inside an element with
`data-placebo-reload`. So does a page whose head loads other scripts or
stylesheets than this one, an answer that is not a page (a file, a CSV), and
a read that fails, so the browser shows why.

A tab that never loads a page keeps the scripts and stylesheets it loaded
first, including after a deploy. Put a version in their URLs, such as
`/static/app.css?v=2` and `/placebo.js?v=2`, and change it when they change:
the next link then loads the page with the new ones.

## Live updates across tabs

A `Feed` tells every page that mounts it that what it shows has changed, over
Server-Sent Events. Each page reads itself again and morphs it in by the rules
above, as a refresh from another action: an open editor keeps its edited fields
and shows the rest, and a component with a request in flight is left alone.
The page that saved skips the signal its own save caused: the save's reply is
already that page, and reading it again would only replace the reply's
feedback.

```rust
fn live_feed() -> Feed {
    Feed::new("tasks-live", "/live/tasks")
}
// Router
.route(live.path(), live.route())
// Page, under the lock the tasks are read with
(app.live.mount())
// Handler, after the write
app.live.changed();
```

The mount records the feed's position, so a change made after the page was
rendered reaches it when it connects: render it under the lock or transaction
the page reads its data with. When the connection drops, the browser
reconnects; if it missed a change, or the server restarted, it reads the page
again. Signals close together may reach a page as one, and a page reads itself
at most once more while a read is running. Pages are ordered like replies, so a
page read earlier never replaces a later one.

Every page following a feed reads itself again, so each renders what its viewer
may see. To tell only some pages, use `Feeds`, a family of feeds with one per
key:

```rust
let inboxes: Feeds<u64> = Feeds::new("inbox", "/live/inbox");
// Page, for the signed-in person
(inboxes.get(&user.id).mount())
// Route: the handler picks the person's own feed
.route(inboxes.path(), get(|user: User, request: Request| async move {
    inboxes.get(&user.id).stream(&request)
}))
// Handler, after a write, for each person it concerns
inboxes.get(&recipient).changed();
```

A path with `{key}`, such as `/live/boards/{key}`, puts the key in each feed's
URL instead; check that the person may follow it, as on any other route. A
keyed feed nobody has followed or mounted for ten minutes is dropped, so get
it when you mount or signal rather than keeping it. A feed lives in one server
process.

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

Setup runs for initial elements and newly added ones. An element the morph
keeps keeps its behavior instance. Cleanup runs when an element is removed, its
behavior changes, the registration is removed, or the runtime stops. `start()`
mounts again after `stop()`. DOM moves are reconciled after the mutation batch,
so a move is not a teardown/remount. Setup and cleanup errors identify their
behavior/element and preserve the cause. Unknown names are diagnosed after
initialization; use `lazyBehavior()` to reserve intentional asynchronous
registration. One behavior name is supported per element. See
[diagnostics](diagnostics.md) for tracing and loading behavior.

The task example's behavior only carries application intent: close the
editor after a save the page shows in full, and return focus to the current
Edit button. Opening and closing are native.

## Local UI state

Browser-owned UI state starts with native HTML, which works before and
without the runtime:

- **Dialogs:** a `<button command="show-modal" commandfor="task:1">` opens a
  modal dialog and `command="close"` closes it; Escape, the focus trap, and
  returning focus come with `<dialog>`. Mount a dialog form with
  `mount_dialog`, so its component id is the dialog's id. A page rendered again
  for a rejected native submission renders it `open`.
- **Popovers:** `<button popovertarget="help-1">` and `<div popover id="help-1">`
  for help text and menus.
- **Disclosures:** `<details id="advanced-1">` for optional parts of a form.

Browsers without invoker commands ignore `command` and `commandfor`; there
the runtime runs the built-in commands (`show-modal`, `close`,
`request-close`, and the popover commands) for these buttons, so they work
wherever the runtime does. Custom `--` commands need the browser's own
support.

Open or closed is the person's: a reply or refresh never opens or closes a
dialog, `details`, or popover. To reset one from the server, render it with a
new id.

Buttons whose `commandfor` or `popovertarget` names no element, or an element
of the wrong kind, do nothing in the browser. The runtime reports them once
each as `missing-command-target` or `invalid-command`, with the button and the
id.

Use `behavior()` for what native features do not express: application intent
such as closing a dialog after a successful save, focus decisions after a
reply, or third-party widgets, and read outcomes from `placebo:applied` on
`document`. There is no reactive expression language or client state store;
state that must survive a reload belongs on the server.

## Dialog recipe

Render the same complete contents on initial mount, validation, conflict and
success. Include the heading, form and feedback each time:

```rust
// Initial page: MountedComponent renders inside Maud's html!.
html! {
    button type="button" command="show-modal" commandfor=(component.id()) { "+ Add task" }
    (component.mount_dialog("add-heading", add_contents("", "")))
}
// Handler: Markup contents only. The dialog root is never in this fragment.
binding.invalid(add_contents(&input.title, "Use 3–80 characters."))
binding.reply(add_contents("", "Saved."))
```

`mount()` and `mount_dialog()` return `MountedComponent`, so passing a mount
directly to `reply`, `invalid` or `conflict` fails to compile. Without
JavaScript, a rejected save renders the page again with its component showing
the reply, and a `mount_dialog` root is rendered `open`, so the person sees the
feedback. A component inside a dialog the application renders cannot open that
dialog, so prefer `mount_dialog` for dialog forms.

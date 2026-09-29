## Rules for building with Placebo

These apply to people and coding agents alike.

- **Forms:** derive `FormInput` on the payload struct and write the form with
  `fields!` and typed `Control` values. Render it with
  `ACTION.bind(&component).form(fields)` for mutations or
  `ACTION.bind(region).form(fields)` for reads. Don't write `data-placebo`
  attributes, named inputs for payload fields, or protocol headers by hand.
- **Routes:** register every action with its adapter:
  `.route(ACTION.path(), ACTION.route(handler))`, and wrap the finished router
  with `placebo::native_forms(app)`. The handler takes any Axum extractors
  (state, session) and then `Input<Payload>` last. The same handler answers
  forms submitted before the runtime loads or without JavaScript; don't branch
  on it. A plain Axum route such as `post(save)` skips payload decoding and the
  mutation request check; the browser reports it as `unadapted-route`.
  Unrelated pages, assets, and JSON endpoints are ordinary Axum routes.
- **Search and other reads:** use `ReadAction` with `.on_input(ms)` for live
  server search, and `.on_load()`, `.on_reveal()`, or `.every(ms)` for reads that
  start themselves. Don't rebuild them with `fetch`, `DOMParser`, or manual DOM
  replacement. Add `.history()` to keep the query in the URL, and render the page
  from the same query (`Input<Search>` in the page handler) so reloads and
  bookmarks work. To extend a list, declare it with `.affects(LIST)` and reply
  with `.also_insert(item, Position::End)`.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount. Contents may mount
  other components; each keeps its node and drafts when the outer one refreshes.
- **Drafts:** typed controls keep what the person typed by themselves. A reply
  replaces everything except controls with edits the server has not accepted;
  after a successful save, the submitted controls show the saved values.
  Render the submitted values in `invalid` and the saved record in `conflict`:
  edited fields keep their edits and the others show the current data.
  Use `data-placebo-local` only for controls that must stay together as one
  unit or controls a behavior renders.
- **Validation:** mark a rejected control with `.invalid(true)` and link its
  message with `.described_by(id)`. Put feedback in a `role="status"` (or
  `role="alert"`) element. An invalid reply moves focus to the first invalid
  control, and the status element keeps its node so screen readers announce it.
  Use `.required()` for fields the browser can check before submitting.
- **Dialogs and local UI:** make the dialog the component root with
  `mount_dialog`, or keep it outside the refreshed component, and open and close
  it with `command="show-modal"`/`"close"` and `commandfor`, which work without
  JavaScript. Use `popovertarget` and `details` for other local UI; with an id,
  their open state survives replies. Use `behavior()` for application intent,
  such as closing after a save; listen for `placebo:applied` on `document`.
- **Shared counts and summaries:** use `VersionedRegion`, mount it with
  `region.mount(revision, contents)`, declare it with `.affects(region)`, and
  reply with `.also_replace(region, revision, contents)`. Return the binding
  from one function that both the view's form and the handler's reply use. Increment the revision
  with the data under the same lock or transaction.
- **Lists:** use a `List` when items are added, removed, or reordered. Mount
  each item with `LIST.item(key).mount(contents)`, declare `.affects(LIST)`,
  and reply with `also_insert`, `also_move`, `also_remove`, or `also_order`.
  Items keep their nodes, drafts, and focus. To show a new record in filtered
  search results, declare the results region and reply with
  `.also_refetch(&region)` instead of inserting into it.
- **Other components, pages, and tabs:** refresh another component with
  `.affects(&component)` and `.also_refresh(&component, contents)`. After
  creating or deleting a record, reply with `.navigate("/path")`. To update
  other open pages, publish the same updates on a `Feed` with
  `feed.push()...send()` under the write's lock, and mount it with
  `feed.mount()`. Give a component that other actions or a feed refresh
  `.revision(n)` on every mount and binding, from the record rendered.
- **Verify in a browser:** compiling proves the Rust side agrees. Before calling
  a change done, run the app and exercise the changed flows: valid saves,
  invalid input, independent drafts, conflicts, and any dialog or search. Check
  the browser console: Placebo logs every failure as `[placebo:<code>]` with a
  next step. Fix the cause rather than working around it. When a write may
  have committed but the page could not show it, the component gets
  `data-placebo-stale`; say so with CSS on that attribute. Submitting the form
  again retries safely: the server replays its recorded reply.

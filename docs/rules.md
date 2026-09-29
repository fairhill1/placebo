## Rules for building with Placebo

These apply to people and coding agents alike.

- **Forms:** derive `FormInput` on the payload struct and write the form with
  `fields!` and typed `Control` values. Render it with
  `ACTION.bind(&component).form(fields)` for mutations or
  `ACTION.bind(region).form(fields)` for reads. Don't write `data-placebo`
  attributes, named inputs for payload fields, or protocol headers by hand.
- **Routes:** register every action with `.route(ACTION.path(), ACTION.route(handler))`
  and wrap the finished router with `placebo::native_forms(app)`. The handler
  takes any Axum extractors (state, session), then `Input<Payload>` last, and
  also answers forms sent without JavaScript; don't branch on that. A plain
  route such as `post(save)` skips decoding and the request checks; the browser
  reports it as `unadapted-route`. Other pages and assets are ordinary routes.
- **Reads:** use `ReadAction` with `.on_input(ms)` for live search, and
  `.on_load()`, `.on_reveal()`, or `.every(ms)` for reads that start themselves.
  Don't rebuild them with `fetch` or manual DOM replacement. Add `.history()` to
  keep the query in the URL, and render the page from the same query
  (`Input<Search>` in the page handler) so reloads and bookmarks work.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount. Contents may mount
  other components; each keeps its node and drafts when the outer one refreshes.
- **Drafts:** typed controls keep what the person typed by themselves. A reply
  replaces everything except controls with edits the server has not accepted;
  after a successful save, the submitted controls show the saved values.
  Render the submitted values in `invalid` and the saved record in `conflict`.
  Use `data-placebo-local` only for controls that must stay together as one
  unit or controls a behavior renders.
- **Validation:** mark a rejected control with `.invalid(true)` and link its
  message with `.described_by(id)`. Put feedback in a `role="status"` (or
  `role="alert"`) element; an invalid reply focuses the first invalid control.
  Use `.required()` for fields the browser can check before submitting.
- **Dialogs and local UI:** make a dialog the component root with `mount_dialog`,
  or keep it outside the refreshed component, and open and close it with
  `command`/`commandfor` buttons, which work without JavaScript. Use
  `popovertarget` and `details` for other local UI; with an id, their open state
  survives replies. Use `behavior()` for intent such as closing after a save,
  and listen for `placebo:applied` on `document`.
- **Shared counts and summaries:** use `VersionedRegion`, mount it with
  `region.mount(revision, contents)`, declare it with `.affects(region)`, and
  reply with `.also_replace(region, revision, contents)`. Return the binding
  from one function that the view's form and the handler's reply both use.
  Increment the revision with the data under the same lock or transaction.
- **Lists:** use a `List` when items are added, removed, or reordered. Mount
  each item with `LIST.item(key).mount(contents)`, declare `.affects(LIST)`,
  and reply with `also_insert`, `also_move`, `also_remove`, or `also_order`; a
  read reply may `also_insert` (a "load more" list). To show a new record in
  filtered search results, reply with `.also_refetch(&region)` instead.
- **Other components, pages, and tabs:** refresh another component with
  `.affects(&component)` and `.also_refresh(&component, contents)`. After
  creating or deleting a record, reply with `.navigate("/path")`. To update
  other open pages, publish the same updates on a `Feed` (`feed.push()...send()`
  under the write's lock) that the page mounts with `feed.mount()`. A component
  that other actions or a feed refresh needs `.revision(n)` on every mount and
  binding, from the record rendered.
- **Verify in a browser:** compiling proves the Rust side agrees. Run the app and
  exercise the changed flows: valid saves, invalid input, independent drafts,
  conflicts, and any dialog or search. Placebo logs every failure in the console
  as `[placebo:<code>]` with a next step; fix the cause. A component whose write
  may have committed gets `data-placebo-stale`; say so with CSS, since
  submitting again retries safely.

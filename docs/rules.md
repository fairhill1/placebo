## Rules for building with Placebo

These apply to people and coding agents alike.

- **How the screen updates:** every save answers with the page it came from,
  rendered again, and the browser changes only what differs. A handler checks
  the input, writes, and returns its component's contents. It never lists what
  else on the page shows the data: render every page from current data and it
  follows. Each save renders its page again, so keep pages bounded: paginate
  long lists with a read form.
- **Forms:** derive `FormInput` on the payload struct and write the form with
  `fields!` and typed `Control` values. Render it with
  `ACTION.bind(&component).form(fields)` for saves or
  `Read::new().form(fields)` for searches and filters. Don't write
  `data-placebo` attributes, named inputs for payload fields, or protocol
  headers by hand.
- **Routes:** register every action with `.route(ACTION.path(), ACTION.route(handler))`
  and wrap the finished router with `placebo::native_forms(app)`, which renders
  each reply's page. The handler takes any Axum extractors (state, session),
  then `Input<Payload>` last, and also answers forms sent without JavaScript;
  don't branch on that. A plain route such as `post(save)` skips decoding and
  the request checks; the browser reports it as `unadapted-route`. Other pages
  and assets are ordinary routes. Look up the signed-in user in middleware
  around `native_forms`, which the page render reuses, not in an extractor
  that queries on every request.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount. Mount the
  component on the page its form is on, in every render of that page.
- **Drafts:** a control the person changed keeps its value by itself, on the
  whole page. After a successful save, the submitted controls show the saved
  values. Render the submitted values in `invalid` and the saved record in
  `conflict`. Use `data-placebo-local` only for controls that must stay
  together as one unit or controls a behavior renders.
- **Validation:** mark a rejected control with `.invalid(true)` and link its
  message with `.described_by(id)`. Put feedback in a `role="status"` (or
  `role="alert"`) element; an invalid reply focuses the first invalid control.
  Use `.required()` for fields the browser can check before submitting.
- **Reads:** a search or filter is a read form, `Read::new().on_input(ms).form(fields)`,
  which reads the page it is on with the form's fields as the query and puts
  the query in the address. Render every page from its query (`Input<Q>` in
  the page handler) and build each read form on it from that same `Q`,
  rendering the fields it does not change as hidden controls. "Load more" is
  a read form asking for a longer page (`?shown=40`), with `.on_reveal()` to
  read as it scrolls into view. To poll, render `placebo::refresh_every(ms)`
  while there is something to wait for. Don't rebuild these with `fetch` or
  manual DOM replacement.
- **Dialogs and local UI:** make a dialog the component root with `mount_dialog`,
  and open and close it with `command`/`commandfor` buttons, which work without
  JavaScript. Use `popovertarget` and `details` for other local UI; their open
  state is the person's and survives replies. Use `behavior()` for intent such
  as closing after a save, and listen for `placebo:applied` on `document`.
- **Other pages and tabs:** link pages with plain `a href`; a click shows the
  next page without a document load, and Back and Forward work. Put
  `data-placebo-reload` on a link that must load its page as usual. After
  creating or deleting a record, reply with `.navigate("/path")`. To update
  other open pages, call `feed.changed()` after the write, on a `Feed` the
  pages mount with `feed.mount()`; each page reads itself again.
- **Styles:** pages compose the kit's classes, which its README lists. When the
  design needs a component the kit lacks (a list row, a page header), write it
  in the app's own `static/components.css`, from the kit's tokens, and
  use it everywhere that pattern appears: don't approximate it out of layout
  primitives. Spacing, type, colour, radii, and timing come only from the
  tokens; a value the scale lacks is a new token, after the person approves
  it. When a layout primitive needs other spacing, set its custom property to
  a token on the element, such as `style="--stack-space: var(--space-xs)"`;
  write no other inline styles and no `<style>` elements. The styles test
  (`placebo::styles::Check`) fails on what strays, and the console reports a
  class no stylesheet defines as `[placebo:unknown-class]`; fix the cause.
  Judge screenshots on how the page looks, not only on whether it works.
- **Verify in a browser:** compiling proves the Rust side agrees. Run the app and
  exercise the changed flows: valid saves, invalid input, independent drafts,
  conflicts, and any dialog or search. Placebo logs every failure in the console
  as `[placebo:<code>]` with a next step; fix the cause. A component whose write
  may have committed gets `data-placebo-stale`; say so with CSS, since
  submitting again retries safely.

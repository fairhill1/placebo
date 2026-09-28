## Rules for building with Placebo

These apply to people and coding agents alike.

- **Forms:** derive `FormInput` on the payload struct and write the form with
  `fields!` and typed `Control` values. Render it with
  `ACTION.bind(&component).form(fields)` for mutations or
  `ACTION.bind(region).form(fields)` for reads. Don't write `data-placebo`
  attributes, named inputs for payload fields, or protocol headers by hand.
- **Routes:** register every action with its adapter:
  `.route(ACTION.path(), ACTION.route(handler))`. The handler takes any Axum
  extractors (state, session) and then `Input<Payload>` last. A plain Axum route such as
  `post(save)` skips payload decoding and the mutation request check; the
  browser reports it as `unadapted-route`. Unrelated pages, assets, and JSON
  endpoints are ordinary Axum routes.
- **Search:** use `ReadAction` with `.on_input(ms)` for live server search. Don't
  rebuild it with `fetch`, `DOMParser`, or manual DOM replacement.
- **Components:** use `component.mount(contents)` only when adding a component to
  the page. `reply`, `invalid`, and `conflict` take the complete contents,
  including the form and its feedback, never another mount.
- **Drafts:** wrap user-editable controls in `data-placebo-local="draft"`. Keep
  record IDs, versions, and feedback outside it so every reply refreshes them.
  Add `.reset_local("draft")` to a successful reply to show normalized values.
- **Dialogs:** make the dialog the component root with `mount_dialog`, or keep
  it outside the refreshed component. Listen for `placebo:applied` on `document`.
- **Shared counts and summaries:** use `VersionedRegion`, mount it with
  `region.mount(revision, contents)`, declare it with `.affects(region)`, and
  reply with `.also_replace(region, revision, contents)`. Return the binding
  from one function that both the view's form and the handler's reply use. Increment the revision
  with the data under the same lock or transaction. Use a plain `Region` for
  read results and `.also_append(...)` collections.
- **Verify in a browser:** compiling proves the Rust side agrees. Before calling
  a change done, run the app and exercise the changed flows: valid saves,
  invalid input, independent drafts, conflicts, and any dialog or search. Check
  the browser console: Placebo logs every failure as `[placebo:<code>]` with a
  next step. Fix the cause rather than working around it.

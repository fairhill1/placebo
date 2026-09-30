# Debugging Placebo interactions

Unexpected runtime failures call `console.error` without enabling tracing.
The message includes a stable code, the action/target or behavior/element,
available request context, the consequence, and a suggested next check. The
structured context and original error object are separate console arguments.

For example, a form whose component is missing from the page produces a
message of this form:

```text
[placebo:missing-target] request=<id> action=save-task target=task:1
relatedTarget=task:1 POST /actions/save-task: Component 'task:1' is not mounted.
Request: not-started; update: not-applied; write: not-started.
Next: Mount the component before submitting its form, and check that the binding and mounted id agree.
```

The line is wrapped here for readability. IDs and paths describe the failing
interaction; input values and response bodies are not included.

## Console tracing

Enable ordinary execution logs in the browser console:

```js
(await import('/placebo.js')).trace(true)
```

Disable them with `trace(false)`. To trace startup and survive a reload:

```js
localStorage.setItem('placebo:trace', 'true')
location.reload()
```

Remove that storage key to disable tracing on future reloads. `trace()` changes
the current runtime only. Tracing reports scheduling, dispatch, application,
deferral, ignored duplicate submits, discarded requests, and behavior lifetime.
Expand the context object for details. Applied updates include the requested
local resets that were skipped because the user edited again, components left
alone while busy, and whether the whole page applied (`page`).

Ordinary validation, conflicts, obsolete reads, and pages older than the one
shown are not console errors. They remain explicit outcomes in the trace. Discarding a
dispatched mutation emits a warning even with tracing off, because aborting
the browser request cannot undo a server write.

## Request correlation and consequences

Each interaction attempt has a `requestId`; dispatched requests carry it in
`X-Placebo-Request-Id`. The Rust `action.route(handler)` adapters echo valid
IDs on responses, including extractor errors. Debug builds log request start
and completion with the action, ID, method, path, status, and elapsed time.
Production builds retain header correlation but omit this automatic stderr
logging; production applications can integrate the header with their logging.
IDs are bounded and validated, and have no authorization meaning.

Use the same ID to find the request in browser Network tools and server logs.
A preflight failure has an ID but no corresponding dispatched request.

The console context separates three facts:

| Field | Meaning |
| --- | --- |
| `requestState` | `not-started`, `started`, or `response-received`; started means fetch was invoked, not proof of server receipt. |
| `updateState` | `not-applied`, `applied`, or `possibly-partial` if an exception interrupted a DOM commit. |
| `writeState` | `not-applicable` for reads, `not-started`, `unknown`, or the valid applied response's `acknowledged`/`rejected` outcome. |

A lost or rejected mutation response leaves the write outcome unknown. The
application may have committed even when the UI did not update, so the component
gets `data-placebo-stale`. Submitting its form again retries safely: the runtime
resends the same request with its idempotency key, and the server replays the
reply it recorded instead of writing again, or runs the write for the first time
if the first attempt never arrived. The retry is traced as `scheduled` with
`retry` and `retryOf`, and `applied` reports `replayed`. `replay-pending` means
the server has the first attempt but not its result (it is still running, or
stopped); wait and submit again, or reload. `replay-unknown` means the first
attempt is older than the server's replay store remembers, so it may have
written; reload to see current data. See [idempotent retries](protocol.md#idempotent-retries).

A redirect is reported as `redirected`, not as a network failure. The usual
cause is authentication middleware sending an expired session to a login page,
which happens before the handler runs. The browser does not expose the redirect's
status or location, so `status` is null and the write outcome stays unknown.

Method/path context excludes query strings and fragments. The framework does
not log form bodies, response bodies, or credentials. JSON parser messages can
contain body snippets, so malformed JSON gets a sanitized error. Network and
behavior errors preserve their original error objects/stacks, including causes;
application-authored error messages remain the application's responsibility.

## Forms submitted without JavaScript

A native submission has no browser runtime to report problems, so the server
logs them, in release builds too: the person gets a lesser page, and nothing
else would show it. `[placebo:native-page]` means a rejected reply got a page
with only its component: the router is not wrapped with `native_forms`, or the
page the form was on did not mount that component when rendered again.
`[placebo:native-no-referer]` means the browser sent no same-origin `Referer`, so
a successful save returned to `/`; keep the default `Referrer-Policy` or reply
with `.navigate(path)`. The person still sees their values and the feedback in
both cases.

## Replies without their page

A reply is its page, rendered again. When that fails, the reply shows in its
component alone and the rest of the page stays as it was:

- `[placebo:page-missing]` (a warning, logged on the server too): the page
  answered an error, usually because the reply removed what it shows, such as a
  deleted record. A successful reply can send the person on with
  `.navigate(path)`.
- `[placebo:page-error]`: the router is not wrapped with
  `placebo::native_forms(app)`, or the request did not say which page it came
  from.
- `[placebo:unmounted-target]`: the page rendered, but does not mount the
  replying component, so a rejected reply's feedback could not be shown. Mount
  the component on the page its form is on, in every render of that page.

## Buttons that do nothing

A `commandfor` or `popovertarget` button whose target is missing, or a command
aimed at the wrong kind of element, is inert in the browser without any
message. After the page loads, and after each change to the DOM, the runtime
reports each such button once as `[placebo:missing-command-target]` or
`[placebo:invalid-command]`, naming the button, the id, and the command.

## Styles outside the kit

After the page and its stylesheets load, and after each change to the DOM,
the runtime reports each class that no stylesheet on the page defines, once,
as `[placebo:unknown-class]`, naming the element. It is usually a guess at the
kit's names; a class that only hooks a script or a test should be a data
attribute. When a stylesheet from another site hides its rules, the check is
skipped.

The styles test, `placebo::styles::Check`, reports the rest in `cargo test`:
`[placebo:kit-changed]` for a vendored kit that differs from Placebo's,
`[placebo:unlayered]` for a rule outside the kit's layers,
`[placebo:important]`, `[placebo:raw-value]` for a value that should be a
token, and `[placebo:style-element]` and `[placebo:inline-style]` in the
views. See [styles](../README.md#styles-stay-in-the-kit).

## Live updates

A feed's connection is traced as `placebo:push` with `push-connected`,
`push-reconnected`, `push-changed`, or `push-own` (a signal this page's own save
caused, which needs no read). A dropped connection logs one
`[placebo:push-disconnected]` warning; the browser reconnects by itself, and a
page that missed a change reads itself again. A stream the browser gives up on
(a missing route, an error status, another content type) logs
`[placebo:push-closed]`: changes from then on do not reach the page. A refresh
whose page could not be read logs `network-error`, `http-error`, or
`redirected` with phase `push-refresh`. An applied refresh emits
`placebo:applied` with `source: "push"`; a page older than the one shown is
discarded with reason `older-page`.

## Files too large

A chosen file over its field's `Upload<MAX_BYTES>` is a person's mistake, not a
failure: the runtime shows the browser's validation message on the file input,
sends nothing, and traces `ignored` with reason `upload-too-large`, the field,
the limit, and the size. If the server still refuses a file (a bypassed check,
or a native submission), the runtime logs `[placebo:upload-too-large]` with the
limit and `writeState: not-started`; debug servers log the field and limit.

## Missing and asynchronous behaviors

Register ordinary behaviors synchronously with `behavior(name, setup)`. Unknown
names are diagnosed after initial document/module setup, or after the next
task for dynamic mounts. The check allows same-turn registration and reports
each unresolved element/name once. Correcting the name or registering it later
allows the element to mount normally.

Reserve intentional asynchronous loading before inserting its mount:

```js
import { lazyBehavior } from '/placebo.js';

const unregister = lazyBehavior(
  'chart',
  () => import('/chart.js').then(module => module.setup),
  { timeoutMs: 10000 },
);
```

The loader must resolve to the behavior's setup function. Loading is traceable;
rejection, an invalid result, or timeout produces an error. The default timeout
is 10 seconds, configurable from 1 to 60000 ms. After failure, unregister and
register again to retry. Unregistering ignores late results and clears the
timer; it does not cancel JavaScript's module import. Registrations and pending
loads survive runtime `stop()`/`start()`; element setups and cleanups follow the
runtime's mount lifetime.

Setup and cleanup errors identify the behavior name and an element descriptor,
with component ownership when available. Synchronous setup must return cleanup
or undefined. Errors in arbitrary application event callbacks still follow the
browser's normal exception reporting; Placebo does not wrap all user code.

## What the tests check

`tests/diagnostics.test.mjs` breaks isolated task-list instances and reads the
browser console. The question for each case: can an author identify the failed
interaction, understand its consequence, and find a sensible next check without
adding application logging?

| Injected failure | Console output |
| --- | --- |
| Unknown behavior name | Names the behavior and element, explains inactivity, suggests registration/import checks. |
| Missing component | Names the action, target and path; says the request never started. |
| Remounted component | Identifies the affected component, old ownership, and possible committed write. |
| HTTP 500 | Request ID, method/path, status, update/write state, and a server-log correlation hint. |
| JSON instead of a page | Expected and received content types; suggests login/error-page or extractor rejection checks. |
| Wrong protocol version | Actual and expected versions with a rebuild/reload hint. |
| Response lost after a real commit | Reports uncertainty and preserves the cause; the request ID matches the server's completed request. Submitting again replays the recorded reply. |
| Behavior setup/cleanup exception | Names the behavior and element, preserves the original stack, and suggests the lifecycle check. |

Other tests cover replies without an outcome, interrupted body streams, invalid form
configuration, mutation unmount warnings, lazy loading, tracing controls,
input/query omission, and behaviors registered by a later module script. The
captured console output is written to `test-results/diagnostics-after.json`.

This establishes diagnostic behavior for the covered cases on Chromium/macOS.
It is not a usability study or a comparison result against HTMX, Datastar, or a
full-stack SSR framework. A server-side exception can still require server logs;
correlation helps locate it but cannot reveal facts the browser never received.

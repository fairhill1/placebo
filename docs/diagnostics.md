# Debugging Placebo interactions

Unexpected runtime failures call `console.error` without enabling tracing.
The message includes a stable code, the action/target or behavior/element,
available request context, the consequence, and a suggested next check. The
structured context and original error object are separate console arguments.

For example, a missing shared summary produces a message of this form:

```text
[placebo:missing-target] request=<id> action=save-task target=task:1
relatedTarget=task-count POST /actions/save-task: Region 'task-count' is not mounted.
Request: not-started; update: not-applied; write: not-started.
Next: Mount the named region before submitting, and check that the binding and mounted id agree.
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
local resets that were skipped because the user edited again, and the revisions
of shared snapshots skipped because they were not newer.

Ordinary validation, conflicts, obsolete reads, and stale snapshots are not
console errors. They remain explicit outcomes in the trace. Discarding a
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
application may have committed even when the UI did not update. Read current
server state before retrying; neither an HTTP status nor a diagnostic implements
transaction rollback or idempotent retries.

Method/path context excludes query strings and fragments. The framework does
not log form bodies, response bodies, or credentials. JSON parser messages can
contain body snippets, so malformed JSON gets a sanitized error. Network and
behavior errors preserve their original error objects/stacks, including causes;
application-authored error messages remain the application's responsibility.

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

## Evaluation: 2026-09-11

We deliberately broke isolated task-list instances and captured actual browser
console output before changing the runtime. The acceptance question was:
can an author identify the failed interaction, understand its consequence, and
find a sensible next check without adding application logging?

| Injected failure | Before | After |
| --- | --- | --- |
| Unknown behavior name | No console message; element stayed inactive. | Names the behavior and element, explains inactivity, suggests registration/import checks. |
| Missing region | Named the missing region but omitted request/consequence context. | Names the action, primary/affected targets and path; explicitly says the request never started. |
| Remounted response destination | Generic additional-region mismatch. | Identifies the affected region, old ownership, and possible committed write. |
| HTTP 500 | Status and action/target only. | Adds request ID, method/path, status, update/write state, and server-log correlation hint. |
| HTML instead of an update | Said the response type was wrong. | Shows expected and received content types; suggests login/error-page or extractor rejection checks. |
| Wrong protocol version | Said the response was incompatible. | Shows actual and expected versions with a rebuild/reload hint. |
| Response lost after a real commit | `Failed to fetch`; no statement about the write. | Reports uncertainty and preserves the cause; request ID matches the server's completed request. |
| Behavior setup/cleanup exception | Error message with null action/target. | Names the behavior and element, preserves the original stack, and suggests the relevant lifecycle check. |

Additional tests cover malformed JSON, interrupted body streams, invalid form
configuration, mutation unmount warnings, lazy loading, tracing controls, and
input/query omission. The tests inspect console output directly. Captures are
in `test-results/diagnostics-before.json` and `test-results/diagnostics-after.json`;
the latter is regenerated by `tests/diagnostics.test.mjs`.

This establishes diagnostic behavior for the covered cases on Chromium/macOS.
It is not a usability study or a comparison result against HTMX, Datastar, or a
full-stack SSR framework. A server-side exception can still require server logs;
correlation helps locate it but cannot reveal facts the browser never received.

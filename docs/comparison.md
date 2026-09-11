# Corrected Issue Desk baseline — 2026-09-11

A subsequent six-agent blind rerun produced Placebo scores of **8/14, 4/14,
14/14**, and Datastar scores of **5/14, 0/14, 6/14**. Each attempt used a fresh
gpt-5.6-luna at medium reasoning, the same complete requirements, a 20-minute
allowance, and no coordinator coaching or repair. These first-submission results
measure a different question from the corrected capability baseline below.

All three Placebo agents used typed mutations, but none invoked `placebo new`,
`check`, `dev`, or generated Cargo aliases. The 14/14 app implemented search with
custom fetch/DOM code; the coordinator's later `placebo check` correctly rejected
its untyped read route. Two other Placebo apps passed source checking while
failing browser scenarios. Consequently there was one complete behavioral pass,
but no complete pass through the intended authoring workflow. Datastar's 0/14
submission did not wire its assigned library into interactions; it counts against
unaided LLM success, not against the capability of a correct Datastar app.

Placebo performed better in this small trial, while workflow adoption and actual
browser verification remained weak. Some failures cascade from one integration
error or missing required markup. Two Datastar agents stopped after sandboxed
Chromium failures without trying the documented escalation; the coordinator could
evaluate all six. There is no timing winner for incomplete apps or precise success
rate claim from three attempts per stack. Detailed scores, timing intervals,
source audits, environment caveats and immutable submissions are in the companion
workspace `../placebo-blind-reruns-2026-09-11/REPORT.md`.

## Workflow follow-up

The next change introduced `VersionedRegion` for shared replacement snapshots,
requiring revisions at mount time and rejecting plain regions in `also_replace`.
The generated starter now includes a contract test that runs the source check
during ordinary `cargo test`. The main README has one generated quickstart;
manual integration moved to a separate reference. Compiler regressions, a real
generated-app Cargo-test regression, and the browser/dev suite passed.

A single fresh Luna-medium follow-up with the same complete specification scored
**3/14**. It used the new snapshot type, but manually created its app and ran none
of `new`, `check`, `dev`, or `cargo test`. The source checker flagged its raw read
route. Duplicate count IDs and an unmounted composer caused runtime preflight
errors and blocked writes; the agent's browser scripts also used absent selectors.
The passing diagnostics check did not prove an HTTP 500 occurred, exposing a
limit in that evaluator assertion. No submission or evaluator was repaired.

The compiler and generated-test protections work for the cases they cover, but
this trial does not show improved unaided workflow adoption. It is one fresh
post-change attempt, not another comparative cohort. Details and preserved
evidence are in `../placebo-workflow-eval-2026-09-11/REPORT.md`.

## Corrected capability baseline

Both the corrected typed Placebo app and the corrected Datastar app pass the
same 14 browser scenarios. The earlier Datastar agent scores were evidence of
a broken implementation, not a demonstrated limitation of Datastar.

This is a coordinator repair with access to the specification, existing code,
and evaluator. It is not another blind weaker-model trial, a timed authoring
comparison, or proof that either framework has better overall DX. Original
submissions and results remain unchanged. HTMX + Alpine was not rerun here.

## What the apps actually use

| Concern | Placebo | Datastar 1.0.3 |
| --- | --- | --- |
| Requests | Typed read/mutation action adapters and generated forms | `@get`/`@post` with explicit `contentType: 'form'` and Axum extractors |
| Updates | Component refreshes, revisioned count patches, appends | Public SSE element and signal patches; morph summaries/versions/feedback, append new cards |
| Drafts | Local subtree retention and guarded resets | Bound local signals; application edit generations guard server acknowledgements |
| Pending saves | Exclusive per-component policy by default | Per-form indicator blocks duplicate submissions; cancellation disabled for independent writes |
| Dialog | Persistent native shell, application open/close behavior | Persistent native shell, Datastar event/effect expressions |
| Failures | Default correlated contract diagnostics | Datastar fetch lifecycle hook logs method/path/status; no form bodies |

Datastar owns the network requests and DOM patches. There is no application
`fetch`, `DOMParser`, replacement engine, or second frontend framework. The app
uses a small Rust helper to encode the documented SSE format rather than an
SDK; it includes a test preventing user-controlled line endings from injecting
SSE fields. Validation and conflicts are successful SSE transports carrying an
explicit failed acknowledgement and visible feedback, without changing data.

The tested scenarios cover SSR/native search, normalization, validation,
independent drafts, typing during a save, duplicates, concurrent saves, stale
search, conflict/retry, dialog/create/append, escaped text/mobile layout, HTTP
failure diagnostics, and priority edits/counts.

## What changed in Placebo

`Component::mount` now returns `MountedComponent`, which can render in Maud but
cannot be passed directly as reply contents. `mount_dialog` makes the native
dialog itself the persistent component root. The alternative of mounting a
form component inside an ordinary dialog remains supported.

The browser rejects a dialog nested in replaceable component contents with
`unstable-dialog`, before sending a mutation or applying a malformed response.
The task example exercises the new mount. Regression tests cover native node
identity, validation, success, reopening/Escape, and rejection before any write
or any patch application. Rust doctests cover the mounted-wrapper type error.

The repaired comparison app uses the existing dialog-outside-component pattern
and typed adapters for all three actions. Preserving its dialog exposed another
application mistake: its success listener was on a container, while lifecycle
events are dispatched on `document`. That was fixed and documented. Incorrect
listener placement, omitted ordinary markup, and arbitrary valid-but-wrong UI
still require browser flow tests. The new source checker is not a proof of UI
correctness.

## Interpretation and remaining gaps

This establishes behavioral parity on this task. Placebo's potential advantage
is how much interaction policy and Rust form/handler agreement it provides by
default. Datastar can implement these flows with its supported primitives, and
already provides a broader reactive/morphing model. This result does not prove
that maintaining a separate Placebo browser runtime is necessary.

The 14 checks do not cover reversed delivery of already-committed shared count
snapshots, IME composition during writes, remounted destinations, navigation,
streaming sessions, authentication, database persistence, or other browsers.
Placebo has separate runtime tests for several of those adverse update cases;
they were not run as a matched Datastar comparison. The Datastar app does not
add a general revision guard for reordered count snapshots or a general IME
response-deferral policy. No code-size, performance, cost, or completion-speed
winner is claimed.

Detailed apps, the unchanged evaluator, results, and a frozen Placebo source
snapshot live in the companion workspace directory
`../placebo-baseline-2026-09-11/` relative to the repository root. See its
`REPORT.md` and `results/validated-baseline/{placebo,datastar}/checks.json`.

Public Datastar references used:
[actions](https://data-star.dev/reference/actions),
[attributes](https://data-star.dev/reference/attributes), and
[SSE events](https://data-star.dev/reference/sse_events).

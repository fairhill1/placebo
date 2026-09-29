# What Placebo is trying to become

Placebo aims to make Rust SSR feel like one coherent way to build a web
application: straightforward to author, predictable to change, and easy to
debug. Rust and Axum are the foundation. The server and browser halves should
agree about actions, updates, local state, and failures.

Evaluate it against building the same application with Maud + HTMX + Alpine,
with Datastar, and with more comprehensive SSR/full-stack frameworks.
The measure of success is the complete development experience: how much an
author must write, understand, coordinate, and debug to get reliable behavior.

The ambition is a better overall experience for Rust/Axum SSR: easier and nicer
to use, with demonstrable advantages in correctness, diagnostics, maintenance,
and resource cost. Those advantages must be established through comparison.

This document records the project's design criteria. It describes the intended
destination; the [README](README.md) describes the prototype's current behavior.

## Simple authoring

Keep UI structure and interaction intent easy to read together. Minimize
repeated names, glue code, and coordination between independently configured
Rust, HTML, and JavaScript. Make common changes local and understandable.

Judge an abstraction by the application code it enables, including validation,
errors, and cleanup. Compiler guarantees should earn their complexity. A smaller
API with understandable rules is preferable to accumulating special cases.

## Clear server and browser ownership

Make it explicit which state the server owns and which state the browser keeps.
An update should have predictable effects on drafts, focus, selection, local
behaviors, and component lifetime. Resetting local state should be deliberate.

Request ordering and component identity belong in the framework contract.
Authors should be able to reason about slow responses, concurrent saves,
remounts, and shared summaries without inventing coordination for each screen.
Browser scheduling and persistence guarantees must remain clearly distinguished.

## Failures must be explainable

Silent failure is a design defect. If an interaction does nothing, the developer
should be able to find out why without modifying the framework or installing a
custom event listener first.

- Catch mistakes at compile time wherever a useful static guarantee is possible:
  mismatched fields, payloads, handlers, and invalid declarations.
- Check the facts that only exist at runtime: mounted targets, actual request
  data, response shape, versions, and browser lifecycle. Unsupported operations
  must produce an actionable diagnostic.
- Report unexpected browser failures in the browser console by default, with a
  stable diagnostic code, a concrete explanation, and relevant context. Include
  the action, target or behavior, operation, request method/path and HTTP status
  when available. Preserve the underlying cause or stack when available.
- Explain the consequence: whether the request was sent, whether an update was
  applied or rejected, and whether a write's outcome is uncertain. A lost response
  cannot establish whether a server write committed.
- Give a useful next step when the cause is known. A missing target should name
  the target and explain the mounting requirement; a protocol mismatch should
  identify the conflicting versions. For server failures, make it possible to
  correlate the browser request with server logs instead of guessing a cause.
- Make ordinary execution traceable from the browser console through an easy
  development trace mode: scheduling, sending, applying, ignoring, deferring,
  and discarding work, with reasons. Expected cancellation or an obsolete
  snapshot should be distinguishable from an error. Routine tracing can be
  optional; discovering a failure must not depend on having enabled it earlier.

Diagnostics should be useful to both a person and a coding agent. Structured
events and readable console messages serve complementary purposes. Context
should describe the interaction without dumping form contents, credentials,
or private server internals into logs by default.

Validation and conflicts are explicit application outcomes. Applications need
a clear way to show those outcomes to users; developer console output serves
the separate purpose of explaining what the framework did.

## A complete development loop

Rebuild Rust, reload changed static assets, recover from compile errors, and
explain failures through a straightforward development command. Keep the
production runtime free of development reload/watch behavior. Ordinary Cargo
and Axum workflows should remain usable.

## Good for humans and LLM-driven coding

Prefer readable structure, consistent conventions, local reasoning, and
actionable diagnostics. Make the intended implementation easy to discover and
verify. Useful compiler checks reduce mistakes, but generated machinery should
not make everyday edits or error messages difficult to understand.

Exercise these qualities on complete interactions and realistic changes:
renaming a field, adding an editor, handling validation, updating several
regions, and diagnosing a request that did not update the page.

## Keep implementation choices open

Maud is the current renderer. The form syntax, macros, browser behavior API,
and fragment protocol are experiments that must earn their place against these
goals. Choosing a renderer or introducing custom syntax requires a concrete
improvement in authoring, correctness, diagnostics, or development workflow.

HTML fragments remain a useful rendering mechanism. Their application needs
clear identity, ownership, and ordering rules. Native browser capabilities
should inform the design, as the dialog example already demonstrates.

## Comparison criteria

Datastar is a direct comparison because it combines backend interactions and
frontend reactivity in one HTML-oriented framework. Compare Placebo against
that integrated experience as well as the Maud/HTMX/Alpine combination.
See [Datastar's getting-started guide](https://data-star.dev/guide/getting_started).

Also evaluate more comprehensive SSR approaches. Leptos is one Rust candidate;
its [server and hydration model](https://book.leptos.dev/server/28_async_quick_reference.html)
offers another basis for examining the authoring experience. A broader framework
may remove work that a smaller library leaves to its users. Treat "heavier" as
something to measure, not a verdict on usability.

Use equivalent application behavior and realistic edits to compare setup,
authoring and refactoring effort, failure visibility, local-state handling,
build feedback, deployment work, browser payload/runtime cost, and server
resource use. Include the glue and maintenance code needed outside each
framework. Record tradeoffs and where another approach works better. Low code
count or bundle size alone does not establish a better developer experience.

## How we judge the next change

For a proposed feature or abstraction, establish:

1. The concrete application task it makes easier and the code it simplifies.
2. The ownership and lifecycle rules an author needs to understand.
3. Which mistakes the compiler catches and how remaining failures become visible.
4. What happens during slow, out-of-order, malformed, or failed requests.
5. Whether the resulting application is easier for a person or LLM to change.

Use the examples as evidence, and revisit the design when a feature makes
individual mechanisms stronger while making the overall experience harder.

## Current evidence and gaps

The prototype has typed forms and handlers, guarded fragment updates, explicit
local retention/reset, shared snapshot revisions, behavior lifecycle hooks,
and a rebuild/reload workflow. Tests exercise both normal and adverse cases.

The [debugging evaluation](docs/diagnostics.md) established console diagnostics
for missing/remounted targets, HTTP and protocol failures, lost mutation
responses, and behavior failures. There is now a console trace mode, request
correlation with debug server logs, repair hints, preserved causes where safe,
and explicit asynchronous behavior registration with loading-failure diagnostics.

That is evidence for covered cases, not a completed diagnostic or usability
goal. Broader application failures, production logging integration, and actual
debugging effort still need evaluation. No comparative developer-experience
result against Datastar or the other comparison stacks has been established.

The broader authoring model is still being evaluated. Forms now work without
JavaScript through the same handlers, retries after uncertain writes are
idempotent, components can nest, component revisions order replies against
server push, and reads can start themselves. Each has tests for its adverse
cases, but none has been compared with the other stacks yet. The examples do not
yet establish that Placebo makes a complete production application simpler than
the comparison stack.

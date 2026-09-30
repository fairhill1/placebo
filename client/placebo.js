// Protocol v6. HTML is trusted server-rendered content, not a sanitizer input.
// Scheduling belongs to the actual mounted target node, not its reusable id.
// A mutation is answered with its whole page; a read form reads its page at
// the form's query, and a feed's signal or a poll reads it again. Idiomorph
// (vendored above this file in /placebo.js) morphs each into the document.
const VERSION = 6;
const KEY_FIELD = "placebo-key";
const pending = new Map();
// Per component: a mutation sent whose outcome is unknown. Submitting the
// same form again resends it unchanged, with its idempotency key.
let uncertain = new WeakMap();
// Per component: a pushed refresh waiting for its own request to finish.
let composing = new WeakSet();
let started = false;
let observer;
let edits = new WeakMap();
// Parsed configuration per form, reused while its attribute is unchanged.
const configs = new WeakMap();
const behaviors = new Map();
const mountedBehaviors = new Map();
let unknownBehaviors = new WeakMap();
let behaviorAudit;
let tracing = false;
try { tracing = localStorage.getItem("placebo:trace") === "true"; } catch { /* Storage may be unavailable. */ }

// In the browser console: (await import('/placebo.js')).trace(true)
// Or set localStorage['placebo:trace'] = 'true' before reloading.
export function trace(enabled = true) { tracing = Boolean(enabled); }

// Setup runs once per actual element. Moving retained local nodes does not
// remount them. Cleanup runs on removal, name change, unregister, or stop().
export function behavior(name, setup) {
  require(typeof name === "string" && name.length > 0 && typeof setup === "function" && !behaviors.has(name),
    "invalid-behavior", "Register a unique behavior name and setup function.");
  const registration = { setup };
  behaviors.set(name, registration);
  if (started) syncBehaviors();
  return () => unregisterBehavior(name, registration);
}

// Reserve a name before starting an asynchronous import, so an intentional
// loading state is distinct from a misspelled or forgotten registration.
export function lazyBehavior(name, load, { timeoutMs = 10000 } = {}) {
  require(typeof name === "string" && name.length > 0 && typeof load === "function" && !behaviors.has(name) &&
    Number.isInteger(timeoutMs) && timeoutMs > 0 && timeoutMs <= 60000,
    "invalid-behavior", "Register a unique behavior name, loader, and timeout between 1 and 60000 ms.");
  const registration = { setup: null };
  behaviors.set(name, registration);
  const failed = error => {
    if (behaviors.get(name) !== registration || registration.failed) return;
    registration.failed = true;
    clearTimeout(registration.timer);
    report(null, error, "behavior-load", { behavior: name, phase: "behavior-load", hint: "Check the behavior module/import and loader. Unregister and register again to retry." });
  };
  registration.timer = setTimeout(() => failed(new ProtocolError("behavior-timeout",
    `Behavior '${name}' did not load within ${timeoutMs} ms.`)), timeoutMs);
  Promise.resolve().then(load).then(setup => {
    if (behaviors.get(name) !== registration || registration.failed) return;
    require(typeof setup === "function", "invalid-behavior", "A lazy behavior loader must return its setup function.");
    clearTimeout(registration.timer);
    registration.setup = setup;
    if (started) syncBehaviors();
  }).catch(failed);
  emit("behavior", null, { behavior: name, phase: "behavior-loading" });
  return () => unregisterBehavior(name, registration);
}

function unregisterBehavior(name, registration) {
  if (behaviors.get(name) !== registration) return;
  clearTimeout(registration.timer);
  behaviors.delete(name);
  syncBehaviors();
}

function elementName(element) {
  return `${element.localName}${element.id ? `#${element.id}` : ""}`;
}

function behaviorContext(element, name, phase) {
  return { behavior: name, element: elementName(element), phase,
    target: element.closest("[data-placebo-component]")?.id ?? element.dataset.owner ?? null };
}

// Deferred and module scripts run after parsing ends but before
// DOMContentLoaded, so "interactive" alone does not mean every script that
// registers behaviors has run yet.
let contentLoaded = document.readyState === "complete" ||
  (document.readyState === "interactive" && performance.getEntriesByType?.("navigation")?.[0]?.domContentLoadedEventStart > 0);
function onContentLoaded() {
  contentLoaded = true;
  auditBehaviors();
}

// A command or popover button whose target is missing, or of the wrong kind,
// does nothing in the browser and says nothing. Report it once per button.
const DIALOG_COMMANDS = ["show-modal", "close", "request-close"];
const POPOVER_COMMANDS = ["show-popover", "hide-popover", "toggle-popover"];
let unresolvedCommands = new WeakMap();
function auditCommands() {
  for (const button of document.querySelectorAll("[commandfor],[popovertarget]")) {
    const attribute = button.hasAttribute("commandfor") ? "commandfor" : "popovertarget";
    const id = button.getAttribute(attribute);
    const command = attribute === "commandfor" ? button.getAttribute("command") ?? "" : "toggle-popover";
    const target = id ? document.getElementById(id) : null;
    const problem = !target ? `${attribute}="${id}" names no element on this page`
      : DIALOG_COMMANDS.includes(command) && target.localName !== "dialog" ? `command "${command}" needs a dialog, but '${id}' is a <${target.localName}>`
      : POPOVER_COMMANDS.includes(command) && !target.hasAttribute("popover") ? `command "${command}" needs an element with popover, but '${id}' has none`
      : !command.startsWith("--") && ![...DIALOG_COMMANDS, ...POPOVER_COMMANDS].includes(command) ? `command "${command}" is not a built-in command; custom commands start with "--"`
      : null;
    const key = `${attribute}:${id}:${command}`;
    if (!problem) { unresolvedCommands.delete(button); continue; }
    if (unresolvedCommands.get(button) === key) continue;
    unresolvedCommands.set(button, key);
    report(null, new ProtocolError(problem.startsWith(attribute) ? "missing-command-target" : "invalid-command",
      `This button does nothing: ${problem}.`), "invalid-command", { element: elementName(button), relatedTarget: id });
  }
}

// A class no stylesheet defines styles nothing: usually a guess at the kit's
// names, or a hook for a script or test. Report each class once.
let reportedClasses = new Set();
function auditClasses() {
  // The load event waits for every stylesheet, including imported ones.
  if (document.readyState !== "complete") return;
  const defined = new Set();
  const read = rules => {
    for (const rule of rules) {
      for (const [, name] of rule.selectorText?.matchAll(/\.(-?[_a-zA-Z][\w-]*)/g) ?? []) defined.add(name);
      if (rule.cssRules) read(rule.cssRules);
      if (rule.styleSheet) read(rule.styleSheet.cssRules);
    }
  };
  // A stylesheet from another site hides its rules, and could define any class.
  try { for (const sheet of document.styleSheets) read(sheet.cssRules); } catch { return; }
  for (const element of document.querySelectorAll("[class]")) {
    for (const name of element.classList) {
      if (defined.has(name) || reportedClasses.has(name)) continue;
      reportedClasses.add(name);
      report(null, new ProtocolError("unknown-class", `No stylesheet on this page defines the class '${name}'.`),
        "unknown-class", { element: elementName(element) });
    }
  }
}

// Browsers without invoker commands get the built-in ones from the runtime,
// so the same buttons open and close dialogs and popovers everywhere the
// runtime runs. Custom "--" commands need the browser's CommandEvent.
const nativeCommands = "commandForElement" in HTMLButtonElement.prototype;
function onCommandClick(event) {
  const button = event.target.closest?.("button[commandfor]");
  if (event.defaultPrevented || !button || button.disabled || (button.form && button.type !== "button")) return;
  const target = document.getElementById(button.getAttribute("commandfor"));
  const command = (button.getAttribute("command") ?? "").toLowerCase();
  const value = button.hasAttribute("value") ? button.value : undefined;
  if (!target) return;
  try {
    if (target.localName === "dialog") {
      if (command === "show-modal" && !target.open) target.showModal();
      if (command === "close" && target.open) target.close(value);
      if (command === "request-close" && target.open) typeof target.requestClose === "function" ? target.requestClose(value) : target.close(value);
    }
    if (target.hasAttribute("popover")) {
      const shown = target.matches(":popover-open");
      if (command === "toggle-popover" || (command === "show-popover" && !shown) || (command === "hide-popover" && shown)) target.togglePopover();
    }
  } catch { /* A disconnected target; the audit reports a missing one. */ }
}

function auditBehaviors() {
  clearTimeout(behaviorAudit);
  if (!started || !contentLoaded) return;
  auditCommands();
  auditClasses();
  for (const element of document.querySelectorAll("[data-placebo-behavior]")) {
    const name = element.dataset.placeboBehavior;
    if (behaviors.has(name) || unknownBehaviors.get(element) === name) continue;
    unknownBehaviors.set(element, name);
    report(null, new ProtocolError("unknown-behavior", `Behavior '${name}' is not registered; this element is inactive.`),
      "unknown-behavior", behaviorContext(element, name, "behavior-mount"));
  }
}

function cleanupBehavior(element, entry, work) {
  mountedBehaviors.delete(element);
  try { entry.cleanup?.(); }
  catch (error) { report(work, error, "behavior-cleanup", behaviorContext(element, entry.name, "behavior-cleanup")); }
  emit("behavior", null, behaviorContext(element, entry.name, "behavior-unmounted"));
}

function syncBehaviors(work = null) {
  for (const [element, entry] of mountedBehaviors) {
    if (!started || !element.isConnected || element.dataset.placeboBehavior !== entry.name ||
        behaviors.get(entry.name) !== entry.registration) cleanupBehavior(element, entry, work);
  }
  if (!started) return;
  for (const element of document.querySelectorAll("[data-placebo-behavior]")) {
    if (mountedBehaviors.has(element)) continue;
    const name = element.dataset.placeboBehavior;
    const registration = behaviors.get(name);
    if (registration) unknownBehaviors.delete(element);
    const setup = registration?.setup;
    if (!setup) continue;
    // Record before invoking user code to allow setup to add more behaviors.
    const entry = { name, registration };
    mountedBehaviors.set(element, entry);
    try {
      const cleanup = setup(element);
      require(cleanup === undefined || typeof cleanup === "function", "invalid-behavior", "Setup must return a cleanup function or undefined.");
      entry.cleanup = cleanup;
      emit("behavior", null, behaviorContext(element, name, "behavior-mounted"));
    } catch (error) { report(work, error, "behavior-setup", behaviorContext(element, name, "behavior-setup")); }
  }
  clearTimeout(behaviorAudit);
  behaviorAudit = setTimeout(auditBehaviors, 0);
}

class ProtocolError extends Error {
  constructor(code, message, context = {}, cause) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "PlaceboError";
    this.code = code;
    this.context = context;
  }
}

function require(condition, code, message, context) {
  if (!condition) throw new ProtocolError(code, message, context);
}

const hints = {
  "invalid-config": "Render the form through its Placebo action binding; check its configuration and reload after rebuilding.",
  "version-mismatch": "Serve the Rust and browser runtime from the same build, then reload the page.",
  "missing-target": "Mount the component before submitting its form, and check that the binding and mounted id agree.",
  "duplicate-target": "Give each mounted component a unique id.",
  "undeclared-target": "Mount this element through Component so it declares the update boundary.",
  "remounted-target": "The response belongs to an older DOM instance. Read current server state before retrying a write.",
  "http-error": "Inspect the request in Network and correlate its X-Placebo-Request-Id with server logs.",
  "invalid-content-type": "Return a Placebo reply from this action. Check for a login/error page or an extractor rejection in the server logs; a value that does not decode, such as a malformed number for a non-Option field, is rejected before the handler runs.",
  "page-error": "Wrap the finished router with placebo::native_forms(app), which renders the page a reply belongs to. The server logged the reason as [placebo:page-error].",
  "page-missing": "The page the form is on answered a GET with an error, often because the reply removed what it shows, such as a deleted record. A successful reply can send the person on with navigate(path). The server logged the status as [placebo:page-missing].",
  "unmounted-target": "Mount the component on the page its form is on, in every render of that page, so the page rendered for its reply can show the reply.",
  "unadapted-route": "Register the handler with its action's adapter: .route(ACTION.path(), ACTION.route(handler)). Plain Axum routes skip payload decoding and the mutation request check.",
  "response-read-error": "Check Network and server logs for an interrupted response body. Submit the form again to retry the write safely with its idempotency key.",
  "network-error": "Check Network and server logs. The write may have committed: submit the form again to retry it safely. The retry resends the same request with its idempotency key, so the server replays its recorded reply instead of writing twice.",
  "replay-pending": "The first attempt is still running, or stopped after possibly writing. Wait and submit again, or reload to see current data.",
  "replay-unavailable": "The server's replay store failed, so nothing was saved by this attempt. Check the server logs, then submit again.",
  "replay-unknown": "The first attempt is older than the server's replay store remembers, so it may have written. Reload to see current data before submitting again. To allow later retries, keep replies longer in the ReplayStore.",
  "redirected": "Sign in again, in another tab to keep this page's input, then resubmit. If the action's handler redirects, return binding.reply(...).navigate(path) instead.",
  "response-mismatch": "Build the reply from the same action and component instance as the initiating form.",
  "invalid-update": "Build the reply with reply(), invalid(), or conflict() from the same binding as the form.",
  "invalid-outcome": "Use reply(), invalid(), or conflict() so HTTP status and update outcome agree.",
  "duplicate-local": "Give each retained subtree a unique key within its component. Two forms for the same action in one component cannot both render a field of the same name.",
  "local-shape": "Keep a retained local key on the same element type, or use a new key for a fresh subtree.",
  "nested-local": "Use separate local ownership boundaries; nested local subtrees are not supported.",
  "local-component": "Mount the nested component outside the data-placebo-local subtree; a nested component keeps its own node and state already.",
  "nested-component": "Mount a nested component the same way in every render (mount or mount_dialog), so its root element keeps its type.",
  "invalid-feed": "Mount the feed with feed.mount() from the same build as the runtime.",
  "push-disconnected": "The browser reconnects by itself. If this repeats, check the feed's route and any proxy timeouts; Network shows the event stream.",
  "push-closed": "Register the feed's route with .route(FEED.path(), FEED.route()), check the path and that it returns text/event-stream, then reload.",
  "missing-command-target": "Give the dialog or popover the id the button names, or mount it on this page. A mount_dialog component's id is its component id, such as 'task:1'.",
  "unknown-class": "Use a class from the kit's README. A class that only hooks a script or a test should be a data attribute, such as data-feedback; a new visual pattern goes into the kit after the person approves it.",
  "invalid-command": "Point show-modal/close/request-close at a <dialog>, and show-/hide-/toggle-popover at an element with popover.",
  "unknown-behavior": "Check the name and module import. Register with behavior() before mounting, or reserve an asynchronous import with lazyBehavior().",
  "behavior-setup": "Inspect the original cause and setup function. Return a cleanup function or undefined.",
  "behavior-cleanup": "Inspect the original cause and cleanup function; release only resources owned by this behavior.",
  "mutation-interrupted": "Aborting a request cannot undo a write. Read current server state before retrying.",
  "unsupported-method": "Render a read form with Read::new().form(fields) and a mutation form with its binding's form(fields).",
  "cross-origin-action": "Use a same-origin action URL.",
  "cross-origin-navigation": "Navigate replies to a path on this site; link to other sites from the page instead.",
  "invalid-component": "Render the mutation form in the contents of the component it updates, not in a component nested inside it.",
  "unsupported-file": "Give the payload an Upload field and render it with Control::file(); only mutation forms send files.",
  "upload-too-large": "The runtime checks file sizes against data-placebo-max-bytes before sending; render the input with Control::file(), or raise the field's Upload<MAX_BYTES>.",
};

function report(work, error, fallback, extra = {}, level = "error") {
  const code = error instanceof ProtocolError ? error.code : fallback;
  emit(level, work, { code, message: error instanceof Error ? error.message : String(error),
    hint: hints[code] ?? "Inspect the diagnostic context and the action or behavior implementation.",
    ...error?.context, ...extra }, error);
}

function requestContext(work) {
  return {
    requestId: work?.requestId ?? null,
    method: work?.method ?? null,
    path: work?.path ?? null,
    phase: work?.phase ?? null,
    status: work?.status ?? null,
    contentType: work?.contentType ?? null,
    requestState: !work?.sent ? "not-started" : work.phase === "request" ? "started" : "response-received",
    updateState: work?.applied ? "applied" : work?.phase === "applying" ? "possibly-partial" : "not-applied",
    writeState: work?.method !== "POST" ? "not-applicable" : !work.sent || work.notSaved ? "not-started" :
      work.applied ? (work.outcome === "applied" ? "acknowledged" : "rejected") : "unknown",
  };
}

function emit(type, work, extra = {}, cause) {
  const detail = {
    ...requestContext(work),
    action: work?.config?.action ?? null,
    target: work?.config?.target ?? null,
    ...extra,
  };
  document.dispatchEvent(new CustomEvent(`placebo:${type}`, { detail }));
  if (type === "error" || type === "warning" || tracing) {
    const context = [detail.requestId && `request=${detail.requestId}`, detail.action && `action=${detail.action}`,
      detail.target && `target=${detail.target}`, detail.relatedTarget && `relatedTarget=${detail.relatedTarget}`,
      detail.behavior && `behavior=${detail.behavior}`, detail.element && `element=${detail.element}`,
      detail.method && `${detail.method} ${detail.path}`, detail.status != null && `HTTP ${detail.status}`].filter(Boolean).join(" ");
    const outcome = work ? ` Request: ${detail.requestState}; update: ${detail.updateState}; write: ${detail.writeState}.` : "";
    const reason = detail.message ?? detail.reason ?? detail.outcome ?? detail.phase ?? "";
    const line = `[placebo:${detail.code ?? type}] ${context}: ${String(reason).replace(/[.\s]+$/, "")}.${outcome}${detail.hint ? ` Next: ${detail.hint}` : ""}`;
    const log = type === "error" ? console.error : type === "warning" ? console.warn : console.info;
    log.call(console, line, detail, ...(cause === undefined ? [] : [cause]));
  }
}

function configFor(form, work) {
  const source = form.dataset.placebo;
  let config = configs.get(form);
  if (config?.source === source) config = config.parsed;
  else {
    try { config = JSON.parse(source); }
    catch { throw new ProtocolError("invalid-config", "The action configuration is not valid JSON."); }
    configs.set(form, { source, parsed: config });
  }
  if (work && config && typeof config === "object") work.config = config;
  require(config?.version === VERSION, "version-mismatch", `Browser protocol ${VERSION} does not match form protocol ${config?.version}.`,
    { expectedVersion: VERSION, receivedVersion: config?.version ?? null });
  // A read form reads the page it is on; a mutation form posts to its action
  // and answers for its component.
  if (config.read === true) {
    require(form.method === "get" && !form.hasAttribute("action"), "unsupported-method",
      "A read form is a GET form without an action, so it reads the page it is on.");
    require(config.input_delay_ms === undefined || (Number.isInteger(config.input_delay_ms) &&
      config.input_delay_ms >= 0 && config.input_delay_ms <= 60000), "invalid-config", "Invalid input delay.");
    require(config.reveal === undefined || config.reveal === true, "invalid-config", "Invalid reveal trigger.");
  } else {
    require(typeof config.action === "string" && config.action.length > 0 &&
      typeof config.target === "string" && config.target.length > 0, "invalid-config", "Expected a read, or a named action and its component.");
    require(form.method === "post", "unsupported-method", "A mutation form posts to its action.");
  }
  const url = new URL(form.action, document.baseURI);
  require(url.origin === location.origin, "cross-origin-action", "Actions must use the document's origin.");
  return config;
}

function targetFor(id) {
  // Count all ids, including ordinary elements: getElementById alone hides duplicates.
  const matches = document.querySelectorAll(`#${CSS.escape(id)}`);
  require(matches.length > 0, "missing-target", `Component '${id}' is not mounted.`, { relatedTarget: id });
  require(matches.length === 1, "duplicate-target", `Component '${id}' has multiple elements with the same id.`, { relatedTarget: id });
  require(matches[0].hasAttribute("data-placebo-component"), "undeclared-target", `Element '${id}' is not a mounted component.`, { relatedTarget: id });
  return matches[0];
}

function finish(work) {
  clearTimeout(work.timer);
  if (pending.get(work.target) !== work) return;
  pending.delete(work.target);
  if (work.previousBusy === null) work.target.removeAttribute("aria-busy");
  else work.target.setAttribute("aria-busy", work.previousBusy);
  // A "load more" form watches again once its read is done.
  if (work.method === "GET") queueMicrotask(syncTriggers);
  // This save's own feed signal came while it was on its way, but its page
  // did not show: read the page after all.
  if (work.signalled && !work.shown) queueMicrotask(() => refreshPage(work.signalled));
}

function cancel(work, reason) {
  work.controller.abort();
  finish(work);
  emit("discarded", work, { reason });
  if (work.method === "POST" && work.sent && !work.applied) {
    report(work, new ProtocolError("mutation-interrupted", `Mutation response discarded: ${reason}. The server write outcome is unknown.`),
      "mutation-interrupted", { reason }, "warning");
  }
}

function isCurrent(work) {
  if (pending.get(work.target) !== work) return false;
  if (!work.form.isConnected || !work.target.isConnected) {
    cancel(work, "unmounted");
    return false;
  }
  return true;
}

function parseHTML(html) {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content;
}

// A mutation's reply is its whole page, rendered again with the submitted
// component showing the reply. The page morphs into the document, so every
// part of it shows current data while nodes, focus, and scroll stay. What the
// person owns is kept: edited controls, open dialogs and disclosures, and
// components with a request of their own in flight.
function applyPage(work, update) {
  work.phase = "validating-response";
  const { outcome } = update;
  require(targetFor(work.config.target) === work.target, "remounted-target", "The original target instance no longer owns this response.");
  // A reply shows in its component alone when the server could not render
  // its page, when its page was rendered before the one shown (which then
  // lacks that page's writes), or when a read has since moved this page to
  // another query. The rest of the page stays as it is.
  const moved = update.page != null && work.pageUrl !== here();
  const older = update.page != null && !moved && update.rendered != null && update.rendered <= shownRendered;
  const whole = update.page != null && !moved && !older;
  const [root, incoming] = whole ? [document.body, update.page.body]
    : [work.target, update.page ? update.page.getElementById(work.config.target) : contentsFor(work.target, update.alone)];
  // Every local key and component root is checked before the first change.
  const plan = incoming ? planPage(work, root, incoming, outcome) : emptyPlan();
  work.phase = "applying";
  const anchor = focusAnchor(document.body);
  if (incoming) morphPage(root, incoming, plan);
  if (whole) {
    shownRendered = Math.max(shownRendered, update.rendered ?? 0);
    if (update.page.title && update.page.title !== document.title) document.title = update.page.title;
    adoptRoot(update.page);
  }
  restoreFocus(document.body, anchor);
  for (const component of plan.morphed) settle(component);
  work.applied = true;
  work.outcome = outcome;
  work.phase = "applied";
  // A page that shows this write, or one newer than it.
  work.shown = whole || older || moved;
  if (outcome === "invalid" && plan.morphed.includes(work.target)) focusInvalid(work, { preserved: plan.summary.preservedLocal });
  syncBehaviors(work);
  emit("applied", work, { outcome, replayed: Boolean(work.replayed), ...plan.summary, navigate: null,
    page: whole ? "whole" : older ? "older" : moved ? "moved" : "missing" });
  // The rest of the page shows the write once read again at its new query;
  // the component keeps the reply it just showed.
  if (moved) {
    holding.add(work.target);
    refreshPage({ source: "moved" });
  }
  if (update.unmounted && outcome !== "applied") {
    report(work, new ProtocolError("unmounted-target", `The page this form is on does not mount component '${work.config.target}', so its ${outcome} reply could not be shown. The rest of the page was updated.`),
      "unmounted-target");
  }
  if (update.pageError != null) {
    report(work, new ProtocolError("page-error", `The server could not render the page for this ${outcome} reply, so it shows in its component alone: ${update.pageError}.`), "page-error");
  } else if (update.pageMissing != null) {
    report(work, new ProtocolError("page-missing", `The page for this ${outcome} reply did not render, so the reply shows in its component alone and the rest of the page may be out of date: ${update.pageMissing}.`),
      "page-missing", {}, "warning");
  }
}

// The reply's contents in a detached copy of its component's root element.
function contentsFor(target, html) {
  const root = target.cloneNode(false), template = document.createElement("template");
  template.innerHTML = html;
  root.append(template.content);
  return root;
}
const byId = (root, id) => root.querySelector(`#${CSS.escape(id)}`);
// Saves this page sent, by request id, to recognize the feed signals they cause.
const sent = new Map();
// The page shown: its path and query, which every page read and reply renders.
const here = () => location.pathname + location.search;
// When the page shown was rendered, from its reply's X-Placebo-Rendered. The
// loaded page is older than any reply, since the reply's request came later.
let shownRendered = 0;
// The <html> attributes the server rendered last, such as a theme or a
// language. A page shown later brings its own, as it brings its title;
// attributes a script or an extension set on <html> are left alone.
let rootAttributes = new Set(Array.from(document.documentElement.attributes, ({ name }) => name));
function adoptRoot(page) {
  const root = document.documentElement;
  const next = page.documentElement;
  for (const name of rootAttributes) if (!next.hasAttribute(name)) root.removeAttribute(name);
  for (const { name, value } of next.attributes) if (root.getAttribute(name) !== value) root.setAttribute(name, value);
  rootAttributes = new Set(Array.from(next.attributes, ({ name }) => name));
}
// Components showing a reply the next page read must not replace.
let holding = new WeakSet();

// Decide what the morph keeps, without changing the document.
function emptyPlan() {
  return { skipped: new Set(), plans: new Map(), morphed: [], editing: new Set(),
    summary: { refreshedLocal: [], preservedLocal: [], refreshedComponents: [], skippedComponents: [] } };
}

function planPage(work, root, incoming, outcome) {
  const plan = emptyPlan(), { skipped, plans, morphed, summary } = plan;
  // A read or refresh answers no component: every component takes it as a
  // refresh from another action.
  const target = work?.target ?? null;
  const read = work?.method === "GET";
  const inSkipped = node => Array.from(skipped).some(root => root.contains(node));
  // A component with its own request in flight keeps its contents: its own
  // reply carries its state.
  for (const old of root.querySelectorAll("[data-placebo-component]")) {
    if (holding.has(old) && !work?.method) {
      skipped.add(old);
      summary.skippedComponents.push({ target: old.id, reason: "reply" });
      continue;
    }
    if (!pending.has(old) || (!read && (old === target || old.contains(target)))) continue;
    skipped.add(old);
    summary.skippedComponents.push({ target: old.id, reason: "busy" });
  }
  // Local units, per component and outside all components, by the rules of a
  // reply: the reply's own component by its outcome, every other one as a
  // refresh from another action (edited controls stay). A read form's own
  // controls are the read's, as a component's are its reply's.
  const scopes = [[root, incoming, root === target ? root : null]];
  if (root === target) morphed.push(root);
  for (const old of root.querySelectorAll("[data-placebo-component]")) {
    const next = byId(incoming, old.id);
    if (inSkipped(old) || !next?.hasAttribute("data-placebo-component")) continue;
    require(old.localName === next.localName, "nested-component",
      `Component '${old.id}' changed its root element from <${old.localName}> to <${next.localName}>.`, { relatedTarget: old.id });
    scopes.push([old, next, old]);
    morphed.push(old);
    if (old !== target) summary.refreshedComponents.push(old.id);
  }
  for (const [oldRoot, newRoot, component] of scopes) {
    const own = component !== null && component === target;
    const current = locals(oldRoot);
    // A form someone is editing keeps the hidden values it was rendered with,
    // such as a record's version, through a refresh from another action: its
    // next save must be checked against what the person started from, or it
    // would overwrite a change they never saw. Its own reply updates them.
    if (!own) for (const form of oldRoot.querySelectorAll("form")) {
      if (!inSkipped(form) && !target?.contains(form) && controlsOf(form).some(control => control.type !== "hidden" && changedFromDefault(control))) plan.editing.add(form);
    }
    for (const [key, next] of locals(newRoot)) {
      const old = current.get(key);
      if (!old || inSkipped(old)) continue;
      require(old.tagName === next.tagName, "local-shape", `Local key '${key}' changed element type.`);
      const controls = controlsOf(old);
      const snapshot = work?.locals?.get(key);
      const mine = own || (read && snapshot?.node === old);
      const unchanged = !mine || (snapshot?.node === old && snapshot.edit === (edits.get(old) ?? 0) && snapshot.values === localValues(old));
      const edited = controls.some(changedFromDefault);
      const take = controls.length > 0 && unchanged && (!edited || (mine && outcome === "applied" && snapshot.submitted));
      plans.set(old, { keep: !take, next });
      if (!own) continue;
      if (take) summary.refreshedLocal.push(key);
      else if (controls.length) summary.preservedLocal.push({ key, reason: unchanged ? "unsaved-edits" : "edited-since-submission" });
    }
  }
  return plan;
}

function morphPage(root, incoming, plan) {
  // A kept or refreshed local unit must meet its own incoming counterpart,
  // even if it moved, so each pair shares an id while the morph runs. It
  // ends with the id it shows: its own if kept, the reply's if refreshed.
  const paired = [];
  for (const [old, { keep, next }] of plan.plans) {
    if (old.id && old.id === next.id) continue;
    paired.push([old, keep ? old.id : next.id]);
    old.id = next.id = `placebo-pair-${paired.length}`;
  }
  // A feed's element keeps the subscription it has.
  const clientOwned = (name, element) =>
    (name === "open" && (element.localName === "dialog" || element.localName === "details")) ||
    name === "data-placebo-stale" || name === "data-placebo-feed" || (name === "aria-busy" && pending.has(element));
  try {
    // The children, not the parent: idiomorph would morph a parent that has
    // one of its own (a parsed <body>) in as a single child.
    Idiomorph.morph(root, Array.from(incoming.childNodes), { morphStyle: "innerHTML", restoreFocus: true, callbacks: {
      beforeNodeMorphed(old, next) {
        if (old.nodeType !== Node.ELEMENT_NODE) return true;
        if (plan.skipped.has(old)) return false;
        const local = plan.plans.get(old);
        // Any other control the person changed keeps its value too, such as a
        // note in a plain textarea.
        const unit = local ? null : localUnit(old);
        const edited = !local && old.matches("input,textarea,select") && !(unit && plan.plans.has(unit))
          && (changedFromDefault(old) || (old.type === "hidden" && plan.editing.has(old.form)));
        if (!local?.keep && !edited) {
          // Idiomorph leaves a file input's files; a unit taking the reply starts empty.
          if (local) for (const control of controlsOf(old)) if (control.type === "file") control.value = "";
          return true;
        }
        // A kept control still takes the reply's validation state.
        const source = local?.next ?? next;
        for (const name of SYNCED) {
          if (source.hasAttribute(name)) old.setAttribute(name, source.getAttribute(name));
          else old.removeAttribute(name);
        }
        return false;
      },
      beforeAttributeUpdated: (name, element) => !clientOwned(name, element),
    } });
  } finally {
    for (const [old, id] of paired) {
      if (id) old.id = id;
      else old.removeAttribute("id");
    }
  }
}

const FOCUSABLE = "a[href],button:not([disabled]),input:not([disabled]):not([type=hidden]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex='-1'])";

// A node belongs to its nearest component. Nested components own their own
// contents: the page's morph plans each one separately.
function ownedBy(node, root) {
  const component = node.closest("[data-placebo-component]");
  return root.nodeType === Node.ELEMENT_NODE && root.hasAttribute("data-placebo-component")
    ? component === root : !component || !root.contains(component);
}

function locals(root) {
  const found = new Map();
  for (const element of root.querySelectorAll("[data-placebo-local],[data-placebo-field]")) {
    if (!ownedBy(element, root)) continue;
    const explicit = element.hasAttribute("data-placebo-local");
    const outer = element.parentElement?.closest("[data-placebo-local]");
    const nested = outer && root.contains(outer);
    // A typed control inside an explicit local belongs to that local.
    if (!explicit && nested) continue;
    require(!nested, "nested-local", "Local subtrees cannot be nested.");
    require(!explicit || !element.querySelector("[data-placebo-component]"), "local-component",
      `Local '${element.dataset.placeboLocal}' contains nested component '${element.querySelector("[data-placebo-component]")?.id}'.`);
    const key = explicit ? element.dataset.placeboLocal : fieldKey(element);
    require(key && !found.has(key), "duplicate-local", `Local key '${key}' must be unique within its component.`);
    found.set(key, element);
  }
  return found;
}

// Fields are keyed by their form's action, so forms for different actions in
// one component can reuse a field name.
function fieldKey(element) {
  const form = element.closest("form");
  const action = form?.getAttribute("action");
  if (form && action == null) {
    // Read forms have no action: they are told apart by their order in their
    // component or page.
    const scope = form.parentElement?.closest("[data-placebo-component]") ?? form.getRootNode();
    const reads = Array.from(scope.querySelectorAll("form:not([action])"));
    return `read-${reads.indexOf(form)}#${element.dataset.placeboField}`;
  }
  return `${action == null ? "" : new URL(action, document.baseURI).pathname}#${element.dataset.placeboField}`;
}

function localUnit(element) {
  return element.closest?.("[data-placebo-local]") ?? element.closest?.("[data-placebo-field]") ?? null;
}

function controlsOf(node) {
  const selector = "input,textarea,select";
  return node.matches(selector) ? [node] : Array.from(node.querySelectorAll(selector));
}

// The server-rendered defaults are the markup's value, checked, and selected
// attributes. A control differs from them only after someone changed it.
function changedFromDefault(control) {
  if (control instanceof HTMLSelectElement) {
    const options = Array.from(control.options);
    let defaults = options.filter(option => option.defaultSelected);
    if (!control.multiple) defaults = defaults.length ? defaults.slice(-1) : options.filter(option => !option.disabled).slice(0, 1);
    const selected = options.filter(option => option.selected);
    return selected.length !== defaults.length || selected.some((option, i) => option !== defaults[i]);
  }
  if (control.type === "checkbox" || control.type === "radio") return control.checked !== control.defaultChecked;
  return control.value !== control.defaultValue;
}

function localValues(node) {
  return JSON.stringify(controlsOf(node).map(input =>
    [input.name, input.value, input.checked, input instanceof HTMLSelectElement ? Array.from(input.selectedOptions, option => option.value) : null]));
}

function snapshotLocals(target, form) {
  return new Map(Array.from(locals(target), ([key, node]) => [key, { node, edit: edits.get(node) ?? 0, values: localValues(node),
    submitted: controlsOf(node).some(control => control.form === form) }]));
}

// Validation state follows the server even when the edited control is kept.
const SYNCED = ["aria-invalid", "aria-describedby", "aria-errormessage", "aria-required", "disabled", "readonly", "required"];

function focusable(node) {
  // Also not inside a closed dialog or details, which show nothing to focus.
  return node.matches(FOCUSABLE) && !node.closest("[inert],[hidden]") && (node.checkVisibility?.() ?? true);
}

// Remember what had focus inside a subtree about to be replaced, such as the
// submit button, so the matching element in the new markup can take it.
function focusAnchor(root) {
  const focused = document.activeElement;
  if (!focused || focused === document.body || focused === root || !root.contains(focused)) return null;
  const same = Array.from(root.querySelectorAll(focused.localName)).filter(focusable);
  return { element: focused, id: focused.id, tag: focused.localName, text: focused.textContent.trim(), index: same.indexOf(focused) };
}

function restoreFocus(root, anchor) {
  if (!anchor || anchor.element.isConnected) return;
  const active = document.activeElement;
  if (active && active !== document.body) return;
  const byId = anchor.id ? root.querySelector(`#${CSS.escape(anchor.id)}`) : null;
  const same = Array.from(root.querySelectorAll(anchor.tag)).filter(focusable);
  const sameText = node => anchor.text && node.textContent.trim() === anchor.text;
  const next = (byId && focusable(byId) ? byId : null) ?? (same[anchor.index] && sameText(same[anchor.index]) ? same[anchor.index] : null) ??
    same.find(sameText) ?? same[Math.min(anchor.index, same.length - 1)] ?? Array.from(root.querySelectorAll(FOCUSABLE)).find(focusable);
  next?.focus({ preventScroll: true });
}

// After a rejected submission, take the person to the first field the server
// marked invalid, unless they have already moved on or kept typing.
function focusInvalid(work, primary) {
  if (primary.preserved.some(local => local.reason === "edited-since-submission")) return;
  const active = document.activeElement;
  if (active && active !== document.body && !work.target.contains(active)) return;
  const invalid = work.target.querySelector('[aria-invalid="true"]');
  const control = invalid && (focusable(invalid) ? invalid : invalid.querySelector("input:checked") ?? invalid.querySelector(FOCUSABLE));
  if (control && control !== active) control.focus({ preventScroll: true });
}

// Every reply is a page (or a component's part of one), so it waits for any
// active IME composition to end.
function applyOrDefer(work, update) {
  if (Array.from(document.forms).some(form => composing.has(form))) {
    work.deferred = update;
    work.phase = "deferred";
    emit("deferred", work, { reason: "composing" });
    return;
  }
  work.deferred = null;
  if (work.method === "GET") showPage(work, update.page, update.rendered, work.pageUrl, { source: work.source });
  else applyPage(work, update);
}

async function send(work) {
  if (!isCurrent(work)) return;
  work.phase = "request";
  work.sent = true;
  work.sentAt = performance.now();
  const mutation = work.method === "POST";
  // A mutation is answered with the page it was submitted from. A read puts
  // its query in the address as it goes, so replies and refreshes from now on
  // render the page at that query.
  if (mutation) {
    work.pageUrl = here();
    sent.set(work.requestId, work);
    if (sent.size > 64) sent.delete(sent.keys().next().value);
  } else {
    work.pageUrl = work.url.pathname + work.url.search;
    if (work.pageUrl !== here()) history.replaceState(history.state, "", work.url);
    shown = work.pageUrl;
  }
  emit("request", work);
  try {
    const response = await fetch(work.url, {
      method: work.method,
      headers: { Accept: "text/html", "X-Placebo-Request-Id": work.requestId,
        ...(mutation ? { "X-Placebo-Request": String(VERSION), "X-Placebo-Page": work.pageUrl } : { "X-Placebo-Refresh": String(VERSION) }),
        // The server runs a retry only if it would still remember the first attempt.
        ...(work.retryOf ? { "X-Placebo-Retry": String(Math.ceil(work.sentAt - work.firstSentAt)) } : {}) },
      body: work.body,
      credentials: "same-origin",
      // A page read must not come from the HTTP cache.
      cache: "no-store",
      // "error" would report a login redirect as a network failure. A manual
      // redirect is opaque: its status and location are not exposed.
      redirect: "manual",
      signal: work.controller.signal,
    });
    work.phase = "response";
    const redirected = response.type === "opaqueredirect";
    if (!redirected) {
      work.status = response.status;
      work.contentType = response.headers.get("content-type")?.split(";")[0].trim() ?? null;
    }
    if (!isCurrent(work)) return;
    require(!redirected, "redirected", "The server redirected this action instead of answering it, often to a login page after a session expired.");
    const adapter = response.headers.get("x-placebo-action");
    // A successful answer to a mutation without the adapter's marker came from
    // a plain Axum route. Other unmarked responses (proxy errors, login pages)
    // keep their HTTP diagnostics. A read is answered by its page's route.
    if (mutation) require(adapter === work.config.action || (adapter === null && !response.ok),
      "unadapted-route", adapter === null
        ? `Action '${work.config.action}' responded without its typed route adapter.`
        : `Action '${work.config.action}' was answered by the adapter for '${adapter}'.`,
      { respondingAction: adapter });
    const replay = response.headers.get("x-placebo-replay");
    require(replay !== "pending", "replay-pending",
      "The server has an earlier attempt of this submission that has not finished, so it did not run it again.");
    require(replay !== "unavailable", "replay-unavailable", "The server could not check for an earlier attempt, so it did not run this one.");
    require(replay !== "unknown", "replay-unknown", "The server no longer knows whether the first attempt of this submission was saved, so it did not run it again.");
    work.replayed = replay === "replayed";
    if (response.status === 413) {
      work.notSaved = true;
      throw new ProtocolError("upload-too-large", `The server refused a file over its field's limit of ${response.headers.get("x-placebo-upload-limit") ?? "?"} bytes before the handler ran. Nothing was saved.`);
    }
    const outcome = response.headers.get("x-placebo-outcome");
    const expectedOutcome = response.ok ? "applied" : response.status === 422 ? "invalid" : response.status === 409 ? "conflict" : null;
    require(response.ok || (mutation && expectedOutcome), "http-error", `Action returned HTTP ${response.status}.`);
    require(work.contentType === "text/html",
      "invalid-content-type", `Expected 'text/html'; received '${work.contentType ?? "no content type"}'.`);
    if (mutation) require(outcome === expectedOutcome, "invalid-outcome", "Response status and reply outcome disagree.");
    const navigate = mutation ? response.headers.get("x-placebo-navigate") : null;
    if (navigate !== null) {
      require(outcome === "applied", "invalid-update", "Only a successful mutation reply can navigate.");
      const destination = new URL(navigate, document.baseURI);
      require(destination.origin === location.origin, "cross-origin-navigation", "A reply can only navigate within this site.");
      if (!isCurrent(work)) return;
      work.applied = true;
      work.outcome = outcome;
      work.phase = "applied";
      work.shown = true;
      settle(work.target);
      emit("applied", work, { outcome, replayed: Boolean(work.replayed), ...emptyPlan().summary,
        navigate: destination.pathname + destination.search + destination.hash });
      visit(destination, "push");
      return;
    }
    let body, update;
    try { body = await response.text(); }
    catch (error) {
      throw new ProtocolError("response-read-error", "Could not finish reading the update response body.", {}, error);
    }
    const rendered = renderedAt(response);
    const pageError = response.headers.get("x-placebo-page-error"), pageMissing = response.headers.get("x-placebo-page-missing");
    if (mutation && (pageError !== null || pageMissing !== null)) update = { alone: body, outcome, pageError, pageMissing };
    else update = { page: new DOMParser().parseFromString(body, "text/html"), outcome, rendered,
      unmounted: response.headers.get("x-placebo-unmounted") !== null };
    // Cancellation is an optimization. This check also protects against a
    // transport that delivers an older response despite cancellation.
    if (!isCurrent(work)) return;
    applyOrDefer(work, update);
  } catch (error) {
    if (work.controller.signal.aborted || !isCurrent(work)) return;
    report(work, error, work.phase === "request" ? "network-error" : "invalid-update");
    if (!work.notSaved) markStale(work);
  } finally {
    if (!work.deferred) finish(work);
  }
}

// A write may have committed while the page could not show it. Mark the
// component, and remember the request: submitting the same form again resends
// it with its idempotency key. Server state shown in the component clears it.
function markStale(work) {
  if (work.method !== "POST" || !work.sent || work.applied || !work.target?.isConnected) return;
  work.target.setAttribute("data-placebo-stale", "");
  // A retry keeps the first attempt's snapshot of the controls, id, and time.
  uncertain.set(work.target, { form: work.form, body: work.body, locals: work.locals,
    requestId: work.retryOf ?? work.requestId, firstSentAt: work.firstSentAt ?? work.sentAt });
}

// The component shows server state again, so an earlier attempt is settled.
function settle(target) {
  target.removeAttribute("data-placebo-stale");
  uncertain.delete(target);
}

function attempt(form) {
  return { config: {}, form, phase: "preflight", method: form.method.toUpperCase(),
    // Exclude query strings and fragments: they can contain user input.
    path: new URL(form.action, document.baseURI).pathname,
    requestId: crypto.randomUUID?.() ?? `p-${Date.now().toString(36)}-${crypto.getRandomValues(new Uint32Array(1))[0].toString(36)}` };
}

function schedule(form, input, submitter = null, source = "user") {
  let work;
  try {
    work = attempt(form);
    const config = configFor(form, work);
    if (input && config.input_delay_ms == null) return;
    // A read belongs to its form; a mutation to its component.
    const read = config.read === true;
    const target = read ? form : targetFor(config.target);
    const previous = pending.get(target);
    if (previous && !read) {
      emit("ignored", work, { reason: "busy", activeRequestId: previous.requestId });
      return;
    }
    if (!read) {
      const nearest = form.closest("[data-placebo-component]");
      require(target.hasAttribute("data-placebo-component") && nearest === target, "invalid-component", nearest && target.contains(nearest)
        ? `This form is inside nested component '${nearest.id}' but targets '${target.id}'. A form belongs to its nearest component.`
        : "A mutation form must belong to its target component.", { relatedTarget: nearest?.id ?? null });
    }
    const url = new URL(form.action, document.baseURI);
    // Submitting a form whose last attempt has an unknown outcome retries that
    // attempt exactly, so the server can replay it instead of writing twice.
    const retry = read ? null : uncertain.get(target);
    let body, localSnapshots;
    if (retry?.form === form) {
      body = retry.body;
      localSnapshots = retry.locals;
    } else {
      const data = new FormData(form, submitter);
      // Each new submission gets its own key, even from markup a cache or
      // another page served with the same one.
      if (!read && data.has(KEY_FIELD)) data.set(KEY_FIELD, freshKey());
      if (!read && form.enctype === "multipart/form-data") {
        if (!checkUploads(form, work)) return;
        body = data;
      } else {
        const fields = new URLSearchParams();
        for (const [name, value] of data) {
          require(typeof value === "string", "unsupported-file", "Files need a mutation form whose payload has an Upload field.");
          fields.append(name, value);
        }
        if (read) url.search = fields.toString();
        else body = fields;
      }
      localSnapshots = read ? readSnapshot(form) : snapshotLocals(target, form);
    }
    // The newest read on the page wins: one still waiting or in flight is dropped.
    if (read) for (const other of Array.from(pending.values())) if (other.method === "GET") cancel(other, "superseded");
    work = { ...work, config, form, target, locals: localSnapshots, url, method: read ? "GET" : "POST", phase: "scheduled",
      body, controller: new AbortController(), source,
      ...(retry?.form === form ? { retryOf: retry.requestId, firstSentAt: retry.firstSentAt } : {}),
      previousBusy: target.getAttribute("aria-busy"), timer: null };
    pending.set(target, work);
    target.setAttribute("aria-busy", "true");
    emit("scheduled", work, retry?.form === form
      ? { source, retry: true, retryOf: retry.requestId, reason: `retrying request ${retry.requestId} unchanged, with its idempotency key` }
      : { source, ...(source === "reveal" ? { reason: "read on reveal" } : {}) });
    work.timer = setTimeout(() => send(work), input ? config.input_delay_ms : 0);
  } catch (error) {
    report(work, error, "invalid-action");
  }
}

// A read form's own controls, which take the page's values unless they
// change while the read is on its way.
function readSnapshot(form) {
  const scope = form.closest("[data-placebo-component]") ?? document.body;
  return new Map(Array.from(snapshotLocals(scope, form)).filter(([, snapshot]) => form.contains(snapshot.node)));
}

function freshKey() {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, "0")).join("");
}

function readableSize(bytes) {
  return bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : bytes >= 1024 ? `${Math.floor(bytes / 1024)} KB` : `${bytes} bytes`;
}

// A file over its field's limit would be refused by the server after the
// upload. Tell the person on the file input instead, and send nothing.
function checkUploads(form, work) {
  for (const input of form.querySelectorAll("input[type=file][data-placebo-max-bytes]")) {
    const limit = Number(input.dataset.placeboMaxBytes);
    const total = Array.from(input.files ?? []).reduce((sum, file) => sum + file.size, 0);
    if (total <= limit) continue;
    const several = input.multiple && input.files.length > 1;
    input.setCustomValidity(`${several ? "These files are" : "This file is"} larger than ${readableSize(limit)}. Choose ${several ? "smaller files" : "a smaller file"}.`);
    input.reportValidity();
    emit("ignored", work, { reason: "upload-too-large", field: input.name, limit, size: total });
    return false;
  }
  return true;
}

function onSubmit(event) {
  const form = event.target;
  if (!(form instanceof HTMLFormElement) || !form.hasAttribute("data-placebo")) return;
  event.preventDefault();
  if (composing.has(form)) {
    try {
      const work = attempt(form);
      configFor(form, work);
      emit("ignored", work, { reason: "composing" });
    } catch (error) { report(null, error, "invalid-action"); }
    return;
  }
  schedule(form, false, event.submitter);
}

function onInput(event) {
  markEdited(event);
  if (event.isComposing) return;
  const form = event.target.form;
  if (form?.hasAttribute("data-placebo") && !composing.has(form)) schedule(form, true);
}

function markEdited(event) {
  // A new choice clears the size message checkUploads set.
  if (event.target instanceof HTMLInputElement && event.target.type === "file") event.target.setCustomValidity("");
  const local = localUnit(event.target);
  if (local) edits.set(local, (edits.get(local) ?? 0) + 1);
}

function onCompositionStart(event) {
  markEdited(event);
  const form = event.target.form;
  if (!form?.hasAttribute("data-placebo")) return;
  composing.add(form);
  // Invalidate the previous search while an IME is assembling new text,
  // without submitting intermediate characters to the server.
  const previous = pending.get(form);
  if (previous?.method === "GET" && previous.config.input_delay_ms != null) cancel(previous, "composing");
}

function onCompositionEnd(event) {
  const form = event.target.form;
  if (!form || !composing.has(form)) return;
  composing.delete(form);
  schedule(form, true);
  if (refreshAfterComposing && !Array.from(document.forms).some(other => composing.has(other))) {
    const extra = refreshAfterComposing;
    refreshAfterComposing = null;
    queueMicrotask(() => refreshPage(extra));
  }
  for (const work of pending.values()) {
    if (!work.deferred || !isCurrent(work)) continue;
    try { applyOrDefer(work, work.deferred); }
    catch (error) {
      work.deferred = null;
      report(work, error, "invalid-update");
      markStale(work);
    } finally {
      if (!work.deferred) finish(work);
    }
  }
}

// Server push. A mounted feed element subscribes the page to its feed, whose
// signal means "what this page shows has changed". The subscription lives as
// long as the element.
const feeds = new Map();
let unloading = false;

function syncFeeds() {
  for (const [element, feed] of feeds) {
    if (!started || !element.isConnected || element.dataset.placeboFeed !== feed.raw) closeFeed(element, feed);
  }
  if (!started) return;
  for (const element of document.querySelectorAll("[data-placebo-feed]")) if (!feeds.has(element)) openFeed(element);
}

function closeFeed(element, feed) {
  feed.source?.close();
  clearTimeout(feed.warning);
  feeds.delete(element);
}

function openFeed(element) {
  const feed = { element, raw: element.dataset.placeboFeed, config: null, source: null, down: false };
  feeds.set(element, feed);
  try {
    let config;
    try { config = JSON.parse(feed.raw); }
    catch { throw new ProtocolError("invalid-feed", "The feed configuration is not valid JSON."); }
    require(config?.version === VERSION, "version-mismatch", `Browser protocol ${VERSION} does not match feed protocol ${config?.version}.`,
      { expectedVersion: VERSION, receivedVersion: config?.version ?? null });
    require(typeof config.feed === "string" && typeof config.url === "string", "invalid-feed", "Expected a feed id and URL.");
    const url = new URL(config.url, document.baseURI);
    require(url.origin === location.origin, "cross-origin-action", "A feed must use the document's origin.");
    feed.config = config;
    feed.path = url.pathname;
  } catch (error) {
    report(null, error, "invalid-feed", { target: element.id || null });
    return;
  }
  const context = (extra = {}) => ({ target: feed.config.feed, feed: feed.config.feed, method: "GET", path: feed.path, ...extra });
  const source = new EventSource(feed.config.url);
  feed.source = source;
  source.addEventListener("open", () => {
    clearTimeout(feed.warning);
    emit("push", null, context({ phase: feed.down ? "push-reconnected" : "push-connected" }));
    feed.down = false;
  });
  source.addEventListener("error", () => {
    // Leaving the page closes its streams; that is not a failure.
    clearTimeout(feed.warning);
    feed.warning = setTimeout(() => {
      if (unloading || !feeds.has(element)) return;
      if (source.readyState === EventSource.CLOSED) {
        report(null, new ProtocolError("push-closed", `Feed '${feed.config.feed}' stopped and will not reconnect. Changes from now on do not reach this page.`),
          "push-closed", context());
      } else if (!feed.down) {
        feed.down = true;
        report(null, new ProtocolError("push-disconnected", `Feed '${feed.config.feed}' lost its connection and is reconnecting. A change made meanwhile shows once it is back.`),
          "push-disconnected", context(), "warning");
      }
    }, 50);
  });
  source.addEventListener("changed", event => {
    let signal;
    try { signal = JSON.parse(event.data); } catch { signal = null; }
    const extra = context({ eventId: event.lastEventId });
    try {
      require(signal?.version === VERSION, "version-mismatch", `Browser protocol ${VERSION} does not match feed protocol ${signal?.version}.`,
        { expectedVersion: VERSION, receivedVersion: signal?.version ?? null });
      require(signal.feed === feed.config.feed, "invalid-feed", `A signal for feed '${signal.feed}' arrived on feed '${feed.config.feed}'.`);
      require(signal.request === undefined || typeof signal.request === "string", "invalid-feed", "A signal's request is an id.");
    } catch (error) {
      report(null, error, "invalid-feed", extra);
      return;
    }
    // A signal this page's own save caused: the save answers with its page,
    // which shows the change, so reading the page again would only replace
    // the save's reply. If that page does not show, it is read after all.
    const own = signal.request === undefined ? undefined : sent.get(signal.request);
    if (own && (own.shown || pending.get(own.target) === own)) {
      if (!own.shown) own.signalled = { ...extra, source: "push" };
      emit("push", null, { ...extra, phase: "push-own", requestId: own.requestId });
      return;
    }
    emit("push", null, { ...extra, phase: "push-changed" });
    refreshPage({ ...extra, source: "push" });
  });
}

// A feed's signal or a poll reads the page again. Signals that arrive
// meanwhile are answered by one more read afterwards; a poll lets a slow
// read finish instead. A refresh waits for an IME composition to end.
let refreshing = null;
let refreshAfterComposing = null;
async function refreshPage(extra, { poll = false } = {}) {
  if (poll && (refreshing || Array.from(pending.values()).some(work => work.method === "GET"))) {
    emit("ignored", null, { ...extra, reason: "busy" });
    return;
  }
  if (refreshing) { refreshing.again = extra; return; }
  refreshing = { again: null };
  const url = here();
  try {
    const response = await fetch(url, { headers: { Accept: "text/html", "X-Placebo-Refresh": String(VERSION) },
      credentials: "same-origin", cache: "no-store", redirect: "manual" });
    require(response.type !== "opaqueredirect", "redirected",
      "Reading the page again was redirected, often to a login page after a session expired.");
    require(response.ok, "http-error", `Reading the page again returned HTTP ${response.status}.`, { status: response.status });
    const rendered = renderedAt(response);
    const page = new DOMParser().parseFromString(await response.text(), "text/html");
    if (!started) return;
    if (Array.from(document.forms).some(form => composing.has(form))) { refreshAfterComposing = extra; return; }
    showPage(null, page, rendered, url, extra);
  } catch (error) {
    report(null, error, "network-error", { ...extra, phase: "refresh" });
  } finally {
    const again = refreshing.again;
    refreshing = null;
    if (again) refreshPage(again);
  }
}

// When the server began rendering a page, from X-Placebo-Rendered.
function renderedAt(response) {
  const rendered = Number(response.headers.get("x-placebo-rendered") ?? NaN);
  return Number.isSafeInteger(rendered) ? rendered : null;
}

// A page read at a read form's query, or again, morphs in as a refresh from
// another action: edited controls, open dialogs, and components with a
// request in flight keep what they have. A page for a query the address no
// longer shows, or rendered before the page shown, is dropped.
function showPage(work, page, rendered, url, extra = {}) {
  if (work) work.phase = "validating-response";
  const reason = url !== here() ? "moved" : rendered != null && rendered <= shownRendered ? "older-page" : null;
  if (reason) {
    emit("discarded", work, { ...extra, reason });
    return;
  }
  const plan = planPage(work, document.body, page.body, "applied");
  if (work) work.phase = "applying";
  const anchor = focusAnchor(document.body);
  morphPage(document.body, page.body, plan);
  if (!work) holding = new WeakSet();
  shownRendered = Math.max(shownRendered, rendered ?? 0);
  if (page.title && page.title !== document.title) document.title = page.title;
  adoptRoot(page);
  restoreFocus(document.body, anchor);
  for (const component of plan.morphed) settle(component);
  if (work) Object.assign(work, { applied: true, outcome: "applied", phase: "applied" });
  syncBehaviors(work);
  emit("applied", work, { ...extra, outcome: "applied", ...plan.summary, page: "whole" });
}

// The browser restores the scroll of a page it loads again, such as on reload.
function onPageHide() { unloading = true; history.scrollRestoration = "auto"; }
function onPageShow() { unloading = false; history.scrollRestoration = "manual"; syncFeeds(); }

// Reads that start themselves: a read form when it scrolls into view, and
// the page again on an interval while a refresh_every element is on it. Each
// belongs to its element and ends when the element goes.
const revealing = new Map();
const polls = new Map();
let revealer = null;

function syncTriggers() {
  for (const [form, state] of revealing) {
    if (started && form.isConnected && form.dataset.placebo === state.raw) continue;
    revealer?.unobserve(form);
    revealing.delete(form);
  }
  for (const [element, poll] of polls) {
    if (started && element.isConnected && element.dataset.placeboRefreshEvery === poll.raw) continue;
    clearTimeout(poll.timer);
    polls.delete(element);
  }
  if (!started) return;
  for (const form of document.querySelectorAll("form[data-placebo]")) {
    let state = revealing.get(form);
    if (!state) {
      let config;
      // An invalid configuration is reported when the form is used.
      try { config = configFor(form); } catch { continue; }
      if (!config.reveal) continue;
      state = { raw: form.dataset.placebo, watching: false };
      revealing.set(form, state);
    }
    // Watch while the form asks for more than the page shows. After a read
    // the page renders the next form; one that failed asks for the same page.
    if (state.watching || pending.has(form) || readUrl(form) === here()) continue;
    state.watching = true;
    revealer ??= new IntersectionObserver(onReveal, { rootMargin: "200px" });
    revealer.observe(form);
  }
  for (const element of document.querySelectorAll("[data-placebo-refresh-every]")) {
    if (polls.has(element)) continue;
    const raw = element.dataset.placeboRefreshEvery, interval = Number(raw);
    const poll = { raw, interval, timer: null, overdue: false };
    polls.set(element, poll);
    if (!Number.isInteger(interval) || interval < 500 || interval > 86400000) {
      report(null, new ProtocolError("invalid-config", `Polling interval '${raw}' is not between 500 ms and a day.`), "invalid-config");
      continue;
    }
    armPoll(element, poll);
  }
}

function readUrl(form) {
  const url = new URL(form.action, document.baseURI);
  url.search = new URLSearchParams(Array.from(new FormData(form), ([name, value]) => [name, String(value)])).toString();
  return url.pathname + url.search;
}

function onReveal(entries) {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    revealer.unobserve(entry.target);
    const state = revealing.get(entry.target);
    if (!state) continue;
    state.watching = false;
    schedule(entry.target, false, null, "reveal");
  }
}

function armPoll(element, poll) {
  clearTimeout(poll.timer);
  poll.timer = setTimeout(() => {
    if (polls.get(element) !== poll) return;
    // Paused until the page is shown again, then read once.
    if (document.visibilityState === "hidden") {
      poll.overdue = true;
      emit("deferred", null, { source: "interval", reason: "page-hidden" });
      return;
    }
    runPoll(element, poll);
  }, poll.interval);
}

function runPoll(element, poll) {
  refreshPage({ source: "interval" }, { poll: true });
  armPoll(element, poll);
}

function onVisibilityChange() {
  if (document.visibilityState !== "visible") return;
  for (const [element, poll] of polls) {
    if (!poll.overdue) continue;
    poll.overdue = false;
    runPoll(element, poll);
  }
}

// A link within the site shows its page without a document load: the page is
// read, and its body replaces this one as a load would, so nothing typed on
// this page carries over to the next. Back and Forward read their page again
// and return to where it was scrolled. A link to another site, one with a
// target or download, one clicked with a modifier key, and one inside
// data-placebo-reload load as usual, as does a page that loads other scripts
// or stylesheets, an answer that is not a page, and a read that fails.
let entry = null;
const scrolls = new Map();
let visiting = null;
let progress = null;
// The page shown: a Back or Forward within it moves between its fragments.
let shown = null;
let loadedAssets = null;
const headAssets = doc =>
  Array.from(doc.head.querySelectorAll('script, style, link[rel~="stylesheet"]'), node => node.outerHTML).join("\n");
const remember = () => { if (entry) scrolls.set(entry, [scrollX, scrollY]); };

function onLinkClick(event) {
  const link = event.target.closest?.("a[href]");
  if (!(link instanceof HTMLAnchorElement) || event.defaultPrevented || event.button !== 0 ||
    event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  if ((link.target && link.target !== "_self") || link.hasAttribute("download") || link.closest("[data-placebo-reload]")) return;
  const url = new URL(link.href);
  // A place on this page: the browser scrolls there.
  if (url.origin !== location.origin || link.getAttribute("href").startsWith("#") ||
    (url.hash && url.pathname + url.search === here())) return;
  event.preventDefault();
  visit(url, "push");
}

function onPopState() {
  if (here() === shown) return;
  remember();
  entry = history.state?.placebo ?? null;
  visit(new URL(location.href), "restore");
}

async function visit(url, how) {
  if (how === "push" && url.href === location.href) how = "replace";
  visiting?.abort();
  const controller = visiting = new AbortController();
  const timer = setTimeout(showProgress, 300);
  const path = url.pathname + url.search;
  let response = null, page = null;
  try {
    response = await fetch(url, { headers: { Accept: "text/html", "X-Placebo-Refresh": String(VERSION) },
      credentials: "same-origin", cache: "no-store", signal: controller.signal });
    if (response.headers.get("content-type")?.split(";")[0].trim() === "text/html") {
      page = new DOMParser().parseFromString(await response.text(), "text/html");
    }
  } catch { /* A read that fails loads the page, and the browser shows why. */ }
  finally { clearTimeout(timer); }
  if (visiting !== controller) return;
  visiting = null;
  progress?.remove();
  progress = null;
  const reload = !page ? (response ? "not-a-page" : "network-error") : headAssets(page) !== loadedAssets ? "head-changed" : null;
  if (reload) {
    emit("navigated", null, { method: "GET", path, how, reason: reload });
    if (how === "restore") location.reload();
    else location[how === "replace" ? "replace" : "assign"](url.href);
    return;
  }
  // A redirect's page is at the address it ended at.
  const at = new URL(response.url);
  if (!response.redirected) at.hash = url.hash;
  if (how === "restore") {
    entry ??= freshKey();
    history.replaceState({ ...history.state, placebo: entry }, "", at);
  } else {
    remember();
    if (how === "push") entry = freshKey();
    history[how === "push" ? "pushState" : "replaceState"]({ placebo: entry }, "", at);
  }
  // Scripts a parser made do not run; the body's own run as on a load.
  for (const old of page.body.querySelectorAll("script")) {
    const script = document.createElement("script");
    for (const { name, value } of old.attributes) script.setAttribute(name, value);
    script.textContent = old.textContent;
    old.replaceWith(script);
  }
  document.body.replaceWith(page.body);
  shown = here();
  shownRendered = renderedAt(response) ?? 0;
  holding = new WeakSet();
  document.title = page.title;
  adoptRoot(page);
  const saved = how === "restore" ? scrolls.get(entry) : null;
  const target = at.hash ? document.getElementById(decodeURIComponent(at.hash.slice(1))) : null;
  if (saved) scrollTo(...saved);
  else if (target) target.scrollIntoView();
  else scrollTo(0, 0);
  // Focus starts at the new page, and a screen reader reads its heading.
  const start = document.querySelector("[autofocus]") ?? document.querySelector("h1") ?? document.body;
  if (!start.matches(FOCUSABLE)) start.setAttribute("tabindex", "-1");
  start.focus({ preventScroll: true });
  emit("navigated", null, { method: "GET", path: here(), how, status: response.status, reason: how });
}

// A thin bar along the top while a page is slow to come, in the site's accent.
function showProgress() {
  if (progress) return;
  progress = document.createElement("div");
  progress.setAttribute("aria-hidden", "true");
  progress.dataset.placeboProgress = "";
  progress.style.cssText = "position: fixed; inset-block-start: 0; inset-inline-start: 0; z-index: 2147483647; " +
    "block-size: 3px; inline-size: 0; background: var(--accent-bg, Highlight); transition: inline-size 8s cubic-bezier(0.1, 0.7, 0.2, 1)";
  document.documentElement.append(progress);
  progress.getBoundingClientRect();
  progress.style.inlineSize = "90%";
}

export function start() {
  if (started) return;
  started = true;
  document.addEventListener("submit", onSubmit);
  document.addEventListener("input", onInput);
  document.addEventListener("change", markEdited);
  document.addEventListener("compositionstart", onCompositionStart);
  document.addEventListener("compositionend", onCompositionEnd);
  document.addEventListener("DOMContentLoaded", onContentLoaded);
  window.addEventListener("load", auditBehaviors);
  window.addEventListener("pagehide", onPageHide);
  window.addEventListener("pageshow", onPageShow);
  document.addEventListener("visibilitychange", onVisibilityChange);
  if (!nativeCommands) document.addEventListener("click", onCommandClick);
  document.addEventListener("click", onLinkClick);
  window.addEventListener("popstate", onPopState);
  history.scrollRestoration = "manual";
  shown = here();
  loadedAssets = headAssets(document);
  entry = history.state?.placebo ?? freshKey();
  history.replaceState({ ...history.state, placebo: entry }, "");
  observer = new MutationObserver(() => {
    for (const work of pending.values()) {
      if (!work.form.isConnected || !work.target.isConnected) cancel(work, "unmounted");
    }
    syncBehaviors();
    syncFeeds();
    syncTriggers();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true,
    attributeFilter: ["data-placebo-behavior", "data-placebo-feed", "data-placebo", "data-placebo-refresh-every"] });
  syncBehaviors();
  syncFeeds();
  syncTriggers();
}

export function stop() {
  if (!started) return;
  started = false;
  document.removeEventListener("submit", onSubmit);
  document.removeEventListener("input", onInput);
  document.removeEventListener("change", markEdited);
  document.removeEventListener("compositionstart", onCompositionStart);
  document.removeEventListener("compositionend", onCompositionEnd);
  document.removeEventListener("DOMContentLoaded", onContentLoaded);
  window.removeEventListener("load", auditBehaviors);
  window.removeEventListener("pagehide", onPageHide);
  window.removeEventListener("pageshow", onPageShow);
  document.removeEventListener("visibilitychange", onVisibilityChange);
  document.removeEventListener("click", onCommandClick);
  document.removeEventListener("click", onLinkClick);
  window.removeEventListener("popstate", onPopState);
  history.scrollRestoration = "auto";
  visiting?.abort();
  visiting = null;
  progress?.remove();
  progress = null;
  clearTimeout(behaviorAudit);
  unknownBehaviors = new WeakMap();
  unresolvedCommands = new WeakMap();
  reportedClasses = new Set();
  composing = new WeakSet();
  edits = new WeakMap();
  uncertain = new WeakMap();
  holding = new WeakSet();
  observer.disconnect();
  for (const work of pending.values()) cancel(work, "runtime-stopped");
  syncBehaviors();
  syncFeeds();
  syncTriggers();
}

start();

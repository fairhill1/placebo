// Protocol v5. HTML is trusted server-rendered content, not a sanitizer input.
// Scheduling belongs to the actual mounted target node, not its reusable id.
const VERSION = 5;
const UPDATE_TYPE = "application/vnd.placebo.update+json";
const KEY_FIELD = "placebo-key";
const pending = new Map();
// Per component: a mutation sent whose outcome is unknown. Submitting the
// same form again resends it unchanged, with its idempotency key.
let uncertain = new WeakMap();
// Per component: a pushed refresh waiting for its own request to finish.
let deferredPushes = new WeakMap();
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

function auditBehaviors() {
  clearTimeout(behaviorAudit);
  if (!started || !contentLoaded) return;
  auditCommands();
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
  "missing-target": "Mount the named region before submitting, and check that the binding and mounted id agree.",
  "duplicate-target": "Give each mounted component or region a unique id.",
  "undeclared-target": "Mount this element through Region or Component so it declares the update boundary.",
  "remounted-target": "The response belongs to an older DOM instance. Read current server state before retrying a write.",
  "http-error": "Inspect the request in Network and correlate its X-Placebo-Request-Id with server logs.",
  "invalid-content-type": "Return a Placebo update from this action. Check for a login/error page or an extractor rejection in the server logs; a value that does not decode, such as a malformed number for a non-Option field, is rejected before the handler runs.",
  "unadapted-route": "Register the handler with its action's adapter: .route(ACTION.path(), ACTION.route(handler)). Plain Axum routes skip payload decoding and the mutation request check.",
  "invalid-json": "Return a complete Placebo update envelope; inspect the response in Network and the server logs.",
  "response-read-error": "Check Network and server logs for an interrupted response body. Submit the form again to retry the write safely with its idempotency key.",
  "network-error": "Check Network and server logs. The write may have committed: submit the form again to retry it safely. The retry resends the same request with its idempotency key, so the server replays its recorded reply instead of writing twice.",
  "replay-pending": "The first attempt is still running, or stopped after possibly writing. Wait and submit again, or reload to see current data.",
  "replay-unavailable": "The server's replay store failed, so nothing was saved by this attempt. Check the server logs, then submit again.",
  "replay-unknown": "The first attempt is older than the server's replay store remembers, so it may have written. Reload to see current data before submitting again. To allow later retries, keep replies longer in the ReplayStore.",
  "redirected": "Sign in again, in another tab to keep this page's input, then resubmit. If the action's handler redirects, return a Placebo update instead.",
  "response-mismatch": "Build the reply from the same action and component instance as the initiating form.",
  "invalid-update": "Check the reply operation, HTML, and patch declarations against the initiating binding.",
  "invalid-outcome": "Use reply(), invalid(), or conflict() so HTTP status and update outcome agree.",
  "invalid-patch": "Declare extra targets with affects() and use the operation for their type: also_replace for a VersionedRegion, also_append for a Region, item operations for a List, also_refresh for another Component.",
  "overlapping-targets": "Use distinct update boundaries that do not contain one another. A List may contain the components and regions it moves or removes.",
  "invalid-revision": "Mount shared snapshots with VersionedRegion::mount(revision, contents) and reply with a monotonic server revision.",
  "duplicate-append": "Insert new instances with unique ids; inserts are not an idempotent retry mechanism. Use also_move for an item that already exists.",
  "duplicate-local": "Give each retained subtree a unique key within its component. Two forms for the same action in one component cannot both render a field of the same name.",
  "local-shape": "Keep a retained local key on the same element type, or use a new key for a fresh subtree.",
  "nested-local": "Use separate local ownership boundaries; nested local subtrees are not supported.",
  "nested-component": "Mount a nested component the same way in every render (mount or mount_dialog), so its root element keeps its type.",
  "unstable-dialog": "Use component.mount_dialog(headingId, contents), or mount the component inside a persistent dialog. Replies must contain only the component contents.",
  "nested-region": "Keep shared snapshot regions free of other mounted regions.",
  "invalid-feed": "Mount the feed with feed.mount() from the same build as the runtime.",
  "missing-revision": "Give the component a revision on every mount and binding: Component::new(kind, key).revision(n), from the record the contents render.",
  "undeclared-push": "Declare the target on the Feed with .affects(target) or .affects_kind(kind), and mount the feed from that same Feed.",
  "push-disconnected": "The browser reconnects by itself. If this repeats, check the feed's route and any proxy timeouts; Network shows the event stream.",
  "push-closed": "Register the feed's route with .route(FEED.path(), FEED.route()), check the path and that it returns text/event-stream, then reload.",
  "missing-command-target": "Give the dialog or popover the id the button names, or mount it on this page. A mount_dialog component's id is its component id, such as 'task:1'.",
  "invalid-command": "Point show-modal/close/request-close at a <dialog>, and show-/hide-/toggle-popover at an element with popover.",
  "unknown-behavior": "Check the name and module import. Register with behavior() before mounting, or reserve an asynchronous import with lazyBehavior().",
  "behavior-setup": "Inspect the original cause and setup function. Return a cleanup function or undefined.",
  "behavior-cleanup": "Inspect the original cause and cleanup function; release only resources owned by this behavior.",
  "mutation-interrupted": "Aborting a request cannot undo a write. Read current server state before retrying.",
  "unsupported-method": "Use a GET read binding or a POST mutation binding with its supported request policy.",
  "cross-origin-action": "Use a same-origin action URL.",
  "cross-origin-navigation": "Navigate replies to a path on this site; link to other sites from the page instead.",
  "invalid-component": "Render the mutation form in the contents of the component it updates, not in a component nested inside it.",
  "unstable-source": "Keep a read form outside the region its response replaces.",
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
    operation: work?.config?.operation ?? null,
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
  require(typeof config.action === "string" && config.action.length > 0 &&
    typeof config.target === "string" && config.target.length > 0 &&
    ["latest", "exclusive"].includes(config.policy), "invalid-config", "Expected a named action, target, and request policy.");
  require(config.input_delay_ms === null || (Number.isInteger(config.input_delay_ms) &&
    config.input_delay_ms >= 0 && config.input_delay_ms <= 60000), "invalid-config", "Invalid input delay.");
  const read = config.policy === "latest" && config.operation === "replace-children" && form.method === "get";
  const mutation = config.policy === "exclusive" && config.operation === "refresh-component" &&
    form.method === "post" && config.input_delay_ms === null;
  require(read || mutation, "unsupported-method", "Expected a GET read binding or a POST component mutation binding.");
  require(config.history === undefined || (read && config.history === true), "invalid-config", "Only a read binding can follow history.");
  require(Array.isArray(config.effects) && config.effects.every(id => typeof id === "string" && id && id !== config.target) &&
    new Set(config.effects).size === config.effects.length,
    "invalid-config", "Additional targets must be declared once each.");
  require((config.load === undefined || config.load === true) && (config.reveal === undefined || config.reveal === true) &&
    (config.every_ms === undefined || (Number.isInteger(config.every_ms) && config.every_ms >= 500 && config.every_ms <= 86400000)) &&
    (read || (config.load === undefined && config.reveal === undefined && config.every_ms === undefined)),
    "invalid-config", "Only a read binding can read on load, when revealed, or on an interval.");
  const url = new URL(form.action, document.baseURI);
  require(url.origin === location.origin, "cross-origin-action", "Actions must use the document's origin.");
  return config;
}

function targetFor(id) {
  // Count all ids, including ordinary elements: getElementById alone hides duplicates.
  const matches = document.querySelectorAll(`#${CSS.escape(id)}`);
  require(matches.length > 0, "missing-target", `Region '${id}' is not mounted.`, { relatedTarget: id });
  require(matches.length === 1, "duplicate-target", `Region '${id}' has multiple elements with the same id.`, { relatedTarget: id });
  require(matches[0].hasAttribute("data-placebo-region"), "undeclared-target", `Element '${id}' is not a declared region.`, { relatedTarget: id });
  return matches[0];
}

function finish(work) {
  clearTimeout(work.timer);
  if (pending.get(work.target) !== work) return;
  pending.delete(work.target);
  if (work.previousBusy === null) work.target.removeAttribute("aria-busy");
  else work.target.setAttribute("aria-busy", work.previousBusy);
  if (deferredPushes.has(work.target)) queueMicrotask(() => flushDeferredPush(work.target));
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

function revision(value, target) {
  require(typeof value === "string" && /^(0|[1-9]\d*)$/.test(value), "invalid-revision", "Expected a decimal server revision.",
    { relatedTarget: target, receivedRevision: value ?? null });
  return BigInt(value);
}

// Checks shared by every batch: replaced targets swap their children;
// containers only add, move, or remove direct children, so they may hold the
// replaced targets. Inserted ids must be new.
function batch(primaryTarget = null, primaryFragment = null) {
  const replaced = primaryTarget ? [primaryTarget] : [];
  const containers = [];
  const newIds = new Set();
  let primaryIds;
  return {
    summary: { refreshedComponents: [], skippedComponents: [], deferredComponents: [], missingTargets: [],
      missingItems: [], misplacedItems: [], existingItems: [], refetched: [], skippedReads: [] },
    replace(target) {
      require(!replaced.some(other => other === target || other.contains(target) || target.contains(other)) &&
        !containers.some(container => target.contains(container)),
      "overlapping-targets", "Replaced targets must be distinct and cannot contain one another or a changed list.");
      replaced.push(target);
    },
    contain(target) {
      require(!replaced.some(other => other === target || other.contains(target)),
        "overlapping-targets", "A changed list or collection cannot be inside a replaced target.");
      containers.push(target);
    },
    reserve(content) {
      primaryIds ??= new Set(primaryFragment ? Array.from(primaryFragment.querySelectorAll("[id]"), node => node.id) : []);
      for (const node of content.querySelectorAll("[id]")) {
        require(node.id && !document.getElementById(node.id) && !newIds.has(node.id) && !primaryIds.has(node.id),
          "duplicate-append", "Inserted content must have new, unique ids.");
        newIds.add(node.id);
      }
    },
  };
}

// A versioned component orders its refreshes by revision. A reply to its own
// form applies unless older; any other refresh applies only when newer.
function componentRevision(target, incoming, strict) {
  const current = target.dataset.placeboRevision;
  require(incoming != null || current == null, "missing-revision",
    `Component '${target.id}' is mounted with a revision, so every refresh of it needs one.`, { relatedTarget: target.id });
  if (incoming == null || current == null) return true;
  const next = revision(incoming, target.id), shown = revision(current, target.id);
  return strict ? next > shown : next >= shown;
}

function componentBusy(target) {
  return pending.has(target) || Array.from(target.querySelectorAll("form")).some(form => composing.has(form));
}

// Plan one patch against the live DOM without changing it. `push` updates
// come from a feed: a busy component defers them instead of skipping them.
function planPatch(patch, target, context, { primaryId = null, push = false } = {}) {
  const { summary } = context;
  const component = target.hasAttribute("data-placebo-component");
  const list = target.hasAttribute("data-placebo-list");
  switch (patch.operation) {
    case "replace-children": {
      require(!component && !list && typeof patch.html === "string", "invalid-patch", "Snapshots replace plain versioned regions.");
      context.replace(target);
      const content = parseHTML(patch.html);
      require(!target.querySelector("[data-placebo-region]") && !content.querySelector("[data-placebo-region]"),
        "nested-region", "Shared snapshot regions cannot contain other regions.");
      const next = revision(patch.revision, patch.target);
      const current = revision(target.dataset.placeboRevision, patch.target);
      if (next <= current) return { skip: { target: patch.target, revision: patch.revision,
        currentRevision: target.dataset.placeboRevision, reason: "not-newer" } };
      const live = pairLiveRegions(target, content);
      return { commit() {
        const anchor = focusAnchor(target);
        keepLiveRegions(live);
        target.replaceChildren(content);
        target.dataset.placeboRevision = patch.revision;
        restoreFocus(target, anchor);
      } };
    }
    case "append-children": {
      require(!push && !component && !list && typeof patch.html === "string" && patch.revision == null &&
        !target.hasAttribute("data-placebo-revision"), "invalid-patch", "Append destinations must be unversioned collections.");
      context.contain(target);
      const content = parseHTML(patch.html);
      context.reserve(content);
      return { commit: () => target.append(content) };
    }
    case "refresh-component": {
      require(component && patch.target !== primaryId && typeof patch.html === "string",
        "invalid-patch", "Only another declared component can be refreshed.");
      context.replace(target);
      if (!componentRevision(target, patch.revision, true)) {
        summary.skippedComponents.push({ target: patch.target, reason: "not-newer", revision: patch.revision,
          currentRevision: target.dataset.placeboRevision });
        return {};
      }
      if (componentBusy(target)) {
        // Its own request in flight answers with its state. A pushed refresh
        // waits for that answer and applies if it is still newer.
        if (!push) { summary.skippedComponents.push({ target: patch.target, reason: "busy" }); return {}; }
        const waiting = deferredPushes.get(target)?.patch;
        if (!waiting || revision(patch.revision, patch.target) > revision(waiting.revision, patch.target)) deferredPushes.set(target, { feed: push, patch });
        summary.deferredComponents.push(patch.target);
        return {};
      }
      const prepared = prepareComponent(target, parseHTML(patch.html), "external");
      return { commit: () => {
        prepared.commit();
        if (patch.revision != null) target.dataset.placeboRevision = patch.revision;
        deferredPushes.delete(target);
        settle(target);
        summary.refreshedComponents.push(patch.target, ...prepared.components.refreshed);
        summary.skippedComponents.push(...prepared.components.skipped);
      } };
    }
    case "insert-item": case "move-item": case "remove-item": case "order-items": {
      require(list, "invalid-patch", "Item updates address a mounted List.");
      context.contain(target);
      return planItems(target, patch, context.reserve, summary);
    }
    case "rerun-read": {
      require(!push && !component && !list && !target.hasAttribute("data-placebo-revision"), "invalid-patch", "Only a read region can be fetched again.");
      return { after() {
        const forms = readFormsFor(patch.target);
        if (forms.length) { summary.refetched.push(patch.target); for (const form of forms) schedule(form, false, null, "refetch"); }
        else summary.skippedReads.push(patch.target);
      } };
    }
    default:
      throw new ProtocolError("invalid-patch", `Unknown patch operation '${patch.operation}'.`);
  }
}

function updateFragment(work, update) {
  work.phase = "validating-response";
  require(update?.version === VERSION, "version-mismatch", `Browser protocol ${VERSION} does not match response protocol ${update?.version}.`,
    { expectedVersion: VERSION, receivedVersion: update?.version ?? null });
  require(update.action === work.config.action && update.target === work.config.target,
    "response-mismatch", "The response action or target does not match the initiating action.",
    { receivedAction: update.action, receivedTarget: update.target });
  require(update.operation === work.config.operation && typeof update.html === "string",
    "invalid-update", "The response operation must match the initiating binding and contain HTML.");
  require(targetFor(update.target) === work.target, "remounted-target", "The original target instance no longer owns this response.");
  const patches = update.patches ?? [];
  require(Array.isArray(patches) && (work.method === "POST" || patches.every(patch => patch?.operation === "insert-item")),
    "invalid-update", "A read reply can only insert items into lists its binding declares.");
  require(update.navigate == null || (typeof update.navigate === "string" && work.method === "POST" && update.outcome === "applied"),
    "invalid-update", "Only a successful mutation reply can navigate.");
  const destination = update.navigate == null ? null : new URL(update.navigate, document.baseURI);
  require(!destination || destination.origin === location.origin, "cross-origin-navigation", "A reply can only navigate within this site.");
  const fragment = parseHTML(update.html);
  const component = update.operation === "refresh-component";
  // A newer refresh (a push, or another action's reply) may already show
  // later state than this reply. Then only its other patches apply.
  const newer = component && !componentRevision(work.target, update.revision, false);
  const primary = newer ? { refreshed: [], preserved: [], components: { refreshed: [], skipped: [] }, commit() {} }
    : component ? prepareComponent(work.target, fragment, update.outcome === "applied" ? "applied" : "rejected", work.locals)
    : prepareRead(work.target, fragment);
  const context = batch(work.target, fragment);
  const { summary } = context;
  if (newer) summary.skippedComponents.push({ target: update.target, reason: "not-newer", revision: update.revision,
    currentRevision: work.target.dataset.placeboRevision });
  const plans = patches.map(patch => {
    require(patch && typeof patch.target === "string" && work.effects.has(patch.target), "invalid-patch", "Patch must address a declared additional target.");
    const target = targetFor(patch.target);
    require(target === work.effects.get(patch.target), "remounted-target", `Additional target '${patch.target}' was remounted during this request.`,
      { relatedTarget: patch.target });
    return planPatch(patch, target, context, { primaryId: work.config.target });
  });
  // Every target, fragment, and local has been checked before any live mutation.
  work.phase = "applying";
  primary.commit();
  if (!newer) {
    if (component && update.revision != null) work.target.dataset.placeboRevision = update.revision;
    settle(work.target);
  }
  summary.refreshedComponents.push(...(primary.components?.refreshed ?? []));
  summary.skippedComponents.push(...(primary.components?.skipped ?? []));
  if (update.outcome === "invalid" && !newer) focusInvalid(work, primary);
  if (work.historyMode && work.historyMode !== "none") updateHistory(work);
  for (const plan of plans) plan.commit?.();
  work.applied = true;
  work.outcome = update.outcome ?? "applied";
  work.phase = "applied";
  syncBehaviors(work);
  const skipped = plans.filter(plan => plan.skip).map(plan => plan.skip);
  for (const plan of plans) plan.after?.();
  emit("applied", work, { outcome: update.outcome ?? "applied", replayed: Boolean(work.replayed), refreshedLocal: primary.refreshed, preservedLocal: primary.preserved,
    skippedRegions: skipped.map(skip => skip.target), skippedSnapshots: skipped, ...summary,
    navigate: destination ? destination.pathname + destination.search + destination.hash : null });
  if (destination) location.assign(destination.href);
}

function readFormsFor(id) {
  return Array.from(document.querySelectorAll("form[data-placebo]")).filter(form => {
    try { const config = configFor(form); return config.target === id && config.operation === "replace-children"; }
    catch { return false; }
  });
}

const POSITIONS = ["start", "end", "before", "after"];

// Items are resolved when the batch is applied. A missing item or anchor is
// skipped or placed at the end and reported, rather than rejecting a reply
// whose write has already committed.
function planItems(list, patch, reserve, summary) {
  const itemId = value => typeof value === "string" && value.length > 0;
  const position = patch.position;
  if (patch.operation === "insert-item" || patch.operation === "move-item") {
    require(position && POSITIONS.includes(position.at) && (["start", "end"].includes(position.at) ? position.item == null : itemId(position.item)),
      "invalid-patch", "An item position is start, end, or before/after an item.");
  }
  const find = id => {
    const node = document.getElementById(id);
    return node?.parentElement === list && node.hasAttribute("data-placebo-item") ? node : null;
  };
  const place = (node, id) => {
    let anchor = null;
    if (position.at === "start") anchor = list.firstElementChild;
    else if (position.at !== "end") {
      const other = find(position.item);
      if (!other) summary.misplacedItems.push(id);
      else anchor = position.at === "before" ? other : other.nextElementSibling;
    }
    if (anchor !== node) moveInto(list, node, anchor);
  };
  switch (patch.operation) {
    case "insert-item": {
      require(itemId(patch.item) && typeof patch.html === "string", "invalid-patch", "An inserted item needs an id and HTML.");
      const content = parseHTML(patch.html);
      const node = content.firstElementChild;
      require(content.childElementCount === 1 && node.id === patch.item && node.hasAttribute("data-placebo-item") &&
        !Array.from(content.childNodes).some(child => child.nodeType === Node.TEXT_NODE && child.textContent.trim()),
        "invalid-patch", "Inserted content must be exactly the mounted item.");
      // The same insert can arrive twice, from a reply and a push. The item
      // already in the list keeps its node and is reported.
      const existing = Boolean(find(patch.item));
      if (!existing) reserve(content);
      return { commit() {
        if (find(patch.item)) summary.existingItems.push(patch.item);
        else place(node, patch.item);
      } };
    }
    case "move-item":
      require(itemId(patch.item), "invalid-patch", "A moved item needs an id.");
      return { commit() {
        const node = find(patch.item);
        if (node) place(node, patch.item);
        else summary.missingItems.push(patch.item);
      } };
    case "remove-item": {
      require(itemId(patch.item), "invalid-patch", "A removed item needs an id.");
      // Earlier parts of the batch may replace the focused element first.
      const hadFocus = Boolean(find(patch.item)?.contains(document.activeElement));
      return { commit() {
        const node = find(patch.item);
        if (node) removeItem(node, hadFocus);
        else summary.missingItems.push(patch.item);
      } };
    }
    case "order-items":
      require(Array.isArray(patch.items) && patch.items.every(itemId) && new Set(patch.items).size === patch.items.length,
        "invalid-patch", "An item order lists unique item ids.");
      return { commit() {
        // Listed items first, in order; items the server did not list keep
        // their relative order after them.
        let anchor = list.firstElementChild;
        for (const id of patch.items) {
          const node = find(id);
          if (!node) { summary.missingItems.push(id); continue; }
          if (node === anchor) anchor = anchor.nextElementSibling;
          else moveInto(list, node, anchor);
        }
      } };
  }
}

// moveBefore keeps focus, open dialogs, and iframes. insertBefore detaches the
// node first, so restore focus, and the modality of an open modal dialog.
function moveInto(parent, node, before) {
  const focused = node.contains(document.activeElement) ? document.activeElement : null;
  const selection = focused && typeof focused.selectionStart === "number"
    ? [focused.selectionStart, focused.selectionEnd, focused.selectionDirection] : null;
  let moved = false;
  if (typeof parent.moveBefore === "function" && parent.isConnected && node.isConnected) {
    try { parent.moveBefore(node, before); moved = true; } catch { /* Fall back below. */ }
  }
  if (!moved) {
    const modal = [node, ...node.querySelectorAll("dialog")].filter(dialog => dialog.matches("dialog:modal"));
    parent.insertBefore(node, before);
    // Removing the attribute and showing it again fires no close event.
    for (const dialog of modal) { dialog.removeAttribute("open"); dialog.showModal(); }
  }
  // Some engines' moveBefore keeps the dialog but not focus.
  if (focused?.isConnected && document.activeElement !== focused) {
    focused.focus({ preventScroll: true });
    if (selection) focused.setSelectionRange(...selection);
  }
}

// A details element's open state and an open popover are the person's, like
// a draft: a refresh keeps them for the element with the same id.
function keepDisclosures(root) {
  const states = [];
  for (const node of root.querySelectorAll("details[id],[popover][id]")) {
    if (!ownedBy(node, root)) continue;
    states.push([node.id, node.localName === "details" ? node.open : null, node.matches(":popover-open")]);
  }
  return () => {
    for (const [id, open, shown] of states) {
      const node = root.querySelector(`#${CSS.escape(id)}`);
      if (!node || !ownedBy(node, root)) continue;
      if (open !== null && node.localName === "details") node.open = open;
      if (node.hasAttribute("popover") && node.matches(":popover-open") !== shown) {
        try { shown ? node.showPopover() : node.hidePopover(); } catch { /* A disconnected or invalid popover. */ }
      }
    }
  };
}

// Put the incoming contents into the target, with each kept node in place of
// its incoming counterpart. The incoming contents are connected first, so a
// kept node moves instead of leaving the document: focus and a modal dialog
// inside it stay where moveBefore is supported, and are restored elsewhere.
function graft(target, fragment, kept) {
  const previous = Array.from(target.childNodes);
  target.append(fragment);
  const keptNodes = new Set();
  for (const [old, next] of kept) {
    moveInto(next.parentNode, old, next);
    next.remove();
    keptNodes.add(old);
  }
  for (const node of previous) if (node.parentNode === target && !keptNodes.has(node)) node.remove();
}

const FOCUSABLE = "a[href],button:not([disabled]),input:not([disabled]):not([type=hidden]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex='-1'])";

// Keep keyboard users in the list: focus moves to a neighbouring item.
function removeItem(node, hadFocus) {
  const active = document.activeElement;
  hadFocus = node.contains(active) || (hadFocus && (!active || active === document.body));
  const neighbours = [node.nextElementSibling, node.previousElementSibling];
  node.remove();
  if (!hadFocus) return;
  for (const item of neighbours) {
    const next = item?.matches(FOCUSABLE) ? item : item?.querySelector(FOCUSABLE);
    if (next) { next.focus({ preventScroll: true }); return; }
  }
}

// A node belongs to its nearest component. Nested components own their own
// contents: a refresh of the outer one refreshes each inner one separately.
function owner(root) { return root.nodeType === Node.ELEMENT_NODE ? root : null; }
function ownedBy(node, root) {
  const component = node.closest("[data-placebo-component]");
  return root.nodeType === Node.ELEMENT_NODE && root.hasAttribute("data-placebo-component")
    ? component === root : !component || !root.contains(component);
}
function childComponents(root) {
  return new Map(Array.from(root.querySelectorAll("[data-placebo-component]"))
    .filter(child => (child.parentElement?.closest("[data-placebo-component]") ?? null) === owner(root))
    .map(child => [child.id, child]));
}

function locals(root) {
  require(!Array.from(root.querySelectorAll("dialog")).some(dialog => ownedBy(dialog, root)), "unstable-dialog",
    "A dialog inside replaceable component contents would lose its native state and listeners.");
  const found = new Map();
  for (const element of root.querySelectorAll("[data-placebo-local],[data-placebo-field]")) {
    if (!ownedBy(element, root)) continue;
    const explicit = element.hasAttribute("data-placebo-local");
    const outer = element.parentElement?.closest("[data-placebo-local]");
    const nested = outer && root.contains(outer);
    // A typed control inside an explicit local belongs to that local.
    if (!explicit && nested) continue;
    require(!nested, "nested-local", "Local subtrees cannot be nested.");
    const key = explicit ? element.dataset.placeboLocal : fieldKey(element);
    require(key && !found.has(key), "duplicate-local", `Local key '${key}' must be unique within its component.`);
    found.set(key, element);
  }
  return found;
}

// Fields are keyed by their form's action, so forms for different actions in
// one component can reuse a field name.
function fieldKey(element) {
  const action = element.closest("form")?.getAttribute("action");
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
const SYNCED = ["aria-invalid", "aria-describedby", "aria-errormessage", "disabled", "readonly", "required"];

// A local with edits the server has not accepted keeps its node. Every other
// local takes the incoming markup: after a successful save, the edits it just
// submitted; on any reply, locals nobody changed, so they show current state.
// Locals without form controls are always kept.
function prepareComponent(target, fragment, mode, snapshots = null) {
  const current = locals(target);
  const incoming = locals(fragment);
  const plans = new Map();
  const refreshed = [], preserved = [];
  // Validate the entire match before touching the live DOM.
  for (const [key, next] of incoming) {
    const old = current.get(key);
    if (!old) continue;
    require(old.tagName === next.tagName, "local-shape", `Local key '${key}' changed element type.`);
    const controls = controlsOf(old);
    const snapshot = snapshots?.get(key);
    const unchanged = !snapshots || (snapshot?.node === old && snapshot.edit === (edits.get(old) ?? 0) && snapshot.values === localValues(old));
    const edited = controls.some(changedFromDefault);
    if (controls.length && unchanged && (!edited || (mode === "applied" && snapshot.submitted))) {
      refreshed.push(key);
      plans.set(key, !edited && old.outerHTML === next.outerHTML ? "same" : "take");
    } else {
      plans.set(key, "keep");
      if (controls.length) preserved.push({ key, reason: unchanged ? "unsaved-edits" : "edited-since-submission" });
    }
  }
  // A nested component keeps its node. It takes its incoming contents by the
  // rule for a refresh from another action (edited controls stay), unless it
  // has its own request in flight, whose reply carries its state.
  const children = [], components = { refreshed: [], skipped: [] };
  const currentChildren = childComponents(target);
  for (const [id, next] of childComponents(fragment)) {
    const old = currentChildren.get(id);
    if (!old) continue;
    require(old.localName === next.localName, "nested-component",
      `Nested component '${id}' changed its root element from <${old.localName}> to <${next.localName}>.`, { relatedTarget: id });
    if (pending.has(old)) {
      components.skipped.push({ target: id, reason: "busy" });
      children.push({ old, next });
      continue;
    }
    const contents = document.createDocumentFragment();
    contents.append(...next.childNodes);
    const plan = prepareComponent(old, contents, "external");
    components.refreshed.push(id, ...plan.components.refreshed);
    components.skipped.push(...plan.components.skipped);
    children.push({ old, next, plan });
  }
  const live = pairLiveRegions(target, fragment);
  return { refreshed, preserved, components, commit() {
    const focused = document.activeElement;
    const anchor = focusAnchor(target);
    const kept = [];
    for (const [key, next] of incoming) {
      const old = current.get(key);
      const plan = plans.get(key);
      if (plan === "keep") {
        for (const name of SYNCED) {
          if (next.hasAttribute(name)) old.setAttribute(name, next.getAttribute(name));
          else old.removeAttribute(name);
        }
      }
      if (plan === "keep" || plan === "same") kept.push([old, next]);
    }
    for (const { old, next, plan } of children) {
      if (plan) { plan.commit(); settle(old); }
      kept.push([old, next]);
    }
    const disclosures = keepDisclosures(target);
    keepLiveRegions(live);
    graft(target, fragment, kept);
    disclosures();
    if (!(focused?.isConnected && target.contains(focused))) restoreFocus(target, anchor);
  } };
}

function prepareRead(target, fragment) {
  const live = pairLiveRegions(target, fragment);
  return { refreshed: [], preserved: [], commit() {
    const anchor = focusAnchor(target);
    const disclosures = keepDisclosures(target);
    keepLiveRegions(live);
    target.replaceChildren(fragment);
    disclosures();
    restoreFocus(target, anchor);
  } };
}

const LIVE = "[aria-live],[role=status],[role=alert],[role=log],output";

// Screen readers announce changes to a live region they already know, not a
// newly inserted one. Each incoming live region therefore reuses the mounted
// node it replaces, matched by id or else by order, and takes its contents.
function pairLiveRegions(root, fragment) {
  const outermost = scope => Array.from(scope.querySelectorAll(LIVE)).filter(node => {
    const outer = node.parentElement?.closest(LIVE);
    return !(outer && scope.contains(outer)) && ownedBy(node, scope) && !node.closest("[data-placebo-local],[data-placebo-field]") &&
      !node.querySelector("[data-placebo-local],[data-placebo-field],[data-placebo-region]");
  });
  const current = outermost(root), incoming = outermost(fragment);
  const pairs = [];
  const used = new Set();
  for (const next of incoming.filter(node => node.id)) {
    const old = current.find(node => node.id === next.id && node.localName === next.localName);
    if (old) { pairs.push([old, next]); used.add(old); }
  }
  const unnamedOld = current.filter(node => !node.id && !used.has(node));
  incoming.filter(node => !node.id).forEach((next, i) => {
    const old = unnamedOld[i];
    if (old?.localName === next.localName) pairs.push([old, next]);
  });
  return pairs;
}

function keepLiveRegions(pairs) {
  for (const [old, next] of pairs) {
    for (const { name } of Array.from(old.attributes)) if (!next.hasAttribute(name)) old.removeAttribute(name);
    for (const { name, value } of Array.from(next.attributes)) old.setAttribute(name, value);
    old.replaceChildren(...next.childNodes);
    next.replaceWith(old);
  }
}

function focusable(node) {
  return node.matches(FOCUSABLE) && !node.closest("[inert],[hidden]");
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

// Each distinct query gets its own entry, except that successive keystrokes
// in one text field replace the entry their first keystroke created.
let lastEntry = null;
function updateHistory(work) {
  const url = new URL(location.href);
  url.search = work.url.search;
  if (url.href === location.href) return;
  const trigger = work.historyTrigger;
  const typing = trigger && (trigger instanceof HTMLTextAreaElement ||
    (trigger instanceof HTMLInputElement && !["checkbox", "radio"].includes(trigger.type)));
  if (typing && lastEntry?.trigger === trigger && lastEntry.url === location.href) history.replaceState(history.state, "", url);
  else history.pushState(history.state, "", url);
  lastEntry = { trigger: typing ? trigger : null, url: url.href };
}

// Back and Forward put the entry's query into each history-bound read form,
// then read again without adding another entry.
function onPopState() {
  lastEntry = null;
  const params = new URLSearchParams(location.search);
  for (const form of document.querySelectorAll("form[data-placebo]")) {
    let config;
    try { config = configFor(form); } catch { continue; }
    if (config.operation !== "replace-children" || !config.history) continue;
    for (const control of form.elements) {
      if (!control.name || control.type === "hidden" || control.type === "file") continue;
      const values = params.getAll(control.name);
      const present = params.has(control.name);
      if (control instanceof HTMLSelectElement) {
        for (const option of control.options) option.selected = present ? values.includes(option.value) : option.defaultSelected;
      } else if (control.type === "checkbox" || control.type === "radio") {
        control.checked = present ? values.includes(control.value) : control.defaultChecked;
      } else if ("value" in control) control.value = present ? values[0] : control.defaultValue;
    }
    schedule(form, false, null, "history");
  }
}

function applyOrDefer(work, update) {
  const refreshed = [work.target, ...(Array.isArray(update?.patches) ? update.patches : [])
    .filter(patch => patch?.operation === "refresh-component").map(patch => work.effects.get(patch.target)).filter(Boolean)];
  if (work.config.operation === "refresh-component" &&
      refreshed.some(target => Array.from(target.querySelectorAll("form")).some(form => composing.has(form)))) {
    work.deferred = update;
    work.phase = "deferred";
    emit("deferred", work, { reason: "composing" });
    return;
  }
  work.deferred = null;
  updateFragment(work, update);
}

async function send(work) {
  if (!isCurrent(work)) return;
  work.phase = "request";
  work.sent = true;
  work.sentAt = performance.now();
  emit("request", work);
  try {
    const response = await fetch(work.url, {
      method: work.method,
      headers: { Accept: UPDATE_TYPE, "X-Placebo-Request-Id": work.requestId,
        ...(work.method === "POST" ? { "X-Placebo-Request": String(VERSION) } : {}),
        // The server runs a retry only if it would still remember the first attempt.
        ...(work.retryOf ? { "X-Placebo-Retry": String(Math.ceil(work.sentAt - work.firstSentAt)) } : {}) },
      body: work.body,
      credentials: "same-origin",
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
    // An update without the adapter's marker came from a plain Axum route. Other
    // unmarked responses (proxy errors, login pages) keep their HTTP diagnostics.
    require(adapter === work.config.action || (adapter === null && work.contentType !== UPDATE_TYPE),
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
    const expectedOutcome = response.ok ? "applied" : response.status === 422 ? "invalid" : response.status === 409 ? "conflict" : null;
    require(response.ok || (work.method === "POST" && expectedOutcome), "http-error", `Action returned HTTP ${response.status}.`);
    require(work.contentType === UPDATE_TYPE,
      "invalid-content-type", `Expected '${UPDATE_TYPE}'; received '${work.contentType ?? "no content type"}'.`);
    // Read and parse separately: engines disagree on the error response.json()
    // throws for malformed JSON (WebKit does not throw a SyntaxError).
    let body, update;
    try { body = await response.text(); }
    catch (error) {
      throw new ProtocolError("response-read-error", "Could not finish reading the update response body.", {}, error);
    }
    // JSON parser messages can contain response-body snippets; omit those.
    try { update = JSON.parse(body); }
    catch { throw new ProtocolError("invalid-json", "The update response is not valid JSON (body omitted)."); }
    // Cancellation is an optimization. This check also protects against a
    // transport that delivers an older response despite cancellation.
    if (!isCurrent(work)) return;
    if (work.method === "POST") require(update.outcome === expectedOutcome, "invalid-outcome", "Response status and update outcome disagree.");
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

function schedule(form, input, submitter = null, source = "user", trigger = null) {
  let work;
  try {
    work = attempt(form);
    const config = configFor(form, work);
    if (input && config.input_delay_ms === null) return;
    const target = targetFor(config.target);
    const effects = new Map(config.effects.map(id => [id, targetFor(id)]));
    const previous = pending.get(target);
    if (previous?.config.policy === "exclusive") {
      emit("ignored", work, { reason: "busy", activeRequestId: previous.requestId });
      return;
    }
    if (config.operation === "refresh-component") {
      const nearest = form.closest("[data-placebo-component]");
      require(target.hasAttribute("data-placebo-component") && nearest === target, "invalid-component", nearest && target.contains(nearest)
        ? `This form is inside nested component '${nearest.id}' but targets '${target.id}'. A form belongs to its nearest component.`
        : "A mutation form must belong to its target component.", { relatedTarget: nearest?.id ?? null });
    } else require(!target.contains(form) || (triggeredBy(config) && config.input_delay_ms === null), "unstable-source",
      "Place persistent read forms outside their replacement region.");
    const url = new URL(form.action, document.baseURI);
    const method = form.method.toUpperCase();
    // Submitting a form whose last attempt has an unknown outcome retries that
    // attempt exactly, so the server can replay it instead of writing twice.
    const retry = method === "POST" ? uncertain.get(target) : null;
    let body, localSnapshots;
    if (retry?.form === form) {
      body = retry.body;
      localSnapshots = retry.locals;
    } else {
      const data = new FormData(form, submitter);
      // Each new submission gets its own key. Markup rendered once for many
      // pages, such as a pushed update, carries the same key everywhere.
      if (method === "POST" && data.has(KEY_FIELD)) data.set(KEY_FIELD, freshKey());
      if (method === "POST" && form.enctype === "multipart/form-data") {
        if (!checkUploads(form, work)) return;
        body = data;
      } else {
        const fields = new URLSearchParams();
        for (const [name, value] of data) {
          require(typeof value === "string", "unsupported-file", "Files need a mutation form whose payload has an Upload field.");
          fields.append(name, value);
        }
        if (method === "GET") url.search = fields.toString();
        else body = fields;
      }
      localSnapshots = config.operation === "refresh-component" ? snapshotLocals(target, form) : new Map();
    }
    if (previous) cancel(previous, "superseded");
    work = { ...work, config, form, target, effects, locals: localSnapshots, url, method, phase: "scheduled",
      body, controller: new AbortController(),
      ...(retry?.form === form ? { retryOf: retry.requestId, firstSentAt: retry.firstSentAt } : {}),
      previousBusy: target.getAttribute("aria-busy"), timer: null,
      historyMode: config.history && source === "user" ? "update" : "none", historyTrigger: input ? trigger : null };
    pending.set(target, work);
    target.setAttribute("aria-busy", "true");
    emit("scheduled", work, retry?.form === form
      ? { source, retry: true, retryOf: retry.requestId, reason: `retrying request ${retry.requestId} unchanged, with its idempotency key` }
      : { source, ...(["load", "reveal", "interval"].includes(source) ? { reason: `read on ${source}` } : {}) });
    work.timer = setTimeout(() => send(work), input ? config.input_delay_ms : 0);
  } catch (error) {
    report(work, error, "invalid-action");
  }
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
  if (form?.hasAttribute("data-placebo") && !composing.has(form)) schedule(form, true, null, "user", event.target);
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
  let work;
  try {
    work = attempt(form);
    const config = configFor(form, work);
    if (config.input_delay_ms === null) return;
    const previous = pending.get(targetFor(config.target));
    if (previous) cancel(previous, "composing");
  } catch (error) {
    report(work, error, "invalid-action");
  }
}

function onCompositionEnd(event) {
  const form = event.target.form;
  if (!form || !composing.has(form)) return;
  composing.delete(form);
  schedule(form, true, null, "user", event.target);
  const component = form.closest("[data-placebo-component]");
  if (component && deferredPushes.has(component)) queueMicrotask(() => flushDeferredPush(component));
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

// Server push. A mounted feed element subscribes the page to its feed's
// updates, which use the same patches as replies and the same ordering by
// revision. The subscription lives as long as the element.
const feeds = new Map();
let unloading = false;

function declaredByFeed(config, id) {
  return config.targets.includes(id) || config.kinds.includes(id.split(":")[0]) && id.includes(":");
}

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
    require(typeof config.feed === "string" && typeof config.url === "string" && Array.isArray(config.targets) &&
      Array.isArray(config.kinds), "invalid-feed", "Expected a feed id, URL, and declared targets.");
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
        report(null, new ProtocolError("push-closed", `Feed '${feed.config.feed}' stopped and will not reconnect. Updates published from now on do not reach this page.`),
          "push-closed", context());
      } else if (!feed.down) {
        feed.down = true;
        report(null, new ProtocolError("push-disconnected", `Feed '${feed.config.feed}' lost its connection and is reconnecting. Updates published meanwhile are replayed or resynced when it is back.`),
          "push-disconnected", context(), "warning");
      }
    }, 50);
  });
  source.addEventListener("update", event => {
    let update;
    try { update = JSON.parse(event.data); }
    catch {
      report(null, new ProtocolError("invalid-json", "A pushed update is not valid JSON (body omitted)."), "invalid-json", context({ eventId: event.lastEventId }));
      return;
    }
    applyPush(feed, update, context({ eventId: event.lastEventId }), "push");
  });
  source.addEventListener("resync", event => resyncFeed(feed, context({ eventId: event.lastEventId })));
}

// Apply a batch of pushed patches. Targets are looked up now; a declared
// target this page does not show is skipped and reported.
function applyPush(feed, update, extra, source) {
  try {
    require(update?.version === VERSION, "version-mismatch", `Browser protocol ${VERSION} does not match pushed protocol ${update?.version}.`,
      { expectedVersion: VERSION, receivedVersion: update?.version ?? null });
    require(update.feed === feed.config.feed && Array.isArray(update.patches), "invalid-update", "A pushed update must name its feed and carry patches.");
    const context = batch();
    const plans = [];
    for (const patch of update.patches) {
      require(patch && typeof patch.target === "string" && declaredByFeed(feed.config, patch.target), "undeclared-push",
        `Feed '${feed.config.feed}' did not declare '${patch?.target}'.`, { relatedTarget: patch?.target ?? null });
      const matches = document.querySelectorAll(`#${CSS.escape(patch.target)}`);
      if (!matches.length) { context.summary.missingTargets.push(patch.target); continue; }
      require(matches.length === 1, "duplicate-target", `Region '${patch.target}' has multiple elements with the same id.`, { relatedTarget: patch.target });
      require(matches[0].hasAttribute("data-placebo-region"), "undeclared-target", `Element '${patch.target}' is not a declared region.`, { relatedTarget: patch.target });
      plans.push(planPatch(patch, matches[0], context, { push: feed }));
    }
    for (const plan of plans) plan.commit?.();
    syncBehaviors();
    const skipped = plans.filter(plan => plan.skip).map(plan => plan.skip);
    emit("applied", null, { ...extra, source, outcome: "applied", refreshedLocal: [], preservedLocal: [],
      skippedRegions: skipped.map(skip => skip.target), skippedSnapshots: skipped, ...context.summary });
  } catch (error) {
    report(null, error, "invalid-update", { ...extra, source });
  }
}

// A pushed refresh deferred while its component was busy applies once the
// component's own request settles, if it is still newer.
function flushDeferredPush(target) {
  const deferred = deferredPushes.get(target);
  if (!deferred || !target.isConnected || componentBusy(target)) return;
  deferredPushes.delete(target);
  const { feed, patch } = deferred;
  if (!feeds.has(feed.element)) return;
  applyPush(feed, { version: VERSION, feed: feed.config.feed, patches: [patch] },
    { target: feed.config.feed, feed: feed.config.feed, relatedTarget: patch.target }, "push-deferred");
}

// The server no longer has the updates this page missed. Read the page again
// and take the declared targets' newer state from it, with the same rules.
async function resyncFeed(feed, extra) {
  emit("push", null, { ...extra, phase: "push-resync" });
  let fresh;
  try {
    const response = await fetch(location.href, { headers: { Accept: "text/html" }, credentials: "same-origin" });
    require(response.ok, "http-error", `Reading the page again for feed '${feed.config.feed}' returned HTTP ${response.status}.`, { status: response.status });
    fresh = new DOMParser().parseFromString(await response.text(), "text/html");
  } catch (error) {
    report(null, error, "network-error", { ...extra, phase: "push-resync",
      hint: "Reload the page to see current data; updates published while it was disconnected are missing." });
    return;
  }
  if (!feeds.has(feed.element)) return;
  applyPush(feed, { version: VERSION, feed: feed.config.feed, patches: resyncPatches(feed.config, fresh) }, extra, "resync");
}

function resyncPatches(config, fresh) {
  const ids = new Set(config.targets);
  for (const kind of config.kinds) {
    for (const node of document.querySelectorAll(`[data-placebo-region][id^="${CSS.escape(kind)}:"]`)) ids.add(node.id);
  }
  const items = [], refreshes = [];
  for (const id of ids) {
    const current = document.getElementById(id), next = fresh.getElementById(id);
    if (!current || !next) continue;
    if (current.hasAttribute("data-placebo-list")) {
      const itemsOf = list => Array.from(list.children).filter(node => node.hasAttribute("data-placebo-item") && node.id);
      const shown = new Set(itemsOf(current).map(node => node.id));
      const wanted = itemsOf(next);
      const wantedIds = new Set(wanted.map(node => node.id));
      for (const item of shown) if (!wantedIds.has(item)) items.push({ target: id, operation: "remove-item", item });
      let previous = null;
      for (const node of wanted) {
        if (!shown.has(node.id)) items.push({ target: id, operation: "insert-item", item: node.id, html: node.outerHTML,
          position: previous ? { at: "after", item: previous } : { at: "start" } });
        previous = node.id;
      }
      items.push({ target: id, operation: "order-items", items: wanted.map(node => node.id) });
    } else if (next.dataset.placeboRevision != null) {
      const operation = current.hasAttribute("data-placebo-component") ? "refresh-component" : "replace-children";
      refreshes.push({ node: current, patch: { target: id, operation, revision: next.dataset.placeboRevision, html: next.innerHTML } });
    }
  }
  // A refreshed component already refreshes the targets inside it.
  return [...items, ...refreshes.filter(({ node }) => !refreshes.some(other => other.node !== node && other.node.contains(node)))
    .map(({ patch }) => patch)];
}

function onPageHide() { unloading = true; }
function onPageShow() { unloading = false; syncFeeds(); }

// Reads that start themselves: once on load, once when revealed, or on an
// interval. Each belongs to its form element and ends when the form goes.
const triggered = new Map();
let revealer = null;

function triggeredBy(config) {
  return Boolean(config.load || config.reveal || config.every_ms != null);
}

function triggerContext(state, extra) {
  return { action: state.config.action, target: state.config.target, ...extra };
}

function syncTriggers() {
  for (const [form, state] of triggered) {
    if (!started || !form.isConnected || form.dataset.placebo !== state.raw) stopTrigger(form, state);
  }
  if (!started) return;
  for (const form of document.querySelectorAll("form[data-placebo]")) {
    if (triggered.has(form)) continue;
    let config;
    // An invalid configuration is reported when the form is used.
    try { config = configFor(form); } catch { continue; }
    if (!triggeredBy(config)) continue;
    const state = { raw: form.dataset.placebo, config, timer: null, overdue: false, ran: false };
    triggered.set(form, state);
    if (config.load) queueMicrotask(() => { if (triggered.get(form) === state) fire(form, state, "load"); });
    if (config.reveal) {
      revealer ??= new IntersectionObserver(onReveal, { rootMargin: "200px" });
      revealer.observe(form);
    }
    if (config.every_ms != null) armInterval(form, state);
  }
}

function stopTrigger(form, state) {
  haltTrigger(form, state);
  triggered.delete(form);
}

// Ends a trigger while its form stays on the page. The form keeps its entry,
// so later DOM changes do not register and start it again.
function haltTrigger(form, state) {
  clearTimeout(state.timer);
  revealer?.unobserve(form);
  state.halted = true;
}

function fire(form, state, source) {
  if (source === "interval") {
    const target = document.getElementById(state.config.target);
    // Polling ends with its region; a region that was never there is an error.
    if (!target && state.ran) {
      haltTrigger(form, state);
      emit("discarded", null, triggerContext(state, { reason: "target-unmounted", phase: "interval-stopped" }));
      return;
    }
    // Let a slow read finish instead of cancelling it every interval.
    if (target && pending.has(target)) {
      emit("ignored", null, triggerContext(state, { reason: "busy" }));
      return;
    }
  }
  state.ran = true;
  schedule(form, false, null, source);
}

function armInterval(form, state) {
  clearTimeout(state.timer);
  state.timer = setTimeout(() => {
    if (triggered.get(form) !== state || state.halted) return;
    // Paused until the page is shown again, then read once.
    if (document.visibilityState === "hidden") {
      state.overdue = true;
      emit("deferred", null, triggerContext(state, { reason: "page-hidden" }));
      return;
    }
    fire(form, state, "interval");
    if (triggered.get(form) === state && !state.halted) armInterval(form, state);
  }, state.config.every_ms);
}

function onVisibilityChange() {
  if (document.visibilityState !== "visible") return;
  for (const [form, state] of triggered) {
    if (!state.overdue || state.halted) continue;
    state.overdue = false;
    fire(form, state, "interval");
    if (triggered.get(form) === state && !state.halted) armInterval(form, state);
  }
}

function onReveal(entries) {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    revealer.unobserve(entry.target);
    const state = triggered.get(entry.target);
    if (state) fire(entry.target, state, "reveal");
  }
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
  window.addEventListener("popstate", onPopState);
  window.addEventListener("pagehide", onPageHide);
  window.addEventListener("pageshow", onPageShow);
  document.addEventListener("visibilitychange", onVisibilityChange);
  observer = new MutationObserver(() => {
    for (const work of pending.values()) {
      if (!work.form.isConnected || !work.target.isConnected) cancel(work, "unmounted");
    }
    syncBehaviors();
    syncFeeds();
    syncTriggers();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true,
    attributeFilter: ["data-placebo-behavior", "data-placebo-feed", "data-placebo"] });
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
  window.removeEventListener("popstate", onPopState);
  window.removeEventListener("pagehide", onPageHide);
  window.removeEventListener("pageshow", onPageShow);
  document.removeEventListener("visibilitychange", onVisibilityChange);
  clearTimeout(behaviorAudit);
  unknownBehaviors = new WeakMap();
  unresolvedCommands = new WeakMap();
  composing = new WeakSet();
  edits = new WeakMap();
  uncertain = new WeakMap();
  deferredPushes = new WeakMap();
  observer.disconnect();
  for (const work of pending.values()) cancel(work, "runtime-stopped");
  syncBehaviors();
  syncFeeds();
  syncTriggers();
}

start();

// Protocol v3. HTML is trusted server-rendered content, not a sanitizer input.
// Scheduling belongs to the actual mounted target node, not its reusable id.
const VERSION = 3;
const UPDATE_TYPE = "application/vnd.placebo.update+json";
const pending = new Map();
let composing = new WeakSet();
let started = false;
let observer;
let edits = new WeakMap();
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

function auditBehaviors() {
  clearTimeout(behaviorAudit);
  if (!started || document.readyState === "loading") return;
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
  "invalid-content-type": "Return a Placebo update from this action. Check for a login/error page or an extractor rejection in the server logs.",
  "invalid-json": "Return a complete Placebo update envelope; inspect the response in Network and the server logs.",
  "response-read-error": "Check Network and server logs for an interrupted response body. Read current state before retrying a write.",
  "network-error": "Check Network and server logs. A dispatched write may have committed; read current state before retrying.",
  "response-mismatch": "Build the reply from the same action and component instance as the initiating form.",
  "invalid-update": "Check the reply operation, HTML, and reset/patch declarations against the initiating binding.",
  "invalid-outcome": "Use reply(), invalid(), or conflict() so HTTP status and update outcome agree.",
  "invalid-patch": "Declare extra regions with affects() and use the matching region update operation.",
  "overlapping-targets": "Use distinct update boundaries that do not contain one another.",
  "invalid-revision": "Mount shared snapshots with VersionedRegion::mount(revision, contents) and reply with a monotonic server revision.",
  "duplicate-append": "Append new instances with unique ids; appends are not an idempotent retry mechanism.",
  "duplicate-local": "Give each retained subtree a unique key within its component.",
  "missing-local": "Reset only a local key present in both the mounted and incoming component.",
  "local-shape": "Keep a retained local key on the same element type, or use a new key for a fresh subtree.",
  "nested-local": "Use separate local ownership boundaries; nested local subtrees are not supported.",
  "nested-component": "Refresh separate component instances; nested component refreshes are not supported yet.",
  "unstable-dialog": "Use component.mount_dialog(headingId, contents), or mount the component inside a persistent dialog. Replies must contain only the component contents.",
  "nested-region": "Keep shared snapshot regions free of other mounted regions.",
  "unknown-behavior": "Check the name and module import. Register with behavior() before mounting, or reserve an asynchronous import with lazyBehavior().",
  "behavior-setup": "Inspect the original cause and setup function. Return a cleanup function or undefined.",
  "behavior-cleanup": "Inspect the original cause and cleanup function; release only resources owned by this behavior.",
  "mutation-interrupted": "Aborting a request cannot undo a write. Read current server state before retrying.",
  "unsupported-method": "Use a GET read binding or a POST mutation binding with its supported request policy.",
  "cross-origin-action": "Use a same-origin action URL.",
  "invalid-component": "Mount the mutation form inside the component it updates.",
  "unstable-source": "Keep a read form outside the region its response replaces.",
  "unsupported-file": "File submission is not supported by this form protocol yet.",
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
    requestState: !work?.sent ? "not-started" : work.status == null ? "started" : "response-received",
    updateState: work?.applied ? "applied" : work?.phase === "applying" ? "possibly-partial" : "not-applied",
    writeState: work?.method !== "POST" ? "not-applicable" : !work.sent ? "not-started" :
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
  let config;
  try { config = JSON.parse(form.dataset.placebo); }
  catch { throw new ProtocolError("invalid-config", "The action configuration is not valid JSON."); }
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
  require(Array.isArray(config.effects) && config.effects.every(id => typeof id === "string" && id && id !== config.target) &&
    new Set(config.effects).size === config.effects.length && (mutation || config.effects.length === 0),
    "invalid-config", "Additional regions must be uniquely declared by a mutation binding.");
  const url = new URL(form.action, document.baseURI);
  require(url.origin === location.origin, "cross-origin-action", "Actions must use the document's origin.");
  return config;
}

function targetFor(id) {
  // Count all ids, including ordinary elements: getElementById alone hides duplicates.
  const matches = Array.from(document.querySelectorAll("[id]")).filter(node => node.id === id);
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
  const resets = update.reset_local ?? [];
  const patches = update.patches ?? [];
  require(Array.isArray(resets) && resets.every(key => typeof key === "string" && key) && new Set(resets).size === resets.length &&
    Array.isArray(patches) && (work.method === "POST" || (!resets.length && !patches.length)) &&
    (!resets.length || update.outcome === "applied"), "invalid-update", "Invalid local resets or additional patches.");
  const fragment = parseHTML(update.html);
  const primary = update.operation === "refresh-component"
    ? prepareComponent(work, fragment, resets)
    : { commit: () => work.target.replaceChildren(fragment), resetLocal: [] };
  const targets = [work.target];
  const appendedIds = new Set();
  const plans = patches.map(patch => {
    require(patch && work.effects.has(patch.target) && typeof patch.html === "string" &&
      ["replace-children", "append-children"].includes(patch.operation), "invalid-patch", "Patch must address a declared additional region.");
    const target = targetFor(patch.target);
    require(target === work.effects.get(patch.target), "remounted-target", `Additional region '${patch.target}' was remounted during this request.`,
      { relatedTarget: patch.target });
    require(!targets.some(other => other === target || other.contains(target) || target.contains(other)),
      "overlapping-targets", "Update targets must be distinct and cannot contain one another.");
    require(!target.hasAttribute("data-placebo-component"), "invalid-patch", "Additional patches address plain regions.");
    targets.push(target);
    const content = parseHTML(patch.html);
    if (patch.operation === "replace-children") {
      require(!target.querySelector("[data-placebo-region]") && !content.querySelector("[data-placebo-region]"),
        "nested-region", "Shared snapshot regions cannot contain other regions.");
      const next = revision(patch.revision, patch.target);
      const current = revision(target.dataset.placeboRevision, patch.target);
      return { target: patch.target, stale: next <= current, revision: patch.revision,
        currentRevision: target.dataset.placeboRevision, commit() {
        target.replaceChildren(content);
        target.dataset.placeboRevision = patch.revision;
      } };
    }
    require(patch.revision == null && !target.hasAttribute("data-placebo-revision"),
      "invalid-patch", "Append destinations must be unversioned collections.");
    for (const node of content.querySelectorAll("[id]")) {
      require(node.id && !document.getElementById(node.id) && !appendedIds.has(node.id) &&
        !Array.from(fragment.querySelectorAll("[id]")).some(other => other.id === node.id),
        "duplicate-append", "Appended content must have new, unique ids.");
      appendedIds.add(node.id);
    }
    return { target: patch.target, commit: () => target.append(content) };
  });
  // Every target, fragment, and reset has been checked before any live mutation.
  work.phase = "applying";
  primary.commit();
  for (const plan of plans) if (!plan.stale) plan.commit();
  work.applied = true;
  work.outcome = update.outcome ?? "applied";
  work.phase = "applied";
  syncBehaviors(work);
  emit("applied", work, { outcome: update.outcome ?? "applied", resetLocal: primary.resetLocal,
    preservedLocal: resets.filter(key => !primary.resetLocal.includes(key)).map(key => ({ key, reason: "edited-since-submission" })),
    skippedRegions: plans.filter(plan => plan.stale).map(plan => plan.target),
    skippedSnapshots: plans.filter(plan => plan.stale).map(plan => ({ target: plan.target, revision: plan.revision,
      currentRevision: plan.currentRevision, reason: "not-newer" })) });
}

function locals(root) {
  require(!root.querySelector("[data-placebo-component]"), "nested-component", "Nested component refresh is not supported yet.");
  require(!root.querySelector("dialog"), "unstable-dialog", "A dialog inside replaceable component contents would lose its native state and listeners.");
  const found = new Map();
  for (const element of root.querySelectorAll("[data-placebo-local]")) {
    const key = element.dataset.placeboLocal;
    require(key && !found.has(key), "duplicate-local", `Local key '${key}' must be unique within its component.`);
    require(!element.parentElement?.closest("[data-placebo-local]"), "nested-local", "Local subtrees cannot be nested.");
    found.set(key, element);
  }
  return found;
}

function localValues(node) {
  return JSON.stringify(Array.from(node.querySelectorAll("input,textarea,select"), input =>
    [input.name, input.value, input.checked, input instanceof HTMLSelectElement ? Array.from(input.selectedOptions, option => option.value) : null]));
}

function prepareComponent(work, fragment, resets) {
  const target = work.target;
  const current = locals(target);
  const incoming = locals(fragment);
  // Validate the entire match before touching the live DOM.
  for (const [key, next] of incoming) {
    const old = current.get(key);
    require(!old || old.tagName === next.tagName, "local-shape", `Local key '${key}' changed element type.`);
  }
  const resetLocal = [];
  for (const key of resets) {
    require(current.has(key) && incoming.has(key), "missing-local", `Reset key '${key}' must exist in both versions.`);
    const node = current.get(key);
    const snapshot = work.locals.get(key);
    if (snapshot?.node === node && snapshot.edit === (edits.get(node) ?? 0) && snapshot.values === localValues(node)) resetLocal.push(key);
  }
  const focused = document.activeElement;
  const retainedFocus = Array.from(current).some(([key, node]) => incoming.has(key) && !resetLocal.includes(key) && node.contains(focused));
  const selection = retainedFocus && typeof focused.selectionStart === "number"
    ? [focused.selectionStart, focused.selectionEnd, focused.selectionDirection] : null;
  const resetFocusKey = resetLocal.find(key => current.get(key).contains(focused));
  const replacementFocus = resetFocusKey && focused.id
    ? Array.from(incoming.get(resetFocusKey).querySelectorAll("[id]")).find(node => node.id === focused.id) : null;
  return { resetLocal, commit() {
    for (const [key, next] of incoming) {
      const old = current.get(key);
      if (old && !resetLocal.includes(key)) next.replaceWith(old);
    }
    target.replaceChildren(fragment);
    if (retainedFocus && focused.isConnected) {
      focused.focus({ preventScroll: true });
      if (selection) focused.setSelectionRange(...selection);
    } else if (replacementFocus?.isConnected) replacementFocus.focus({ preventScroll: true });
  } };
}

function applyOrDefer(work, update) {
  if (work.config.operation === "refresh-component" &&
      Array.from(work.target.querySelectorAll("form")).some(form => composing.has(form))) {
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
  emit("request", work);
  try {
    const response = await fetch(work.url, {
      method: work.method,
      headers: { Accept: UPDATE_TYPE, "X-Placebo-Request-Id": work.requestId,
        ...(work.method === "POST" ? { "X-Placebo-Request": String(VERSION) } : {}) },
      body: work.body,
      credentials: "same-origin",
      redirect: "error",
      signal: work.controller.signal,
    });
    work.status = response.status;
    work.contentType = response.headers.get("content-type")?.split(";")[0].trim() ?? null;
    work.phase = "response";
    if (!isCurrent(work)) return;
    const expectedOutcome = response.ok ? "applied" : response.status === 422 ? "invalid" : response.status === 409 ? "conflict" : null;
    require(response.ok || (work.method === "POST" && expectedOutcome), "http-error", `Action returned HTTP ${response.status}.`);
    require(work.contentType === UPDATE_TYPE,
      "invalid-content-type", `Expected '${UPDATE_TYPE}'; received '${work.contentType ?? "no content type"}'.`);
    let update;
    try { update = await response.json(); }
    catch (error) {
      // JSON parser messages can contain response-body snippets; omit those.
      if (error instanceof SyntaxError) throw new ProtocolError("invalid-json", "The update response is not valid JSON (body omitted).");
      throw new ProtocolError("response-read-error", "Could not finish reading the update response body.", {}, error);
    }
    // Cancellation is an optimization. This check also protects against a
    // transport that delivers an older response despite cancellation.
    if (!isCurrent(work)) return;
    if (work.method === "POST") require(update.outcome === expectedOutcome, "invalid-outcome", "Response status and update outcome disagree.");
    applyOrDefer(work, update);
  } catch (error) {
    if (work.controller.signal.aborted || !isCurrent(work)) return;
    report(work, error, work.phase === "request" ? "network-error" : "invalid-update");
  } finally {
    if (!work.deferred) finish(work);
  }
}

function attempt(form) {
  return { config: {}, form, phase: "preflight", method: form.method.toUpperCase(),
    // Exclude query strings and fragments: they can contain user input.
    path: new URL(form.action, document.baseURI).pathname,
    requestId: crypto.randomUUID?.() ?? `p-${Date.now().toString(36)}-${crypto.getRandomValues(new Uint32Array(1))[0].toString(36)}` };
}

function schedule(form, input, submitter = null) {
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
      require(target.hasAttribute("data-placebo-component") && target.contains(form),
        "invalid-component", "A mutation form must belong to its target component.");
    } else require(!target.contains(form), "unstable-source", "Place persistent read forms outside their replacement region.");
    const data = new FormData(form, submitter);
    const url = new URL(form.action, document.baseURI);
    const fields = new URLSearchParams();
    for (const [name, value] of data) {
      require(typeof value === "string", "unsupported-file", "File submission is not supported yet.");
      fields.append(name, value);
    }
    const method = form.method.toUpperCase();
    if (method === "GET") url.search = fields.toString();
    if (previous) cancel(previous, "superseded");
    const localSnapshots = config.operation === "refresh-component"
      ? new Map(Array.from(locals(target), ([key, node]) => [key, { node, edit: edits.get(node) ?? 0, values: localValues(node) }])) : new Map();
    work = { ...work, config, form, target, effects, locals: localSnapshots, url, method, phase: "scheduled",
      body: method === "POST" ? fields : undefined, controller: new AbortController(),
      previousBusy: target.getAttribute("aria-busy"), timer: null };
    pending.set(target, work);
    target.setAttribute("aria-busy", "true");
    emit("scheduled", work);
    work.timer = setTimeout(() => send(work), input ? config.input_delay_ms : 0);
  } catch (error) {
    report(work, error, "invalid-action");
  }
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
  const local = event.target.closest?.("[data-placebo-local]");
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
  schedule(form, true);
  for (const work of pending.values()) {
    if (!work.deferred || !isCurrent(work)) continue;
    try { applyOrDefer(work, work.deferred); }
    catch (error) {
      work.deferred = null;
      report(work, error, "invalid-update");
    } finally {
      if (!work.deferred) finish(work);
    }
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
  document.addEventListener("DOMContentLoaded", auditBehaviors);
  observer = new MutationObserver(() => {
    for (const work of pending.values()) {
      if (!work.form.isConnected || !work.target.isConnected) cancel(work, "unmounted");
    }
    syncBehaviors();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-placebo-behavior"] });
  syncBehaviors();
}

export function stop() {
  if (!started) return;
  started = false;
  document.removeEventListener("submit", onSubmit);
  document.removeEventListener("input", onInput);
  document.removeEventListener("change", markEdited);
  document.removeEventListener("compositionstart", onCompositionStart);
  document.removeEventListener("compositionend", onCompositionEnd);
  document.removeEventListener("DOMContentLoaded", auditBehaviors);
  clearTimeout(behaviorAudit);
  unknownBehaviors = new WeakMap();
  composing = new WeakSet();
  edits = new WeakMap();
  observer.disconnect();
  for (const work of pending.values()) cancel(work, "runtime-stopped");
  syncBehaviors();
}

start();

import assert from "node:assert/strict";
import { test, after } from "node:test";
import { mkdir, writeFile } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";
import { serverFixture } from "./fixture.mjs";

const fixture = serverFixture("tasks");
const evidence = [];
after(async () => {
  await mkdir("test-results", { recursive: true });
  await writeFile("test-results/diagnostics-after.json", JSON.stringify(evidence, null, 2));
});

test("a body stream failure is distinguished from malformed JSON", async t => {
  const audit = await visit(t);
  await audit.page.evaluate(() => {
    const nativeFetch = window.fetch;
    window.fetch = (url, options) => new URL(url).pathname === "/actions/save-task"
      ? Promise.resolve(new Response(new ReadableStream({ start(controller) { controller.error(new Error("Body stream interrupted")); } }),
        { status: 200, headers: { "content-type": "application/vnd.placebo.update+json" } }))
      : nativeFetch(url, options);
  });
  await submit(audit.page);
  const { detail } = await error(audit, "response-read-error");
  assert.equal(detail.status, 200);
  assert.equal(detail.requestState, "response-received");
  assert.equal(detail.writeState, "unknown");
});

async function visit(t, { trace = false } = {}) {
  const page = await fixture.page(t);
  const logs = [], captures = [];
  page.on("console", message => {
    if (!message.text().startsWith("[placebo:")) return;
    const capture = Promise.all(message.args().map(arg => arg.evaluate(value => value instanceof Error
      ? { name: value.name, message: value.message, stack: value.stack } : value)))
      .then(args => logs.push({ level: message.type(), text: message.text(), detail: args[1], cause: args[2] }));
    captures.push(capture);
  });
  if (trace) await page.addInitScript(() => localStorage.setItem("placebo:trace", "true"));
  await page.goto(fixture.origin);
  t.after(async () => { await Promise.all(captures); });
  return { page, logs, captures };
}

async function logged(audit, predicate) {
  for (let attempt = 0; attempt < 200; attempt++) {
    const found = audit.logs.find(predicate);
    if (found) return found;
    await delay(20);
  }
  assert.fail(`Expected console diagnostic. Got ${JSON.stringify(audit.logs)}`);
}
async function error(audit, code) {
  const found = await logged(audit, log => log.level === "error" && log.detail.code === code);
  assert.ok(found.detail.hint?.length > 15);
  evidence.push({ scenario: code, ...found });
  return found;
}
async function submit(page, title = "Diagnostics save") {
  await page.locator('[data-task="1"] [data-dialog-open]').click();
  await page.locator("#title-1").fill(title);
  await page.locator("#title-1").press("Enter");
}
async function settle(audit) {
  await audit.page.evaluate(() => new Promise(resolve => setTimeout(() => setTimeout(resolve, 0), 0)));
  await Promise.all(audit.captures);
}

test("unknown behavior is visible once without enabling trace and can recover", async t => {
  const audit = await visit(t);
  const { page } = audit;
  await settle(audit);
  assert.deepEqual(audit.logs, [], "normal initial module registration is quiet");
  await page.evaluate(() => {
    const node = document.createElement("div");
    node.id = "forgotten-widget";
    node.dataset.placeboBehavior = "dialog-typo";
    document.body.append(node);
  });
  const log = await error(audit, "unknown-behavior");
  assert.equal(log.detail.behavior, "dialog-typo");
  assert.equal(log.detail.element, "div#forgotten-widget");
  assert.match(log.text, /inactive/);
  await page.evaluate(() => document.body.append(document.createElement("hr")));
  await settle(audit);
  assert.equal(audit.logs.length, 1, "unrelated DOM changes do not spam the same diagnostic");
  await page.evaluate(async () => {
    const { behavior } = await import("/placebo.js");
    behavior("dialog-typo", node => { node.textContent = "Mounted after registration"; });
  });
  assert.equal(await page.locator("#forgotten-widget").textContent(), "Mounted after registration");
});

test("missing target explains that no request was started and names the affected region", async t => {
  const audit = await visit(t);
  await audit.page.evaluate(() => document.querySelector("#task-count").remove());
  await submit(audit.page);
  const { detail, text } = await error(audit, "missing-target");
  assert.equal(detail.action, "save-task");
  assert.equal(detail.target, "task:1");
  assert.equal(detail.relatedTarget, "task-count");
  assert.equal(detail.method, "POST");
  assert.equal(detail.path, "/actions/save-task");
  assert.equal(detail.requestState, "not-started");
  assert.equal(detail.updateState, "not-applied");
  assert.equal(detail.writeState, "not-started");
  assert.match(text, /Mount/);
});

test("invalid form configuration retains useful preflight context", async t => {
  const audit = await visit(t);
  await audit.page.evaluate(() => {
    const form = document.querySelector("#title-1").form;
    const config = JSON.parse(form.dataset.placebo); config.version = 999;
    form.dataset.placebo = JSON.stringify(config);
    form.requestSubmit();
  });
  const { detail } = await error(audit, "version-mismatch");
  assert.equal(detail.action, "save-task");
  assert.equal(detail.expectedVersion, 3);
  assert.equal(detail.receivedVersion, 999);
  assert.equal(detail.requestState, "not-started");
});

for (const [scenario, code, status, contentType] of [
  ["http-500", "http-error", 500, "text/plain"],
  ["html-response", "invalid-content-type", 200, "text/html"],
  ["invalid-json", "invalid-json", 200, "application/vnd.placebo.update+json"],
  ["version-mismatch", "version-mismatch", 200, "application/vnd.placebo.update+json"],
]) {
  test(`${scenario} includes request context, consequence, and a next step`, async t => {
    const audit = await visit(t);
    let sentId;
    await audit.page.route("**/actions/save-task", route => {
      sentId = route.request().headers()["x-placebo-request-id"];
      return route.fulfill({ status, contentType, body: scenario === "version-mismatch"
        ? JSON.stringify({ version: 999, outcome: "applied" }) : "RESPONSE_SECRET must not appear in diagnostics" });
    });
    await submit(audit.page, "FORM_SECRET must stay out of logs");
    const { detail, text } = await error(audit, code);
    assert.equal(detail.requestId, sentId);
    assert.equal(detail.action, "save-task");
    assert.equal(detail.method, "POST");
    assert.equal(detail.path, "/actions/save-task");
    assert.equal(detail.status, status);
    assert.equal(detail.contentType, contentType);
    assert.equal(detail.requestState, "response-received");
    assert.equal(detail.updateState, "not-applied");
    assert.equal(detail.writeState, "unknown");
    assert.match(text, /Next:/);
    assert.ok(!JSON.stringify(audit.logs).includes("FORM_SECRET"));
    assert.ok(!JSON.stringify(audit.logs).includes("RESPONSE_SECRET"));
    if (scenario === "version-mismatch") {
      assert.equal(detail.expectedVersion, 3);
      assert.equal(detail.receivedVersion, 999);
    }
  });
}

test("a lost response reports uncertainty and correlates with the completed server request", async t => {
  const audit = await visit(t);
  let sentId, echoedId;
  await audit.page.route("**/actions/save-task", async route => {
    sentId = route.request().headers()["x-placebo-request-id"];
    const response = await route.fetch();
    echoedId = response.headers()["x-placebo-request-id"];
    assert.equal(response.status(), 200);
    await route.abort("failed");
  });
  await submit(audit.page, "Committed before losing the response");
  const { detail, text, cause } = await error(audit, "network-error");
  assert.equal(detail.requestId, sentId);
  assert.equal(echoedId, sentId);
  assert.equal(detail.writeState, "unknown");
  assert.equal(detail.status, null);
  assert.match(text, /read current state before retrying/i);
  assert.match(cause.stack, /send/);
  assert.ok(fixture.serverLog.includes(`request=${sentId} POST /actions/save-task HTTP 200`));
  await audit.page.reload();
  assert.equal(await audit.page.locator('[data-task="1"] .task-title').textContent(), "Committed before losing the response");
});

test("remounted secondary target identifies the old ownership without hiding the write uncertainty", async t => {
  const audit = await visit(t);
  let committed;
  const received = new Promise(resolve => { committed = resolve; });
  let release;
  const hold = new Promise(resolve => { release = resolve; });
  t.after(() => release());
  await audit.page.route("**/actions/save-task", async route => {
    const response = await route.fetch(); committed();
    await hold; await route.fulfill({ response });
  });
  await submit(audit.page);
  await received;
  await audit.page.evaluate(() => {
    const summary = document.querySelector("#task-count"); summary.replaceWith(summary.cloneNode(true));
  });
  release();
  const { detail } = await error(audit, "remounted-target");
  assert.equal(detail.relatedTarget, "task-count");
  assert.equal(detail.writeState, "unknown");
  assert.equal(detail.updateState, "not-applied");
});

test("unmounting a dispatched mutation warns by default even with trace disabled", async t => {
  const audit = await visit(t);
  await audit.page.locator('[data-task="1"] [data-dialog-open]').click();
  await audit.page.locator("#delay-1").selectOption("700");
  const request = audit.page.waitForRequest("**/actions/save-task");
  await audit.page.locator("#title-1").press("Enter");
  await request;
  await audit.page.evaluate(() => document.querySelector('[id="task:1"]').remove());
  const log = await logged(audit, log => log.level === "warning" && log.detail.code === "mutation-interrupted");
  evidence.push({ scenario: "mutation-interrupted", ...log });
  assert.equal(log.detail.writeState, "unknown");
  assert.equal(log.detail.reason, "unmounted");
  assert.match(log.text, /cannot undo a write/);
});

for (const phase of ["setup", "cleanup"]) {
  test(`behavior ${phase} errors name the behavior/element and preserve the original stack`, async t => {
    const audit = await visit(t);
    await audit.page.evaluate(async phase => {
      const { behavior } = await import("/placebo.js");
      const node = document.createElement("div"); node.id = "broken-widget"; node.dataset.placeboBehavior = "broken";
      behavior("broken", function setupWidget() {
        if (phase === "setup") throw new Error("setup failed");
        return function cleanupWidget() { throw new Error("cleanup failed"); };
      });
      document.body.append(node);
      await new Promise(resolve => setTimeout(resolve, 0));
      if (phase === "cleanup") node.remove();
    }, phase);
    const { detail, cause } = await error(audit, `behavior-${phase}`);
    assert.equal(detail.behavior, "broken");
    assert.equal(detail.element, "div#broken-widget");
    assert.match(cause.stack, new RegExp(`${phase}Widget`));
  });
}

test("lazy behavior reserves its name, mounts after loading, and diagnoses rejected/stalled loaders", async t => {
  const audit = await visit(t, { trace: true });
  await audit.page.evaluate(async () => {
    const { lazyBehavior } = await import("/placebo.js");
    lazyBehavior("later", () => new Promise(resolve => { window.finishBehavior = resolve; }));
    const node = document.createElement("div"); node.id = "lazy-widget"; node.dataset.placeboBehavior = "later";
    document.body.append(node);
  });
  await logged(audit, log => log.detail.behavior === "later" && log.detail.phase === "behavior-loading");
  await settle(audit);
  assert.ok(!audit.logs.some(log => log.level === "error"));
  await audit.page.evaluate(() => finishBehavior(node => { node.textContent = "Ready"; }));
  await logged(audit, log => log.detail.behavior === "later" && log.detail.phase === "behavior-mounted");
  assert.equal(await audit.page.locator("#lazy-widget").textContent(), "Ready");
  await audit.page.evaluate(async () => {
    const { lazyBehavior } = await import("/placebo.js");
    lazyBehavior("broken-import", () => Promise.reject(new Error("Import failed")));
    lazyBehavior("stalled", () => new Promise(() => {}), { timeoutMs: 25 });
  });
  assert.equal((await error(audit, "behavior-load")).detail.behavior, "broken-import");
  assert.equal((await error(audit, "behavior-timeout")).detail.behavior, "stalled");
});

test("console trace exposes lifecycle and duplicate suppression, can turn off, and excludes input/query data", async t => {
  const audit = await visit(t);
  await audit.page.evaluate(async () => {
    (await import("/placebo.js")).trace(true);
    document.querySelector("#title-1").form.action += "?private=QUERY_SECRET";
  });
  await audit.page.locator('[data-task="1"] [data-dialog-open]').click();
  await audit.page.locator("#delay-1").selectOption("700");
  await audit.page.locator("#title-1").fill("FORM_SECRET private input");
  await audit.page.locator("#title-1").press("Enter");
  const sent = await logged(audit, log => log.detail.phase === "request");
  await audit.page.locator("#title-1").press("Enter");
  const ignored = await logged(audit, log => log.detail.reason === "busy");
  assert.equal(ignored.detail.activeRequestId, sent.detail.requestId);
  const applied = await logged(audit, log => log.detail.updateState === "applied");
  assert.equal(applied.detail.requestId, sent.detail.requestId);
  assert.equal(applied.detail.writeState, "acknowledged");
  assert.ok(!JSON.stringify(audit.logs).includes("FORM_SECRET"));
  assert.ok(!JSON.stringify(audit.logs).includes("QUERY_SECRET"));
  assert.ok(!fixture.serverLog.includes("FORM_SECRET"));
  assert.ok(!fixture.serverLog.includes("QUERY_SECRET"));
  await audit.page.evaluate(async () => (await import("/placebo.js")).trace(false));
  const length = audit.logs.length;
  await submit(audit.page, "x");
  await audit.page.waitForFunction(() => document.querySelector("#feedback-1").textContent.includes("between 3"));
  await settle(audit);
  assert.equal(audit.logs.length, length, "ordinary validation does not become a console error");
});
